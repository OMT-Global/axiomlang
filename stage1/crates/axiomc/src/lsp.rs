use crate::diagnostics::Diagnostic;
use crate::hir;
use crate::manifest::load_manifest;
use crate::mir;
use crate::project::{package_graph_metadata, project_capabilities};
use crate::syntax;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const TEXT_DOCUMENT_SYNC_KIND_INCREMENTAL: u8 = 2;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_WORKSPACE_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORKSPACE_DOCUMENTS: usize = 2048;
const MAX_LATENCY_SAMPLES: usize = 128;
const DOCUMENT_METADATA_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct LspResponse {
    pub messages: Vec<Value>,
    pub exit: bool,
}

#[derive(Debug, Clone)]
struct LspDocument {
    version: i64,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LspSymbol {
    name: String,
    kind: &'static str,
    uri: String,
    line: usize,
    column: usize,
    detail: String,
}

#[derive(Debug, Default)]
struct WorkspaceIndex {
    documents: BTreeMap<String, String>,
    symbols: Vec<LspSymbol>,
}

/// Stateful, bounded LSP service. The public protocol only exposes compiler
/// package/semantic data, never Rust implementation details.
#[derive(Debug, Default)]
pub struct LspServer {
    documents: BTreeMap<String, LspDocument>,
    workspace_roots: BTreeSet<PathBuf>,
    cancelled_requests: BTreeSet<String>,
    workspace_generation: u64,
    last_analysis_ms: u128,
    analysis_latency_samples_ms: VecDeque<u128>,
}

impl LspServer {
    pub fn handle_message(&mut self, payload: &str) -> Result<LspResponse, Diagnostic> {
        let value: Value = serde_json::from_str(payload)
            .map_err(|err| Diagnostic::new("lsp", format!("invalid JSON-RPC payload: {err}")))?;
        self.handle_value(&value)
    }

    fn handle_value(&mut self, value: &Value) -> Result<LspResponse, Diagnostic> {
        let method = value.get("method").and_then(Value::as_str);
        let id = value.get("id").cloned();
        if matches!(method, Some("$/cancelRequest")) {
            if let Some(request_id) = value.get("params").and_then(|params| params.get("id")) {
                self.cancelled_requests.insert(request_key(request_id));
            }
            return Ok(LspResponse { messages: Vec::new(), exit: false });
        }
        if let Some(request_id) = id.as_ref() {
            if self.cancelled_requests.remove(&request_key(request_id)) {
                return Ok(LspResponse { messages: Vec::new(), exit: false });
            }
        }

        let messages = match method {
            Some("initialize") => {
                self.configure_workspace(value);
                id.map(initialize_response).into_iter().collect()
            }
            Some("shutdown") => id.map(empty_response).into_iter().collect(),
            Some("textDocument/didOpen") => self.did_open(value),
            Some("textDocument/didChange") => self.did_change(value),
            Some("textDocument/hover") => id
                .map(|request_id| self.hover_response(request_id, value))
                .into_iter()
                .collect(),
            Some("textDocument/definition") => id
                .map(|request_id| self.definition_response(request_id, value))
                .into_iter()
                .collect(),
            Some("textDocument/references") => id
                .map(|request_id| self.references_response(request_id, value))
                .into_iter()
                .collect(),
            Some("textDocument/documentSymbol") => id
                .map(|request_id| self.document_symbols_response(request_id, value))
                .into_iter()
                .collect(),
            Some("workspace/symbol") => id
                .map(|request_id| self.workspace_symbols_response(request_id, value))
                .into_iter()
                .collect(),
            Some("textDocument/completion") => id
                .map(|request_id| self.completion_response(request_id, value))
                .into_iter()
                .collect(),
            Some("textDocument/signatureHelp") => id
                .map(|request_id| self.signature_help_response(request_id, value))
                .into_iter()
                .collect(),
            Some("axiom/serverStatus") => id
                .map(|request_id| self.server_status_response(request_id))
                .into_iter()
                .collect(),
            Some("initialized") | Some("exit") => Vec::new(),
            Some(other) => id
                .map(|request_id| unsupported_method_response(request_id, other))
                .into_iter()
                .collect(),
            None => Vec::new(),
        };
        Ok(LspResponse { messages, exit: matches!(method, Some("exit")) })
    }

    fn configure_workspace(&mut self, message: &Value) {
        let params = message.get("params").unwrap_or(&Value::Null);
        if let Some(uri) = params.get("rootUri").and_then(Value::as_str) {
            self.workspace_roots.insert(path_for_uri(uri));
        }
        if let Some(folders) = params.get("workspaceFolders").and_then(Value::as_array) {
            for folder in folders {
                if let Some(uri) = folder.get("uri").and_then(Value::as_str) {
                    self.workspace_roots.insert(path_for_uri(uri));
                }
            }
        }
    }

    fn did_open(&mut self, message: &Value) -> Vec<Value> {
        let Some(document) = message.get("params").and_then(|params| params.get("textDocument")) else {
            return Vec::new();
        };
        let (Some(uri), Some(text)) = (
            document.get("uri").and_then(Value::as_str),
            document.get("text").and_then(Value::as_str),
        ) else {
            return Vec::new();
        };
        let version = document.get("version").and_then(Value::as_i64).unwrap_or(0);
        self.note_document_root(uri);
        if !self.documents.contains_key(uri) && self.documents.len() >= MAX_WORKSPACE_DOCUMENTS {
            return vec![log_message("workspace exceeds the 2,048-document LSP limit")];
        }
        if text.len() > MAX_DOCUMENT_BYTES || !self.fits_workspace_limit(uri, text) {
            return vec![log_message("document exceeds the bounded LSP memory limit")];
        }
        self.documents.insert(uri.to_owned(), LspDocument { version, text: text.to_owned() });
        self.publish_workspace_diagnostics()
    }

    fn did_change(&mut self, message: &Value) -> Vec<Value> {
        let Some(params) = message.get("params") else { return Vec::new() };
        let Some(document) = params.get("textDocument") else { return Vec::new() };
        let Some(uri) = document.get("uri").and_then(Value::as_str) else { return Vec::new() };
        let version = document.get("version").and_then(Value::as_i64).unwrap_or(-1);
        let Some(existing) = self.documents.get(uri).cloned() else {
            return vec![log_message("didChange received for a document that was not opened")];
        };
        if version <= existing.version {
            return vec![log_message("stale document version ignored")];
        }
        let Some(changes) = params.get("contentChanges").and_then(Value::as_array) else { return Vec::new() };
        let mut text = existing.text;
        for change in changes {
            let Some(replacement) = change.get("text").and_then(Value::as_str) else { continue };
            if let Some(range) = change.get("range") {
                let Some((start, end)) = range_offsets(&text, range) else {
                    return vec![log_message("invalid incremental change range ignored")];
                };
                text.replace_range(start..end, replacement);
            } else {
                text = replacement.to_owned();
            }
            if text.len() > MAX_DOCUMENT_BYTES || !self.fits_workspace_limit(uri, &text) {
                return vec![log_message("document exceeds the bounded LSP memory limit")];
            }
        }
        self.documents.insert(uri.to_owned(), LspDocument { version, text });
        self.publish_workspace_diagnostics()
    }

    fn note_document_root(&mut self, uri: &str) {
        let path = path_for_uri(uri);
        if let Some(root) = package_root_for_path(&path) {
            self.workspace_roots.insert(root);
        }
    }

    fn fits_workspace_limit(&self, uri: &str, candidate: &str) -> bool {
        let retained = self
            .documents
            .iter()
            .filter(|(stored_uri, _)| stored_uri.as_str() != uri)
            .map(|(stored_uri, document)| document_memory_bytes(stored_uri, &document.text))
            .sum::<usize>();
        retained.saturating_add(document_memory_bytes(uri, candidate)) <= MAX_WORKSPACE_BYTES
    }

    fn workspace_index(&mut self) -> WorkspaceIndex {
        let started = Instant::now();
        let mut documents = self
            .documents
            .iter()
            .map(|(uri, document)| (uri.clone(), document.text.clone()))
            .collect::<BTreeMap<_, _>>();
        let remaining_bytes = MAX_WORKSPACE_BYTES.saturating_sub(
            documents
                .iter()
                .map(|(uri, source)| document_memory_bytes(uri, source))
                .sum::<usize>(),
        );
        let remaining_documents = MAX_WORKSPACE_DOCUMENTS.saturating_sub(documents.len());
        for (uri, text) in workspace_files(&self.workspace_roots, remaining_bytes, remaining_documents) {
            documents.entry(uri).or_insert(text);
        }
        let mut symbols = Vec::new();
        for (uri, text) in &documents {
            match syntax::parse_program_with_recovery(text, &path_for_uri(uri)) {
                Ok(program) => symbols.extend(symbols_for_program(uri, text, &program)),
                Err(_) => symbols.extend(symbols_for_incomplete_source(uri, text)),
            }
        }
        symbols.sort_by(|left, right| {
            (&left.name, &left.uri, left.line, left.column).cmp(&(&right.name, &right.uri, right.line, right.column))
        });
        self.workspace_generation = self.workspace_generation.saturating_add(1);
        self.last_analysis_ms = started.elapsed().as_millis();
        self.analysis_latency_samples_ms.push_back(self.last_analysis_ms);
        if self.analysis_latency_samples_ms.len() > MAX_LATENCY_SAMPLES {
            self.analysis_latency_samples_ms.pop_front();
        }
        WorkspaceIndex { documents, symbols }
    }

    fn publish_workspace_diagnostics(&mut self) -> Vec<Value> {
        let index = self.workspace_index();
        index
            .documents
            .iter()
            .map(|(uri, source)| publish_diagnostics(uri, source))
            .collect()
    }

    fn hover_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let Some((uri, position)) = request_position(message) else { return empty_response(id) };
        let Some(name) = word_at(index.documents.get(&uri).map(String::as_str).unwrap_or_default(), &position) else { return empty_response(id) };
        let Some(symbol) = index.symbols.iter().find(|symbol| symbol.name == name) else { return empty_response(id) };
        json!({ "jsonrpc": "2.0", "id": id, "result": { "contents": { "kind": "markdown", "value": format!("```axiom\\n{} {}\\n```", symbol.kind, symbol.detail) } } })
    }

    fn definition_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let Some((uri, position)) = request_position(message) else { return empty_response(id) };
        let Some(name) = word_at(index.documents.get(&uri).map(String::as_str).unwrap_or_default(), &position) else { return empty_response(id) };
        let locations = index.symbols.iter().filter(|symbol| symbol.name == name).map(symbol_location).collect::<Vec<_>>();
        json!({ "jsonrpc": "2.0", "id": id, "result": locations })
    }

    fn references_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let Some((uri, position)) = request_position(message) else { return empty_response(id) };
        let Some(name) = word_at(index.documents.get(&uri).map(String::as_str).unwrap_or_default(), &position) else { return empty_response(id) };
        let include_declaration = message.get("params").and_then(|params| params.get("context")).and_then(|context| context.get("includeDeclaration")).and_then(Value::as_bool).unwrap_or(false);
        let definitions = index.symbols.iter().filter(|symbol| symbol.name == name).map(|symbol| (symbol.uri.clone(), symbol.line, symbol.column)).collect::<BTreeSet<_>>();
        let mut locations = Vec::new();
        for (document_uri, source) in &index.documents {
            for (line, column) in word_occurrences(source, &name) {
                if include_declaration || !definitions.contains(&(document_uri.clone(), line, column)) {
                    locations.push(location(document_uri, line, column, name.len()));
                }
            }
        }
        json!({ "jsonrpc": "2.0", "id": id, "result": locations })
    }

    fn document_symbols_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let uri = message.get("params").and_then(|params| params.get("textDocument")).and_then(|document| document.get("uri")).and_then(Value::as_str).unwrap_or_default();
        let symbols = index.symbols.iter().filter(|symbol| symbol.uri == uri).map(document_symbol).collect::<Vec<_>>();
        json!({ "jsonrpc": "2.0", "id": id, "result": symbols })
    }

    fn workspace_symbols_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let query = message.get("params").and_then(|params| params.get("query")).and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
        let symbols = index.symbols.iter().filter(|symbol| query.is_empty() || symbol.name.to_ascii_lowercase().contains(&query)).map(workspace_symbol).collect::<Vec<_>>();
        json!({ "jsonrpc": "2.0", "id": id, "result": symbols })
    }

    fn completion_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let prefix = request_position(message).and_then(|(uri, position)| index.documents.get(&uri).and_then(|source| word_prefix_at(source, &position))).unwrap_or_default();
        let mut items = index.symbols.iter().filter(|symbol| prefix.is_empty() || symbol.name.starts_with(&prefix)).map(|symbol| json!({ "label": symbol.name, "kind": lsp_symbol_kind(symbol.kind), "detail": format!("{} {}", symbol.kind, symbol.detail) })).collect::<Vec<_>>();
        items.dedup_by(|left, right| left["label"] == right["label"]);
        json!({ "jsonrpc": "2.0", "id": id, "result": { "isIncomplete": false, "items": items } })
    }

    fn signature_help_response(&mut self, id: Value, message: &Value) -> Value {
        let index = self.workspace_index();
        let name = request_position(message).and_then(|(uri, position)| index.documents.get(&uri).and_then(|source| word_at(source, &position))).unwrap_or_default();
        let signatures = index.symbols.iter().filter(|symbol| symbol.name == name && matches!(symbol.kind, "function" | "method")).map(|symbol| json!({ "label": symbol.detail, "parameters": [] })).collect::<Vec<_>>();
        json!({ "jsonrpc": "2.0", "id": id, "result": { "signatures": signatures, "activeSignature": 0, "activeParameter": 0 } })
    }

    fn server_status_response(&mut self, id: Value) -> Value {
        let index = self.workspace_index();
        let packages = self.workspace_roots.iter().filter_map(|root| package_graph_metadata(root).ok()).map(|graph| json!({ "manifest": graph.manifest, "packages": graph.packages.len() })).collect::<Vec<_>>();
        let capabilities = self.workspace_roots.iter().filter_map(|root| project_capabilities(root).ok()).flatten().map(|capability| capability.name).collect::<BTreeSet<_>>();
        let memory_bytes = index
            .documents
            .iter()
            .map(|(uri, source)| document_memory_bytes(uri, source))
            .sum::<usize>();
        json!({ "jsonrpc": "2.0", "id": id, "result": { "schema": "axiom.lsp.v1", "workspaceGeneration": self.workspace_generation, "documents": index.documents.len(), "symbols": index.symbols.len(), "memoryBytes": memory_bytes, "memoryLimitBytes": MAX_WORKSPACE_BYTES, "lastAnalysisMs": self.last_analysis_ms, "p95AnalysisMs": self.p95_analysis_ms(), "latencySamples": self.analysis_latency_samples_ms.len(), "packages": packages, "capabilities": capabilities, "cancellation": "json-rpc-cancel-request", "documentSync": "incremental" } })
    }

    fn p95_analysis_ms(&self) -> u128 {
        let mut samples = self.analysis_latency_samples_ms.iter().copied().collect::<Vec<_>>();
        samples.sort_unstable();
        let percentile_rank = samples.len().saturating_mul(95).saturating_add(99) / 100;
        samples.get(percentile_rank.saturating_sub(1)).copied().unwrap_or(0)
    }
}

pub fn serve_stdio<R, W>(mut input: R, mut output: W) -> Result<(), Diagnostic>
where
    R: BufRead,
    W: Write,
{
    let mut server = LspServer::default();
    while let Some(message) = read_message(&mut input)? {
        let response = server.handle_message(&message)?;
        for payload in response.messages {
            write_message(&mut output, &payload)?;
        }
        output
            .flush()
            .map_err(|err| Diagnostic::new("lsp", format!("failed to flush LSP output: {err}")))?;
        if response.exit {
            break;
        }
    }
    Ok(())
}

pub fn handle_message(payload: &str) -> Result<LspResponse, Diagnostic> {
    LspServer::default().handle_message(payload)
}

pub fn publish_diagnostics(uri: &str, source: &str) -> Value {
    let diagnostics = analyze_source(uri, source)
        .into_iter()
        .map(|diagnostic| lsp_diagnostic(source, diagnostic))
        .collect::<Vec<Value>>();
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics
        }
    })
}

fn initialize_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "serverInfo": { "name": "axiom-analyzer", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": TEXT_DOCUMENT_SYNC_KIND_INCREMENTAL },
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "completionProvider": { "triggerCharacters": [".", ":"] },
                "signatureHelpProvider": { "triggerCharacters": ["(", ","] }
            }
        }
    })
}

fn empty_response(id: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": null })
}

fn unsupported_method_response(id: Value, method: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unsupported method {method:?}") } })
}

fn log_message(message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "method": "window/logMessage", "params": { "type": 2, "message": message } })
}

fn request_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_default()
}

fn request_position(message: &Value) -> Option<(String, &Value)> {
    let params = message.get("params")?;
    let uri = params.get("textDocument")?.get("uri")?.as_str()?.to_owned();
    Some((uri, params.get("position")?))
}

fn package_root_for_path(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?.to_path_buf();
    loop {
        if current.join("axiom.toml").is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn workspace_files(
    roots: &BTreeSet<PathBuf>,
    byte_limit: usize,
    document_limit: usize,
) -> BTreeMap<String, String> {
    let mut documents = BTreeMap::new();
    let mut pending = roots.iter().cloned().collect::<Vec<_>>();
    let mut bytes = 0usize;
    while let Some(path) = pending.pop() {
        if documents.len() >= document_limit || bytes >= byte_limit {
            break;
        }
        let Ok(entries) = fs::read_dir(&path) else { continue };
        for entry in entries.flatten() {
            if documents.len() >= document_limit || bytes >= byte_limit {
                break;
            }
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if !matches!(entry.file_name().to_str(), Some("target" | ".git" | "dist")) {
                    pending.push(entry_path);
                }
                continue;
            }
            if entry_path.extension().and_then(|extension| extension.to_str()) != Some("ax") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&entry_path) else { continue };
            let uri = uri_for_path(&entry_path);
            let storage_bytes = document_memory_bytes(&uri, &text);
            if text.len() > MAX_DOCUMENT_BYTES || bytes.saturating_add(storage_bytes) > byte_limit {
                continue;
            }
            bytes += storage_bytes;
            documents.insert(uri, text);
        }
    }
    documents
}

fn document_memory_bytes(uri: &str, source: &str) -> usize {
    DOCUMENT_METADATA_BYTES
        .saturating_add(uri.len())
        .saturating_add(source.len())
}

fn uri_for_path(path: &Path) -> String {
    format!("file://{}", path.display().to_string().replace(' ', "%20"))
}

fn symbols_for_program(uri: &str, source: &str, program: &syntax::Program) -> Vec<LspSymbol> {
    let mut symbols = Vec::new();
    let mut push = |name: &str, kind: &'static str, line: usize, column: usize| {
        symbols.push(LspSymbol {
            name: name.to_owned(),
            kind,
            uri: uri.to_owned(),
            line,
            column: utf16_column_for_source_position(source, line, column),
            detail: source_line(source, line).to_owned(),
        });
    };
    for declaration in &program.macros { push(&declaration.name, "macro", declaration.line, declaration.column); }
    for declaration in &program.axioms { push(&declaration.name, "axiom", declaration.line, declaration.column); }
    for declaration in &program.semantic_capabilities { push(&declaration.name, "capability", declaration.line, declaration.column); }
    for declaration in &program.evidence { push(&declaration.name, "evidence", declaration.line, declaration.column); }
    for declaration in &program.consts { push(&declaration.name, "constant", declaration.line, declaration.column); }
    for declaration in &program.type_aliases { push(&declaration.name, "type", declaration.line, declaration.column); }
    for declaration in &program.structs {
        push(&declaration.name, "struct", declaration.line, declaration.column);
        for field in &declaration.fields { push(&field.name, "field", field.line, field.column); }
    }
    for declaration in &program.enums {
        push(&declaration.name, "enum", declaration.line, declaration.column);
        for variant in &declaration.variants { push(&variant.name, "variant", variant.line, variant.column); }
    }
    for declaration in &program.traits {
        push(&declaration.name, "trait", declaration.line, declaration.column);
        for method in &declaration.methods { push(&method.name, "method", method.line, method.column); }
    }
    for declaration in &program.functions { push(&declaration.name, if declaration.impl_target.is_some() { "method" } else { "function" }, declaration.line, declaration.column); }
    symbols
}

/// Editors need navigation while a document is temporarily unparsable. This
/// fallback is deliberately limited to top-level declaration headers; complete
/// documents always use the compiler parser/HIR path above.
fn symbols_for_incomplete_source(uri: &str, source: &str) -> Vec<LspSymbol> {
    let mut symbols = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let declaration = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        let kind = [
            ("fn ", "function"),
            ("struct ", "struct"),
            ("enum ", "enum"),
            ("trait ", "trait"),
            ("type ", "type"),
            ("const ", "constant"),
            ("macro ", "macro"),
        ]
        .into_iter()
        .find_map(|(prefix, kind)| declaration.strip_prefix(prefix).map(|rest| (rest, kind)));
        let Some((rest, kind)) = kind else { continue };
        let name = rest.chars().take_while(|character| character.is_ascii_alphanumeric() || *character == '_').collect::<String>();
        if name.is_empty() { continue }
        let scalar_column = line[..line.find(&name).unwrap_or_default()].chars().count() + 1;
        let column = utf16_column_for_line(line, scalar_column);
        symbols.push(LspSymbol { name, kind, uri: uri.to_owned(), line: line_index + 1, column, detail: line.trim().to_owned() });
    }
    symbols
}

fn source_line(source: &str, line: usize) -> &str {
    source.lines().nth(line.saturating_sub(1)).unwrap_or_default().trim()
}

fn symbol_location(symbol: &LspSymbol) -> Value {
    location(&symbol.uri, symbol.line, symbol.column, symbol.name.len())
}

fn location(uri: &str, line: usize, column: usize, width: usize) -> Value {
    json!({ "uri": uri, "range": { "start": { "line": line.saturating_sub(1), "character": column.saturating_sub(1) }, "end": { "line": line.saturating_sub(1), "character": column.saturating_sub(1).saturating_add(width) } } })
}

fn document_symbol(symbol: &LspSymbol) -> Value {
    json!({ "name": symbol.name, "kind": lsp_symbol_kind(symbol.kind), "detail": symbol.detail, "range": symbol_location(symbol)["range"], "selectionRange": symbol_location(symbol)["range"] })
}

fn workspace_symbol(symbol: &LspSymbol) -> Value {
    json!({ "name": symbol.name, "kind": lsp_symbol_kind(symbol.kind), "containerName": symbol.kind, "location": symbol_location(symbol) })
}

fn lsp_symbol_kind(kind: &str) -> u8 {
    match kind {
        "function" | "method" => 12,
        "struct" | "enum" | "trait" | "type" => 5,
        "field" => 8,
        "variant" => 22,
        "constant" => 14,
        _ => 13,
    }
}

fn position_offset(source: &str, position: &Value) -> Option<usize> {
    let line = position.get("line")?.as_u64()? as usize;
    let character = position.get("character")?.as_u64()? as usize;
    let mut offset = 0usize;
    for (index, text) in source.split_inclusive('\n').enumerate() {
        if index == line {
            let byte_offset = byte_offset_for_utf16_position(text, character)?;
            return Some(offset + byte_offset);
        }
        offset += text.len();
    }
    if line == source.lines().count() && character == 0 { Some(source.len()) } else { None }
}

fn byte_offset_for_utf16_position(line: &str, character: usize) -> Option<usize> {
    let line = line
        .strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line);
    let mut utf16_offset = 0usize;
    for (byte_offset, scalar) in line.char_indices() {
        if utf16_offset == character {
            return Some(byte_offset);
        }
        utf16_offset = utf16_offset.saturating_add(scalar.len_utf16());
        if utf16_offset > character {
            return None;
        }
    }
    (utf16_offset == character).then_some(line.len())
}

fn utf16_column_for_source_position(source: &str, line: usize, scalar_column: usize) -> usize {
    source
        .lines()
        .nth(line.saturating_sub(1))
        .map(|text| utf16_column_for_line(text, scalar_column))
        .unwrap_or(1)
}

fn utf16_column_for_line(line: &str, scalar_column: usize) -> usize {
    line.chars()
        .take(scalar_column.saturating_sub(1))
        .map(char::len_utf16)
        .sum::<usize>()
        .saturating_add(1)
}

fn range_offsets(source: &str, range: &Value) -> Option<(usize, usize)> {
    let start = position_offset(source, range.get("start")?)?;
    let end = position_offset(source, range.get("end")?)?;
    (start <= end).then_some((start, end))
}

fn word_at(source: &str, position: &Value) -> Option<String> {
    let offset = position_offset(source, position)?;
    let bytes = source.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_identifier_byte(bytes[start - 1]) { start -= 1; }
    let mut end = offset.min(bytes.len());
    while end < bytes.len() && is_identifier_byte(bytes[end]) { end += 1; }
    (start < end).then(|| source[start..end].to_owned())
}

fn word_prefix_at(source: &str, position: &Value) -> Option<String> {
    let offset = position_offset(source, position)?;
    let bytes = source.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_identifier_byte(bytes[start - 1]) { start -= 1; }
    Some(source[start..offset.min(bytes.len())].to_owned())
}

fn word_occurrences(source: &str, word: &str) -> Vec<(usize, usize)> {
    let mut occurrences = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let mut cursor = 0usize;
        while let Some(relative) = line[cursor..].find(word) {
            let start = cursor + relative;
            let end = start + word.len();
            if (start == 0 || !is_identifier_byte(line.as_bytes()[start - 1])) && (end == line.len() || !is_identifier_byte(line.as_bytes()[end])) {
                let scalar_column = line[..start].chars().count() + 1;
                occurrences.push((line_index + 1, utf16_column_for_line(line, scalar_column)));
            }
            cursor = end;
        }
    }
    occurrences
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}


fn read_message<R>(input: &mut R) -> Result<Option<String>, Diagnostic>
where
    R: BufRead,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = input
            .read_line(&mut line)
            .map_err(|err| Diagnostic::new("lsp", format!("failed to read LSP header: {err}")))?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if !name.trim().eq_ignore_ascii_case("Content-Length") {
                continue;
            }
            content_length = Some(value.trim().parse::<usize>().map_err(|err| {
                Diagnostic::new("lsp", format!("invalid Content-Length header: {err}"))
            })?);
        }
    }

    let length = content_length
        .ok_or_else(|| Diagnostic::new("lsp", "missing Content-Length header in LSP message"))?;
    let mut body = vec![0; length];
    input
        .read_exact(&mut body)
        .map_err(|err| Diagnostic::new("lsp", format!("failed to read LSP body: {err}")))?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| Diagnostic::new("lsp", format!("LSP body is not UTF-8: {err}")))
}

fn write_message<W>(output: &mut W, payload: &Value) -> Result<(), Diagnostic>
where
    W: Write,
{
    let body = serde_json::to_string(payload)
        .map_err(|err| Diagnostic::new("lsp", format!("failed to serialize LSP message: {err}")))?;
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .map_err(|err| Diagnostic::new("lsp", format!("failed to write LSP message: {err}")))
}

pub fn analyze_source(uri: &str, source: &str) -> Vec<Diagnostic> {
    let path = path_for_uri(uri);
    let capabilities = package_root_for_path(&path)
        .and_then(|root| load_manifest(&root).ok())
        .map(|manifest| manifest.capabilities)
        .unwrap_or_default();
    match syntax::parse_program_with_recovery(source, &path) {
        Ok(program) => {
            match hir::lower_with_capabilities_recovery(&program, &capabilities) {
                Ok(hir) => {
                    let _ = mir::lower(&hir);
                    Vec::new()
                }
                Err(diagnostics) => diagnostics_with_default_path(diagnostics, &path),
            }
        }
        Err(diagnostics) => diagnostics_with_default_path(diagnostics, &path),
    }
}

fn diagnostics_with_default_path(diagnostics: Vec<Diagnostic>, path: &Path) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic_with_default_path(diagnostic, path))
        .collect()
}

fn diagnostic_with_default_path(mut diagnostic: Diagnostic, path: &Path) -> Diagnostic {
    if diagnostic.path.is_none() {
        diagnostic.path = Some(path.display().to_string());
    }
    diagnostic
}

fn lsp_diagnostic(source: &str, diagnostic: Diagnostic) -> Value {
    let start_line = diagnostic.line.unwrap_or(1);
    let start_column = diagnostic.column.unwrap_or(1);
    let end_line = diagnostic.end_line.unwrap_or(start_line);
    let end_column = diagnostic
        .end_column
        .unwrap_or_else(|| start_column.saturating_add(1));
    let start_character = utf16_column_for_source_position(source, start_line, start_column)
        .saturating_sub(1);
    let end_character = utf16_column_for_source_position(source, end_line, end_column)
        .saturating_sub(1);
    json!({
        "range": {
            "start": {
                "line": start_line.saturating_sub(1),
                "character": start_character
            },
            "end": {
                "line": end_line.saturating_sub(1),
                "character": end_character
            }
        },
        "severity": 1,
        "source": "axiomc",
        "code": diagnostic.code.unwrap_or(diagnostic.kind),
        "message": diagnostic.message
    })
}

fn path_for_uri(uri: &str) -> PathBuf {
    if let Some(path) = uri.strip_prefix("file://") {
        return PathBuf::from(percent_decode(path));
    }
    PathBuf::from(uri)
}

fn percent_decode(input: &str) -> String {
    let mut output = Vec::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            let mut encoded = [0; 4];
            output.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            continue;
        }
        let hi = chars.next();
        let lo = chars.next();
        match (hi, lo) {
            (Some(hi), Some(lo)) => {
                let encoded = format!("{hi}{lo}");
                if let Ok(value) = u8::from_str_radix(&encoded, 16) {
                    output.push(value);
                } else {
                    output.push(b'%');
                    let mut hi_encoded = [0; 4];
                    output.extend_from_slice(hi.encode_utf8(&mut hi_encoded).as_bytes());
                    let mut lo_encoded = [0; 4];
                    output.extend_from_slice(lo.encode_utf8(&mut lo_encoded).as_bytes());
                }
            }
            (Some(hi), None) => {
                output.push(b'%');
                let mut encoded = [0; 4];
                output.extend_from_slice(hi.encode_utf8(&mut encoded).as_bytes());
            }
            _ => output.push(b'%'),
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(method: &str, params: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        })
        .to_string()
    }

    #[test]
    fn initialize_advertises_incremental_document_sync() {
        let response =
            handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
                .expect("handle initialize");

        assert!(!response.exit);
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0]["id"], json!(1));
        assert_eq!(
            response.messages[0]["result"]["serverInfo"]["name"],
            json!("axiom-analyzer")
        );
        assert_eq!(
            response.messages[0]["result"]["capabilities"]["textDocumentSync"]["change"],
            json!(2)
        );
    }

    #[test]
    fn did_open_publishes_compiler_diagnostic() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/bad.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "}\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        assert_eq!(response.messages.len(), 1);
        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["source"], json!("axiomc"));
        assert_eq!(diagnostics[0]["code"], json!("parse"));
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .expect("message")
                .contains("unexpected closing brace")
        );
        assert_eq!(
            diagnostics[0]["range"]["start"],
            json!({ "line": 0, "character": 0 })
        );
    }

    #[test]
    fn did_open_publishes_multiple_parse_diagnostics() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/multi-parse.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "import math.ax\nlet answer int = 42\nelse {\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics[0]["message"],
            json!("import must use a quoted relative path")
        );
        assert_eq!(
            diagnostics[1]["message"],
            json!("let binding is missing ':'")
        );
        assert_eq!(diagnostics[2]["message"], json!("unexpected else block"));
    }

    #[test]
    fn did_open_publishes_multiple_type_diagnostics() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/multi-type.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print missing_name\nlet answer: int = \"nope\"\nprint answer\nprint also_missing\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics[0]["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("undefined variable")
        );
        assert_eq!(
            diagnostics[1]["message"],
            json!("let binding \"answer\" expects int, got string")
        );
        assert_eq!(
            diagnostics[2]["range"]["start"],
            json!({ "line": 3, "character": 6 })
        );
        assert!(
            diagnostics[2]["message"]
                .as_str()
                .unwrap()
                .contains("undefined variable")
        );
    }

    #[test]
    fn lsp_diagnostic_uses_explicit_diagnostic_end_range() {
        let diagnostic = Diagnostic::new("ownership", "use of moved value")
            .with_code("use_after_move")
            .with_span_range(2, 7, 2, 15);

        let payload = lsp_diagnostic("first\nsecond diagnostic\n", diagnostic);

        assert_eq!(
            payload["range"],
            json!({
                "start": { "line": 1, "character": 6 },
                "end": { "line": 1, "character": 14 }
            })
        );
    }

    #[test]
    fn lsp_diagnostic_uses_utf16_characters_after_astral_unicode() {
        let diagnostic = Diagnostic::new("type", "undefined variable")
            .with_span_range(1, 2, 1, 7);

        let payload = lsp_diagnostic("😀value\n", diagnostic);

        assert_eq!(
            payload["range"],
            json!({
                "start": { "line": 0, "character": 2 },
                "end": { "line": 0, "character": 7 }
            })
        );
    }

    #[test]
    fn did_change_recomputes_and_clears_diagnostics() {
        let open = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print missing_name\\n"
                }
            }),
        );
        let payload = notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "version": 2
                },
                "contentChanges": [{
                    "text": "let answer: int = 42\nprint answer\n"
                }]
            }),
        );

        let mut server = LspServer::default();
        server.handle_message(&open).expect("handle didOpen");
        let response = server.handle_message(&payload).expect("handle didChange");

        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn did_change_applies_incremental_range_changes() {
        let open = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print 1\\n"
                }
            }),
        );
        let payload = notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "version": 3
                },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 0 }
                    },
                    "text": "}"
                }]
            }),
        );

        let mut server = LspServer::default();
        server.handle_message(&open).expect("handle didOpen");
        let response = server.handle_message(&payload).expect("handle didChange");

        assert_eq!(response.messages.len(), 1);
        assert_eq!(server.documents["file:///tmp/good.ax"].text, "}print 1\\n");
    }

    #[test]
    fn did_change_interprets_ranges_as_utf16_code_units() {
        let uri = "file:///tmp/utf16-change.ax";
        let open = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "axiom",
                    "version": 1,
                    "text": "😀beta\n"
                }
            }),
        );
        let change = notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 0, "character": 2 },
                        "end": { "line": 0, "character": 6 }
                    },
                    "text": "gamma"
                }]
            }),
        );

        let mut server = LspServer::default();
        server.handle_message(&open).expect("open document");
        server.handle_message(&change).expect("apply incremental change");

        assert_eq!(server.documents[uri].text, "😀gamma\n");
    }

    #[test]
    fn references_emit_utf16_positions_after_astral_unicode() {
        let uri = "file:///tmp/utf16-navigation.ax";
        let mut server = LspServer::default();
        server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "axiom",
                        "version": 1,
                        "text": "// 😀 health\npub fn health(): int {\nreturn 1\n}\n"
                    }
                }),
            ))
            .expect("open document");

        let response = server
            .handle_message(&request(
                1,
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 1, "character": 7 },
                    "context": { "includeDeclaration": true }
                }),
            ))
            .expect("find references");
        let locations = response.messages[0]["result"].as_array().expect("locations");

        assert!(locations.iter().any(|location| {
            location["uri"] == json!(uri)
                && location["range"]["start"] == json!({ "line": 0, "character": 6 })
                && location["range"]["end"] == json!({ "line": 0, "character": 12 })
        }));
    }

    #[test]
    fn did_open_exercises_hir_diagnostics_after_parse() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/type.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print missing_name\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["code"], json!("type"));
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .expect("message")
                .contains("undefined variable")
        );
    }

    #[test]
    fn stdio_loop_reads_and_writes_framed_messages() {
        let body = r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#;
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        serve_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""id":7"#));
        assert!(output.contains(r#""axiom-analyzer""#));
    }

    #[test]
    fn stdio_loop_accepts_case_insensitive_content_length_header() {
        let body = r#"{"jsonrpc":"2.0","id":8,"method":"initialize","params":{}}"#;
        let input = format!("content-length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        serve_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""id":8"#));
    }

    #[test]
    fn stdio_loop_preserves_workspace_state_between_requests() {
        let provider = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "file:///tmp/lsp-stdio/provider.ax", "languageId": "axiom", "version": 1, "text": "pub fn health(): int {\nreturn 1\n}\n" } }
        })
        .to_string();
        let consumer = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "file:///tmp/lsp-stdio/consumer.ax", "languageId": "axiom", "version": 1, "text": "print health()\n" } }
        })
        .to_string();
        let definition = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "file:///tmp/lsp-stdio/consumer.ax" }, "position": { "line": 0, "character": 8 } }
        })
        .to_string();
        let input = [provider, consumer, definition]
            .into_iter()
            .map(|body| format!("Content-Length: {}\r\n\r\n{body}", body.len()))
            .collect::<String>();
        let mut output = Vec::new();

        serve_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stateful stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains(r#""id":9"#));
        assert!(output.contains("file:///tmp/lsp-stdio/provider.ax"));
    }

    #[test]
    fn percent_decode_decodes_utf8_file_uri_bytes_once() {
        let path = path_for_uri("file:///tmp/%E6%96%87%E4%BB%B6.ax");

        assert_eq!(path, PathBuf::from("/tmp/文件.ax"));
    }

    #[test]
    fn exit_notification_stops_stdio_loop() {
        let response =
            handle_message(r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#).expect("exit");

        assert!(response.exit);
        assert!(response.messages.is_empty());
    }

    fn request(id: u64, method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    #[test]
    fn persistent_workspace_supports_navigation_completion_and_status() {
        let provider = "file:///tmp/lsp-workspace/provider.ax";
        let consumer = "file:///tmp/lsp-workspace/consumer.ax";
        let mut server = LspServer::default();
        server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": provider, "languageId": "axiom", "version": 1, "text": "pub fn health(): int {\\nreturn 1\\n}\\n" } }),
            ))
            .expect("open provider");
        server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": consumer, "languageId": "axiom", "version": 1, "text": "print health()\\n" } }),
            ))
            .expect("open consumer");

        let position = json!({ "line": 0, "character": 8 });
        let definition = server
            .handle_message(&request(1, "textDocument/definition", json!({ "textDocument": { "uri": consumer }, "position": position })))
            .expect("definition");
        assert_eq!(definition.messages[0]["result"][0]["uri"], json!(provider), "{:?}", definition.messages);

        let hover = server
            .handle_message(&request(2, "textDocument/hover", json!({ "textDocument": { "uri": consumer }, "position": { "line": 0, "character": 8 } })))
            .expect("hover");
        assert!(hover.messages[0]["result"]["contents"]["value"].as_str().unwrap().contains("health"));

        let completion = server
            .handle_message(&request(3, "textDocument/completion", json!({ "textDocument": { "uri": consumer }, "position": { "line": 0, "character": 10 } })))
            .expect("completion");
        assert!(completion.messages[0]["result"]["items"].as_array().unwrap().iter().any(|item| item["label"] == "health"));

        let status = server.handle_message(&request(4, "axiom/serverStatus", json!({}))).expect("status");
        assert_eq!(status.messages[0]["result"]["schema"], json!("axiom.lsp.v1"));
        assert_eq!(status.messages[0]["result"]["documents"], json!(2));
        assert_eq!(status.messages[0]["result"]["documentSync"], json!("incremental"));
        assert!(status.messages[0]["result"]["latencySamples"].as_u64().unwrap() >= 1);
        let schema: Value = serde_json::from_str(include_str!("../../../schemas/axiom.lsp.v1.schema.json")).expect("status schema");
        jsonschema::validator_for(&schema)
            .expect("compile status schema")
            .validate(&status.messages[0]["result"])
            .expect("status matches public schema");

        let transcript: Value = serde_json::from_str(include_str!("../../../compiler-contracts/snapshots/lsp-v1-transcript.json")).expect("LSP transcript fixture");
        assert_eq!(transcript["documents"].as_array().expect("documents").len(), 2);
        assert!(transcript["requests"].as_array().expect("requests").iter().any(|method| method == "axiom/serverStatus"));
    }

    #[test]
    fn stale_versions_and_cancelled_requests_do_not_publish_stale_results() {
        let uri = "file:///tmp/lsp-state/main.ax";
        let mut server = LspServer::default();
        server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": "axiom", "version": 2, "text": "pub fn main(): int { return 1 }" } }),
            ))
            .expect("open");
        let stale = server
            .handle_message(&notification(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "}" }] }),
            ))
            .expect("stale change");
        assert_eq!(stale.messages[0]["method"], json!("window/logMessage"));
        assert_eq!(server.documents[uri].version, 2);

        server
            .handle_message(&notification("$/cancelRequest", json!({ "id": 11 })))
            .expect("cancel");
        let cancelled = server
            .handle_message(&request(11, "workspace/symbol", json!({ "query": "main" })))
            .expect("cancelled request");
        assert!(cancelled.messages.is_empty());
    }

    #[test]
    fn document_updates_cannot_exceed_the_workspace_memory_bound() {
        let mut server = LspServer::default();
        server.documents.insert(
            String::from("file:///tmp/lsp-limit/existing.ax"),
            LspDocument { version: 1, text: "x".repeat(MAX_WORKSPACE_BYTES) },
        );
        let response = server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": "file:///tmp/lsp-limit/next.ax", "languageId": "axiom", "version": 1, "text": "pub fn next(): int { return 1 }" } }),
            ))
            .expect("bounded open");
        assert_eq!(response.messages[0]["method"], json!("window/logMessage"));
        assert_eq!(server.documents.len(), 1);
    }

    #[test]
    fn empty_editor_documents_cannot_exceed_the_workspace_document_bound() {
        let mut server = LspServer::default();
        for index in 0..MAX_WORKSPACE_DOCUMENTS {
            server.documents.insert(
                format!("file:///tmp/lsp-document-limit/{index}.ax"),
                LspDocument { version: 1, text: String::new() },
            );
        }
        let uri = "file:///tmp/lsp-document-limit/overflow.ax";
        let open = server
            .handle_message(&notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": "axiom", "version": 1, "text": "" } }),
            ))
            .expect("bounded empty open");
        assert_eq!(open.messages[0]["method"], json!("window/logMessage"));
        assert_eq!(server.documents.len(), MAX_WORKSPACE_DOCUMENTS);

        let change = server
            .handle_message(&notification(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "" }] }),
            ))
            .expect("unopened change");
        assert_eq!(change.messages[0]["method"], json!("window/logMessage"));
        assert_eq!(server.documents.len(), MAX_WORKSPACE_DOCUMENTS);
    }

    #[test]
    fn compiler_scale_workspace_reports_bounded_p95_latency_and_memory_evidence() {
        let transcript: Value = serde_json::from_str(include_str!("../../../compiler-contracts/snapshots/lsp-v1-transcript.json")).expect("LSP transcript fixture");
        let bounds = &transcript["bounds"];
        let document_count = bounds["compilerScaleWorkspaceDocuments"].as_u64().expect("document count") as usize;
        let maximum_p95 = bounds["p95IndexingMilliseconds"].as_u64().expect("p95 limit");
        let mut server = LspServer::default();
        for index in 0..document_count {
            server.documents.insert(
                format!("file:///tmp/lsp-scale/module-{index}.ax"),
                LspDocument { version: 1, text: format!("pub fn operation_{index}(): int {{\nreturn {index}\n}}\n") },
            );
        }
        let status = server.handle_message(&request(1, "axiom/serverStatus", json!({}))).expect("status");
        assert_eq!(status.messages[0]["result"]["documents"], json!(document_count));
        assert!(status.messages[0]["result"]["memoryBytes"].as_u64().unwrap() <= MAX_WORKSPACE_BYTES as u64);
        assert!(status.messages[0]["result"]["p95AnalysisMs"].as_u64().unwrap() <= maximum_p95);
    }
}
