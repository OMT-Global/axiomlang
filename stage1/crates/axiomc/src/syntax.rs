use crate::diagnostics::Diagnostic;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub path: String,
    pub imports: Vec<Import>,
    pub consts: Vec<ConstDecl>,
    pub type_aliases: Vec<TypeAliasDecl>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub functions: Vec<Function>,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ReceiverKind {
    Value,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Module,
    Package,
    Public,
}

impl Visibility {
    pub fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub source_name: String,
    pub path: String,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_ty: TypeName,
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_extern: bool,
    pub extern_abi: Option<String>,
    pub extern_library: Option<String>,
    pub visibility: Visibility,
    pub receiver: Option<ReceiverKind>,
    pub impl_target: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConstDecl {
    pub name: String,
    pub ty: TypeName,
    pub expr: Expr,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TypeAliasDecl {
    pub name: String,
    pub ty: TypeName,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<StructField>,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeName,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariantDecl>,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumVariantDecl {
    pub name: String,
    pub payload_tys: Vec<TypeName>,
    pub payload_names: Vec<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeName,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Stmt {
    Let {
        name: String,
        ty: TypeName,
        expr: Expr,
        line: usize,
        column: usize,
    },
    Print {
        expr: Expr,
        line: usize,
        column: usize,
    },
    Panic {
        expr: Expr,
        line: usize,
        column: usize,
    },
    Defer {
        expr: Expr,
        line: usize,
        column: usize,
    },
    If {
        cond: Expr,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
        line: usize,
        column: usize,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        line: usize,
        column: usize,
    },
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        line: usize,
        column: usize,
    },
    Return {
        expr: Expr,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchArm {
    pub variant: String,
    pub bindings: Vec<String>,
    pub is_named: bool,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum TypeName {
    Int,
    Bool,
    String,
    Named(String, Vec<TypeName>),
    Ptr(Box<TypeName>),
    MutPtr(Box<TypeName>),
    Slice(Box<TypeName>),
    MutSlice(Box<TypeName>),
    LifetimeSlice(String, Box<TypeName>),
    LifetimeMutSlice(String, Box<TypeName>),
    Option(Box<TypeName>),
    Result(Box<TypeName>, Box<TypeName>),
    Tuple(Vec<TypeName>),
    Map(Box<TypeName>, Box<TypeName>),
    Array(Box<TypeName>),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Expr {
    Literal(Literal),
    VarRef {
        name: String,
        line: usize,
        column: usize,
    },
    Call {
        name: String,
        type_args: Vec<TypeName>,
        args: Vec<Expr>,
        line: usize,
        column: usize,
    },
    MethodCall {
        base: Box<Expr>,
        method: String,
        type_args: Vec<TypeName>,
        args: Vec<Expr>,
        line: usize,
        column: usize,
    },
    BinaryAdd {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: usize,
        column: usize,
    },
    BinaryCompare {
        op: CompareOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: usize,
        column: usize,
    },
    Try {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    Await {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    StructLiteral {
        name: String,
        fields: Vec<StructFieldValue>,
        line: usize,
        column: usize,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
        line: usize,
        column: usize,
    },
    TupleLiteral {
        elements: Vec<Expr>,
        line: usize,
        column: usize,
    },
    TupleIndex {
        base: Box<Expr>,
        index: usize,
        line: usize,
        column: usize,
    },
    MapLiteral {
        entries: Vec<MapEntry>,
        line: usize,
        column: usize,
    },
    ArrayLiteral {
        elements: Vec<Expr>,
        line: usize,
        column: usize,
    },
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        line: usize,
        column: usize,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expr,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
struct MacroRule {
    name: String,
    params: Vec<String>,
    template: String,
}

pub fn parse_program(source: &str, path: &Path) -> Result<Program, Diagnostic> {
    parse_program_with_recovery(source, path).map_err(|mut diagnostics| {
        let mut first = diagnostics.remove(0);
        first.related = diagnostics;
        first
    })
}

pub fn parse_program_with_recovery(source: &str, path: &Path) -> Result<Program, Vec<Diagnostic>> {
    let source = expand_declarative_macros(source, path)?;
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    let mut imports = Vec::new();
    let mut consts = Vec::new();
    let mut type_aliases = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut stmts = Vec::new();
    let mut diagnostics = Vec::new();
    while index < lines.len() {
        let line_no = index + 1;
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        match trimmed {
            "}" => {
                diagnostics.push(
                    Diagnostic::new("parse", "unexpected closing brace")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                );
                index += 1;
                continue;
            }
            "} else {" | "else {" => {
                diagnostics.push(
                    Diagnostic::new("parse", "unexpected else block")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                );
                index += 1;
                continue;
            }
            _ => {}
        }
        match parse_import(trimmed, path, line_no) {
            Ok(Some(import)) => {
                imports.push(import);
                index += 1;
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                diagnostics.push(error);
                index += 1;
                continue;
            }
        }
        if trimmed.starts_with("pub import ")
            || trimmed.starts_with("pub(pkg) import ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("pub(pkg) use ")
            || trimmed.starts_with("export ")
        {
            diagnostics.push(
                Diagnostic::new(
                    "parse",
                    "stage1 bootstrap does not support re-exports; expose public symbols from their defining module instead",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
            );
            index += 1;
            continue;
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
            || trimmed.starts_with("pub(pkg) fn ")
            || trimmed.starts_with("pub(pkg) async fn ")
            || trimmed.starts_with("pub(pkg) extern fn ")
        {
            let start_index = index;
            match parse_function(&lines, &mut index, path) {
                Ok(function) => functions.push(function),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("const ")
            || trimmed.starts_with("pub const ")
            || trimmed.starts_with("pub(pkg) const ")
        {
            match parse_const_decl(trimmed, path, line_no) {
                Ok(const_decl) => consts.push(const_decl),
                Err(error) => diagnostics.push(error),
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("type ")
            || trimmed.starts_with("pub type ")
            || trimmed.starts_with("pub(pkg) type ")
        {
            match parse_type_alias(trimmed, path, line_no) {
                Ok(type_alias) => type_aliases.push(type_alias),
                Err(error) => diagnostics.push(error),
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub(pkg) struct ")
        {
            let start_index = index;
            match parse_struct(&lines, &mut index, path) {
                Ok(struct_decl) => structs.push(struct_decl),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("enum ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub(pkg) enum ")
        {
            let start_index = index;
            match parse_enum(&lines, &mut index, path) {
                Ok(enum_decl) => enums.push(enum_decl),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("impl ") {
            let start_index = index;
            match parse_impl(&lines, &mut index, path) {
                Ok(methods) => functions.extend(methods),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        let start_index = index;
        match parse_stmt(&lines, &mut index, path, false) {
            Ok(stmt) => stmts.push(stmt),
            Err(error) => {
                diagnostics.push(error);
                index = start_index;
                synchronize_top_level(&lines, &mut index);
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(Program {
        path: path.display().to_string(),
        imports,
        consts,
        type_aliases,
        structs,
        enums,
        functions,
        stmts,
    })
}

fn expand_declarative_macros(source: &str, path: &Path) -> Result<String, Vec<Diagnostic>> {
    let (macros, mut expanded) = collect_macro_rules(source, path)?;
    if macros.is_empty() {
        return Ok(expanded);
    }
    const MAX_MACRO_EXPANSION_DEPTH: usize = 32;
    for _ in 0..MAX_MACRO_EXPANSION_DEPTH {
        let (next, changed) = expand_macro_invocations_once(&expanded, &macros, path)?;
        expanded = next;
        if !changed {
            return Ok(expanded);
        }
    }
    Err(vec![Diagnostic::new(
        "parse",
        format!(
            "declarative macro expansion exceeded bounded depth of {MAX_MACRO_EXPANSION_DEPTH}; check for recursive macro invocation"
        ),
    )
    .with_path(path.display().to_string())
    .with_span(1, 1)])
}

fn collect_macro_rules(
    source: &str,
    path: &Path,
) -> Result<(std::collections::HashMap<String, MacroRule>, String), Vec<Diagnostic>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut macros = std::collections::HashMap::new();
    let mut kept = Vec::new();
    let mut index = 0usize;
    let mut top_level_depth = 0i32;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if !trimmed.starts_with("macro_rules! ") {
            kept.push(lines[index].to_string());
            top_level_depth += brace_delta(lines[index]);
            index += 1;
            continue;
        }
        let start_line = index + 1;
        if top_level_depth != 0 {
            return Err(vec![
                Diagnostic::new(
                    "parse",
                    "macro_rules! definitions are only supported at top level",
                )
                .with_path(path.display().to_string())
                .with_span(start_line, 1),
            ]);
        }
        let mut definition = String::new();
        let mut depth = 0i32;
        loop {
            let line = lines.get(index).ok_or_else(|| {
                vec![
                    Diagnostic::new("parse", "unterminated macro_rules! definition")
                        .with_path(path.display().to_string())
                        .with_span(start_line, 1),
                ]
            })?;
            if !definition.is_empty() {
                definition.push('\n');
            }
            definition.push_str(line);
            depth += brace_delta(line);
            index += 1;
            if depth == 0 && definition.contains('{') {
                break;
            }
        }
        let rule = parse_macro_rule(&definition, path, start_line)?;
        if macros.insert(rule.name.clone(), rule).is_some() {
            return Err(vec![
                Diagnostic::new("parse", "duplicate macro_rules! definition")
                    .with_path(path.display().to_string())
                    .with_span(start_line, 1),
            ]);
        }
    }
    Ok((macros, kept.join("\n")))
}

fn parse_macro_rule(
    definition: &str,
    path: &Path,
    line_no: usize,
) -> Result<MacroRule, Vec<Diagnostic>> {
    let trimmed = definition.trim();
    let Some(rest) = trimmed.strip_prefix("macro_rules! ") else {
        return Err(vec![
            Diagnostic::new("parse", "invalid macro_rules! definition")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    };
    let Some(open_brace) = find_top_level_char(rest, '{') else {
        return Err(vec![
            Diagnostic::new("parse", "macro_rules! definition is missing '{'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    };
    let name = rest[..open_brace].trim();
    validate_ident(name, path, line_no, 14).map_err(|error| vec![error])?;
    let Some(close_brace) = find_matching_brace(rest, open_brace) else {
        return Err(vec![
            Diagnostic::new("parse", "macro_rules! definition is missing closing '}'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    };
    if !rest[close_brace + 1..].trim().is_empty() {
        return Err(vec![
            Diagnostic::new("parse", "unexpected tokens after macro_rules! definition")
                .with_path(path.display().to_string())
                .with_span(line_no, close_brace + 1),
        ]);
    }
    let body = rest[open_brace + 1..close_brace].trim();
    let Some(arrow) = body.find("=>") else {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "macro_rules! definition must contain a single `=>` arm",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    };
    let pattern = body[..arrow].trim().trim_end_matches(';').trim();
    let template = body[arrow + 2..].trim().trim_end_matches(';').trim();
    if !pattern.starts_with('(')
        || !pattern.ends_with(')')
        || find_matching_paren(pattern, 0) != Some(pattern.len() - 1)
    {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "macro_rules! pattern must use `($name:fragment, ...)` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    }
    if !template.starts_with('{')
        || !template.ends_with('}')
        || find_matching_brace(template, 0) != Some(template.len() - 1)
    {
        return Err(vec![
            Diagnostic::new("parse", "macro_rules! expansion must be enclosed in braces")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    }
    let mut params = Vec::new();
    let params_raw = pattern[1..pattern.len() - 1].trim();
    if !params_raw.is_empty() {
        for part in split_top_level(params_raw, ',') {
            let part = part.trim();
            let Some(part) = part.strip_prefix('$') else {
                return Err(vec![
                    Diagnostic::new("parse", "macro parameter must start with `$`")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                ]);
            };
            let name = part
                .split_once(':')
                .map(|(name, _)| name)
                .unwrap_or(part)
                .trim();
            validate_ident(name, path, line_no, 1).map_err(|error| vec![error])?;
            if params.iter().any(|existing| existing == name) {
                return Err(vec![
                    Diagnostic::new("parse", format!("duplicate macro parameter {name:?}"))
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                ]);
            }
            params.push(name.to_string());
        }
    }
    Ok(MacroRule {
        name: name.to_string(),
        params,
        template: template[1..template.len() - 1]
            .trim_matches('\n')
            .to_string(),
    })
}

fn expand_macro_invocations_once(
    source: &str,
    macros: &std::collections::HashMap<String, MacroRule>,
    path: &Path,
) -> Result<(String, bool), Vec<Diagnostic>> {
    let mut changed = false;
    let mut output = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let (expanded, line_changed) = expand_macro_line_once(line, macros, path, line_index + 1)?;
        changed |= line_changed;
        output.extend(expanded);
    }
    Ok((output.join("\n"), changed))
}

fn expand_macro_line_once(
    line: &str,
    macros: &std::collections::HashMap<String, MacroRule>,
    path: &Path,
    line_no: usize,
) -> Result<(Vec<String>, bool), Vec<Diagnostic>> {
    let mut first_match: Option<(usize, usize, &MacroRule)> = None;
    for rule in macros.values() {
        let needle = format!("{}!(", rule.name);
        if let Some(start) = find_macro_invocation(line, &needle) {
            let open = start + rule.name.len() + 1;
            if let Some(close) = find_matching_paren(line, open) {
                if first_match.is_none_or(|(existing, _, _)| start < existing) {
                    first_match = Some((start, close, rule));
                }
            } else {
                return Err(vec![
                    Diagnostic::new("parse", "macro invocation is missing closing ')'")
                        .with_path(path.display().to_string())
                        .with_span(line_no, start + 1),
                ]);
            }
        }
    }
    let Some((start, close, rule)) = first_match else {
        return Ok((vec![line.to_string()], false));
    };
    let args_raw = &line[start + rule.name.len() + 2..close];
    let args: Vec<&str> = if args_raw.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level(args_raw, ',')
            .into_iter()
            .map(str::trim)
            .collect()
    };
    if args.len() != rule.params.len() {
        return Err(vec![
            Diagnostic::new(
                "parse",
                format!(
                    "macro {}! expects {} argument(s), got {}",
                    rule.name,
                    rule.params.len(),
                    args.len()
                ),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, start + 1),
        ]);
    }
    let expansion = render_macro_expansion(&rule.template, &rule.params, &args);
    let before = &line[..start];
    let after = &line[close + 1..];
    let invocation_is_statement = before.trim().is_empty() && after.trim().is_empty();
    if invocation_is_statement {
        let indent = before;
        let lines = expansion
            .lines()
            .map(|expanded_line| {
                if expanded_line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{indent}{}", expanded_line.trim())
                }
            })
            .collect();
        return Ok((lines, true));
    }
    if expansion.lines().count() > 1 {
        return Err(vec![
            Diagnostic::new(
                "parse",
                format!(
                    "multi-line macro {}! can only be invoked as a full statement",
                    rule.name
                ),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, start + 1),
        ]);
    }
    Ok((
        vec![format!("{}{}{}", before, expansion.trim(), after)],
        true,
    ))
}

fn find_macro_invocation(line: &str, needle: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => return None,
            _ => {
                if line[index..].starts_with(needle)
                    && line[..index]
                        .chars()
                        .next_back()
                        .is_none_or(|previous| !is_identifier_char(previous))
                {
                    return Some(index);
                }
            }
        }
    }
    None
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn render_macro_expansion(template: &str, params: &[String], args: &[&str]) -> String {
    let mut output = String::new();
    let mut chars = template.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some((index, ch)) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
                continue;
            }
            '#' => {
                output.push_str(&template[index..]);
                break;
            }
            '$' => {}
            _ => {
                output.push(ch);
                continue;
            }
        }
        let name_start = index + ch.len_utf8();
        let Some((_, first)) = chars.peek().copied() else {
            output.push(ch);
            continue;
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            output.push(ch);
            continue;
        }
        let mut name_end = name_start;
        while let Some((next_index, next_ch)) = chars.peek().copied() {
            if is_identifier_char(next_ch) {
                name_end = next_index + next_ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let name = &template[name_start..name_end];
        if let Some(position) = params.iter().position(|param| param == name) {
            output.push_str(args[position]);
        } else {
            output.push('$');
            output.push_str(name);
        }
    }
    output
}

fn synchronize_top_level(lines: &[&str], index: &mut usize) {
    if *index >= lines.len() {
        return;
    }
    let mut depth = brace_delta(lines[*index]);
    *index += 1;
    while *index < lines.len() && depth > 0 {
        depth += brace_delta(lines[*index]);
        *index += 1;
    }
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => break,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn parse_import(trimmed: &str, path: &Path, line_no: usize) -> Result<Option<Import>, Diagnostic> {
    let Some(rest) = trimmed.strip_prefix("import ") else {
        return Ok(None);
    };
    if let Some((import_path, alias)) = rest.split_once(" as ")
        && serde_json::from_str::<String>(import_path.trim()).is_ok()
    {
        let message = if alias.trim().is_empty() {
            "stage1 bootstrap does not support import aliases"
        } else {
            "stage1 bootstrap does not support import aliases; import exported symbols directly"
        };
        let column = trimmed.find(" as ").map(|index| index + 2).unwrap_or(1);
        return Err(Diagnostic::new("parse", message)
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }
    let import_path = serde_json::from_str::<String>(rest).map_err(|_| {
        Diagnostic::new("parse", "import must use a quoted relative path")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    Ok(Some(Import {
        path: import_path,
        line: line_no,
        column: 1,
    }))
}

fn parse_stmt_list(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<Stmt>, Diagnostic> {
    let mut stmts = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(stmts);
        }
        if trimmed == "} else {" {
            return Ok(stmts);
        }
        if trimmed == "else {" {
            return Err(Diagnostic::new("parse", "unexpected else block")
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
        }
        if trimmed.starts_with("import ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports imports at the top level",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("pub import ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("export ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap does not support re-exports inside blocks",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level function declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("const ") || trimmed.starts_with("pub const ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level const declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("type ") || trimmed.starts_with("pub type ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level type alias declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level struct declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level enum declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        stmts.push(parse_stmt(lines, index, path, true)?);
    }
    Err(Diagnostic::new("parse", "missing closing brace for block")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_stmt(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
    in_block: bool,
) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    if trimmed.starts_with("for ") {
        return Err(Diagnostic::new(
            "parse",
            "stage1 bootstrap does not support `for` loops yet; use `while`-based iteration until the iteration protocol lands",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1));
    }
    if trimmed.starts_with("if ") {
        return parse_if_stmt(lines, index, path);
    }
    if trimmed.starts_with("while ") {
        return parse_while_stmt(lines, index, path);
    }
    if trimmed.starts_with("match ") {
        return parse_match_stmt(lines, index, path);
    }
    if let Some(rest) = trimmed.strip_prefix("let ") {
        let stmt = parse_let_stmt(rest, path, line_no)?;
        *index += 1;
        return Ok(stmt);
    }
    if let Some(rest) = trimmed.strip_prefix("print ") {
        let expr = parse_expr(rest, path, line_no, 7)?;
        *index += 1;
        return Ok(Stmt::Print {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if trimmed == "panic"
        || trimmed.starts_with("panic(")
        || trimmed.starts_with("panic<")
        || trimmed
            .strip_prefix("panic")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    {
        let expr = parse_expr(trimmed, path, line_no, 1)?;
        *index += 1;
        return Ok(Stmt::Panic {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("defer ") {
        let expr = parse_expr(rest, path, line_no, 7)?;
        *index += 1;
        return Ok(Stmt::Defer {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("return ") {
        let expr = parse_expr(rest, path, line_no, 8)?;
        *index += 1;
        return Ok(Stmt::Return {
            expr,
            line: line_no,
            column: 1,
        });
    }
    let message = if in_block {
        "stage1 bootstrap currently supports let, print, panic, defer, if/else, while, match, and return statements inside blocks"
    } else {
        "stage1 bootstrap currently supports top-level import, const, type, struct, enum, fn, let, print, panic, defer, if/else, while, and match statements"
    };
    Err(Diagnostic::new("parse", message)
        .with_path(path.display().to_string())
        .with_span(line_no, 1))
}

fn parse_function(lines: &[&str], index: &mut usize, path: &Path) -> Result<Function, Diagnostic> {
    parse_function_in_context(lines, index, path, None)
}

fn parse_function_in_context(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
    impl_target: Option<&str>,
) -> Result<Function, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let (is_async, is_extern, header, fn_column) =
        if let Some(rest) = rest.strip_prefix("async fn ") {
            (true, false, rest, visibility_column + 6)
        } else if let Some(rest) = rest.strip_prefix("extern fn ") {
            (false, true, rest, visibility_column + 7)
        } else {
            let rest = rest.strip_prefix("fn ").ok_or_else(|| {
                Diagnostic::new("parse", "invalid function declaration")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1)
            })?;
            (false, false, rest, visibility_column)
        };
    let open_paren = find_top_level_char(header, '(').ok_or_else(|| {
        Diagnostic::new("parse", "function declaration is missing '('")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let close_paren = find_matching_paren(header, open_paren).ok_or_else(|| {
        Diagnostic::new("parse", "function declaration is missing ')'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name_text = header[..open_paren].trim();
    let (name, type_params) = parse_function_name(name_text, path, line_no, fn_column + 3)?;
    let (receiver, params) = parse_params(
        &header[open_paren + 1..close_paren],
        path,
        line_no,
        impl_target.is_some(),
    )?;
    let after_paren = header[close_paren + 1..].trim();
    let after_colon = after_paren.strip_prefix(':').ok_or_else(|| {
        Diagnostic::new("parse", "function declaration must include a return type")
            .with_path(path.display().to_string())
            .with_span(line_no, close_paren + 2)
    })?;
    if is_extern {
        let (return_text, extern_library) = after_colon.rsplit_once(" from ").ok_or_else(|| {
            Diagnostic::new(
                "parse",
                "extern function declaration must use `extern fn name(args): type from \"lib\"` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
        })?;
        let return_ty = parse_type_name(return_text.trim(), path, line_no, 1)?;
        let extern_library =
            serde_json::from_str::<String>(extern_library.trim()).map_err(|_| {
                Diagnostic::new("parse", "extern function library must be a quoted string")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1)
            })?;
        *index += 1;
        return Ok(Function {
            name: name.to_string(),
            source_name: name.to_string(),
            path: path.display().to_string(),
            type_params,
            params,
            return_ty,
            body: Vec::new(),
            is_async,
            is_extern,
            extern_abi: Some(String::from("C")),
            extern_library: Some(extern_library),
            visibility,
            receiver,
            impl_target: impl_target.map(str::to_string),
            line: line_no,
            column: 1,
        });
    }
    let return_text = after_colon
        .strip_suffix('{')
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new(
                "parse",
                "function declaration must use `fn name(args): type {` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
        })?;
    let return_ty = parse_type_name(return_text, path, line_no, 1)?;
    *index += 1;
    let body = parse_stmt_list(lines, index, path)?;
    Ok(Function {
        name: name.to_string(),
        source_name: name.to_string(),
        path: path.display().to_string(),
        type_params,
        params,
        return_ty,
        body,
        is_async,
        is_extern,
        extern_abi: None,
        extern_library: None,
        visibility,
        receiver,
        impl_target: impl_target.map(str::to_string),
        line: line_no,
        column: 1,
    })
}

fn parse_type_alias(
    trimmed: &str,
    path: &Path,
    line_no: usize,
) -> Result<TypeAliasDecl, Diagnostic> {
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("type ") {
        rest
    } else {
        let _ = rest.strip_prefix("type ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid type alias declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let equals = find_top_level_char(header, '=').ok_or_else(|| {
        Diagnostic::new("parse", "type alias declaration is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name = header[..equals].trim();
    validate_ident(name, path, line_no, visibility_column + 5)?;
    let target = header[equals + 1..].trim();
    if target.is_empty() {
        return Err(
            Diagnostic::new("parse", "type alias is missing a target type")
                .with_path(path.display().to_string())
                .with_span(line_no, equals + 2),
        );
    }
    let ty = parse_type_name(target, path, line_no, equals + 2)?;
    Ok(TypeAliasDecl {
        name: name.to_string(),
        ty,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_const_decl(trimmed: &str, path: &Path, line_no: usize) -> Result<ConstDecl, Diagnostic> {
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("const ") {
        rest
    } else {
        let _ = rest.strip_prefix("const ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid const declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let column = visibility_column + 6;
    let colon = find_top_level_char(header, ':').ok_or_else(|| {
        Diagnostic::new("parse", "const declaration is missing ':'")
            .with_path(path.display().to_string())
            .with_span(line_no, column)
    })?;
    let equals = find_top_level_char(header, '=').ok_or_else(|| {
        Diagnostic::new("parse", "const declaration is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, column)
    })?;
    if equals <= colon {
        return Err(Diagnostic::new(
            "parse",
            "const declaration must use `const NAME: Type = expr` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column));
    }
    let name = header[..colon].trim();
    validate_ident(name, path, line_no, column)?;
    let ty_text = header[colon + 1..equals].trim();
    if ty_text.is_empty() {
        return Err(
            Diagnostic::new("parse", "const declaration is missing a type")
                .with_path(path.display().to_string())
                .with_span(line_no, column + colon + 1),
        );
    }
    let expr_text = header[equals + 1..].trim();
    if expr_text.is_empty() {
        return Err(
            Diagnostic::new("parse", "const declaration is missing an initializer")
                .with_path(path.display().to_string())
                .with_span(line_no, column + equals + 1),
        );
    }
    Ok(ConstDecl {
        name: name.to_string(),
        ty: parse_type_name(ty_text, path, line_no, column + colon + 2)?,
        expr: parse_expr(expr_text, path, line_no, column + equals + 2)?,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_struct(lines: &[&str], index: &mut usize, path: &Path) -> Result<StructDecl, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("struct ") {
        rest
    } else {
        let _ = rest.strip_prefix("struct ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid struct declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let name_text = header.strip_suffix('{').map(str::trim).ok_or_else(|| {
        Diagnostic::new(
            "parse",
            "struct declaration must use `struct Name {` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1)
    })?;
    let (name, type_params) =
        parse_decl_name(name_text, "struct", path, line_no, visibility_column + 7)?;
    *index += 1;
    let fields = parse_struct_fields(lines, index, path)?;
    Ok(StructDecl {
        name: name.to_string(),
        type_params,
        fields,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_struct_fields(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<StructField>, Diagnostic> {
    let mut fields = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(fields);
        }
        let colon = find_top_level_char(trimmed, ':').ok_or_else(|| {
            Diagnostic::new("parse", "struct field is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let name = trimmed[..colon].trim();
        validate_ident(name, path, line_no, 1)?;
        let ty = parse_type_name(trimmed[colon + 1..].trim(), path, line_no, colon + 2)?;
        fields.push(StructField {
            name: name.to_string(),
            ty,
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(Diagnostic::new("parse", "missing closing brace for struct")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_impl(lines: &[&str], index: &mut usize, path: &Path) -> Result<Vec<Function>, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let target = trimmed
        .strip_prefix("impl ")
        .and_then(|rest| rest.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "impl block must use `impl Type {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    validate_ident(target, path, line_no, 6)?;
    *index += 1;
    let mut methods = Vec::new();
    while *index < lines.len() {
        skip_blank_lines(lines, index);
        if *index >= lines.len() {
            break;
        }
        let trimmed = lines[*index].trim();
        if trimmed == "}" {
            *index += 1;
            return Ok(methods);
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
            || trimmed.starts_with("pub(pkg) fn ")
            || trimmed.starts_with("pub(pkg) async fn ")
            || trimmed.starts_with("pub(pkg) extern fn ")
        {
            methods.push(parse_function_in_context(lines, index, path, Some(target))?);
            continue;
        }
        return Err(Diagnostic::new(
            "parse",
            "impl blocks may only contain function declarations",
        )
        .with_path(path.display().to_string())
        .with_span(*index + 1, 1));
    }
    Err(
        Diagnostic::new("parse", "impl block is missing closing '}'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
    )
}

fn parse_enum(lines: &[&str], index: &mut usize, path: &Path) -> Result<EnumDecl, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("enum ") {
        rest
    } else {
        let _ = rest.strip_prefix("enum ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid enum declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let name_text = header.strip_suffix('{').map(str::trim).ok_or_else(|| {
        Diagnostic::new("parse", "enum declaration must use `enum Name {` syntax")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let (name, type_params) =
        parse_decl_name(name_text, "enum", path, line_no, visibility_column + 5)?;
    *index += 1;
    let variants = parse_enum_variants(lines, index, path)?;
    Ok(EnumDecl {
        name: name.to_string(),
        type_params,
        variants,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_visibility_prefix(trimmed: &str) -> (Visibility, &str, usize) {
    if let Some(rest) = trimmed.strip_prefix("pub(pkg) ") {
        (Visibility::Package, rest, 10)
    } else if let Some(rest) = trimmed.strip_prefix("pub ") {
        (Visibility::Public, rest, 5)
    } else {
        (Visibility::Module, trimmed, 1)
    }
}

fn parse_enum_variants(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<EnumVariantDecl>, Diagnostic> {
    let mut variants = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(variants);
        }
        let (name, payload_tys, payload_names) = if trimmed.ends_with('}')
            && let Some(open_brace) = find_top_level_char(trimmed, '{')
            && matches!(find_matching_brace(trimmed, open_brace), Some(close) if close == trimmed.len() - 1)
        {
            let name = trimmed[..open_brace].trim();
            validate_ident(name, path, line_no, 1)?;
            let payload_raw = trimmed[open_brace + 1..trimmed.len() - 1].trim();
            let fields = parse_named_enum_payload_fields(
                payload_raw,
                path,
                line_no,
                open_brace + 2,
                "enum variant payload field",
            )?;
            (
                name.to_string(),
                fields.iter().map(|field| field.1.clone()).collect(),
                fields.into_iter().map(|field| field.0).collect(),
            )
        } else if trimmed.ends_with(')')
            && let Some(open_paren) = find_top_level_char(trimmed, '(')
            && matches!(find_matching_paren(trimmed, open_paren), Some(close) if close == trimmed.len() - 1)
        {
            let name = trimmed[..open_paren].trim();
            validate_ident(name, path, line_no, 1)?;
            let payload_raw = trimmed[open_paren + 1..trimmed.len() - 1].trim();
            if payload_raw.is_empty() {
                return Err(
                    Diagnostic::new("parse", "enum variant payload type is empty")
                        .with_path(path.display().to_string())
                        .with_span(line_no, open_paren + 2),
                );
            }
            let payload_tys = split_top_level_type(payload_raw, ',')
                .into_iter()
                .map(|ty| {
                    let ty = ty.trim();
                    if ty.is_empty() {
                        return Err(
                            Diagnostic::new("parse", "enum variant payload type is empty")
                                .with_path(path.display().to_string())
                                .with_span(line_no, open_paren + 2),
                        );
                    }
                    parse_type_name(ty, path, line_no, open_paren + 2)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), payload_tys, Vec::new())
        } else {
            validate_ident(trimmed, path, line_no, 1)?;
            (trimmed.to_string(), Vec::new(), Vec::new())
        };
        variants.push(EnumVariantDecl {
            name,
            payload_tys,
            payload_names,
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(Diagnostic::new("parse", "missing closing brace for enum")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_params(
    raw: &str,
    path: &Path,
    line_no: usize,
    allow_self: bool,
) -> Result<(Option<ReceiverKind>, Vec<Param>), Diagnostic> {
    if raw.trim().is_empty() {
        return Ok((None, Vec::new()));
    }
    let mut receiver = None;
    let mut params = Vec::new();
    for (index, param_text) in split_top_level_type(raw, ',').into_iter().enumerate() {
        let param_text = param_text.trim();
        if param_text == "self" {
            if !allow_self {
                return Err(Diagnostic::new(
                    "parse",
                    "self parameter is only allowed inside impl methods",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
            }
            if index != 0 {
                return Err(Diagnostic::new(
                    "parse",
                    "self parameter must be the first parameter in an impl method",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
            }
            receiver = Some(ReceiverKind::Value);
            continue;
        }
        let colon = find_top_level_char(param_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "function parameter is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let name = param_text[..colon].trim();
        validate_ident(name, path, line_no, 1)?;
        let ty = parse_type_name(param_text[colon + 1..].trim(), path, line_no, 1)?;
        params.push(Param {
            name: name.to_string(),
            ty,
            line: line_no,
            column: 1,
        });
    }
    Ok((receiver, params))
}

fn parse_if_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let cond_raw = trimmed
        .strip_prefix("if ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "if statement must use `if <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    let cond = parse_expr(cond_raw, path, line_no, 4)?;
    *index += 1;
    let then_block = parse_stmt_list(lines, index, path)?;
    skip_blank_lines(lines, index);
    let else_block = if *index < lines.len() {
        match lines[*index].trim() {
            "} else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            "else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(Stmt::If {
        cond,
        then_block,
        else_block,
        line: line_no,
        column: 1,
    })
}

fn parse_while_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let cond_raw = trimmed
        .strip_prefix("while ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "while statement must use `while <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    let cond = parse_expr(cond_raw, path, line_no, 7)?;
    *index += 1;
    let body = parse_stmt_list(lines, index, path)?;
    Ok(Stmt::While {
        cond,
        body,
        line: line_no,
        column: 1,
    })
}

fn parse_match_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let expr_raw = trimmed
        .strip_prefix("match ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "match statement must use `match <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    let expr = parse_expr(expr_raw, path, line_no, 7)?;
    *index += 1;
    let arms = parse_match_arms(lines, index, path)?;
    Ok(Stmt::Match {
        expr,
        arms,
        line: line_no,
        column: 1,
    })
}

fn parse_match_arms(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<MatchArm>, Diagnostic> {
    let mut arms = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(arms);
        }
        let variant = trimmed.strip_suffix('{').map(str::trim).ok_or_else(|| {
            Diagnostic::new("parse", "match arm must use `Variant {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        if let Some(column) = find_top_level_keyword(variant, "if") {
            return Err(
                Diagnostic::new("parse", "match arm guards are not supported yet")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + 1),
            );
        }
        if variant.starts_with('(') {
            return Err(
                Diagnostic::new("parse", "nested match patterns are not supported yet")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            );
        }
        let (variant, bindings, is_named) = if variant.ends_with('}')
            && let Some(open_brace) = find_top_level_char(variant, '{')
            && matches!(find_matching_brace(variant, open_brace), Some(close) if close == variant.len() - 1)
        {
            let name = variant[..open_brace].trim();
            validate_ident(name, path, line_no, 1)?;
            let bindings_raw = &variant[open_brace + 1..variant.len() - 1];
            if bindings_raw.trim().is_empty() {
                return Err(Diagnostic::new("parse", "match arm binding is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, open_brace + 2));
            }
            let bindings = split_top_level_with_offsets(bindings_raw, ',')
                .into_iter()
                .map(|(binding_offset, raw_binding)| {
                    let binding = raw_binding.trim();
                    let leading_ws = raw_binding
                        .len()
                        .saturating_sub(raw_binding.trim_start().len());
                    let binding_column = open_brace + 2 + binding_offset + leading_ws;
                    if binding.is_empty() {
                        return Err(Diagnostic::new("parse", "match arm binding is empty")
                            .with_path(path.display().to_string())
                            .with_span(line_no, binding_column));
                    }
                    if let Some(nested_offset) = find_nested_match_pattern_offset(binding) {
                        return Err(Diagnostic::new(
                            "parse",
                            "nested match patterns are not supported yet",
                        )
                        .with_path(path.display().to_string())
                        .with_span(line_no, binding_column + nested_offset));
                    }
                    validate_ident(binding, path, line_no, binding_column)?;
                    Ok(binding.to_string())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), bindings, true)
        } else if variant.ends_with(')')
            && let Some(open_paren) = find_top_level_char(variant, '(')
            && matches!(find_matching_paren(variant, open_paren), Some(close) if close == variant.len() - 1)
        {
            let name = variant[..open_paren].trim();
            validate_ident(name, path, line_no, 1)?;
            let bindings_raw = &variant[open_paren + 1..variant.len() - 1];
            if bindings_raw.trim().is_empty() {
                return Err(Diagnostic::new("parse", "match arm binding is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, open_paren + 2));
            }
            let bindings = split_top_level_with_offsets(bindings_raw, ',')
                .into_iter()
                .map(|(binding_offset, raw_binding)| {
                    let binding = raw_binding.trim();
                    let leading_ws = raw_binding
                        .len()
                        .saturating_sub(raw_binding.trim_start().len());
                    let binding_column = open_paren + 2 + binding_offset + leading_ws;
                    if binding.is_empty() {
                        return Err(Diagnostic::new("parse", "match arm binding is empty")
                            .with_path(path.display().to_string())
                            .with_span(line_no, binding_column));
                    }
                    if let Some(nested_offset) = find_nested_match_pattern_offset(binding) {
                        return Err(Diagnostic::new(
                            "parse",
                            "nested match patterns are not supported yet",
                        )
                        .with_path(path.display().to_string())
                        .with_span(line_no, binding_column + nested_offset));
                    }
                    validate_ident(binding, path, line_no, binding_column)?;
                    Ok(binding.to_string())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), bindings, false)
        } else {
            validate_ident(variant, path, line_no, 1)?;
            (variant.to_string(), Vec::new(), false)
        };
        *index += 1;
        let body = parse_stmt_list(lines, index, path)?;
        arms.push(MatchArm {
            variant,
            bindings,
            is_named,
            body,
            line: line_no,
            column: 1,
        });
    }
    Err(Diagnostic::new("parse", "missing closing brace for match")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_let_stmt(rest: &str, path: &Path, line_no: usize) -> Result<Stmt, Diagnostic> {
    let colon = find_top_level_char(rest, ':').ok_or_else(|| {
        Diagnostic::new("parse", "let binding is missing ':'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name = rest[..colon].trim();
    validate_ident(name, path, line_no, 5)?;
    let after_colon = &rest[colon + 1..];
    let eq = find_top_level_char(after_colon, '=').ok_or_else(|| {
        Diagnostic::new("parse", "let binding is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, colon + 2)
    })?;
    let type_text = after_colon[..eq].trim();
    let expr_text = after_colon[eq + 1..].trim();
    let ty = parse_type_name(type_text, path, line_no, colon + 2)?;
    let expr = parse_expr(expr_text, path, line_no, colon + eq + 3)?;
    Ok(Stmt::Let {
        name: name.to_string(),
        ty,
        expr,
        line: line_no,
        column: 1,
    })
}

fn parse_type_name(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<TypeName, Diagnostic> {
    if let Some(rest) = raw.strip_prefix("&'") {
        let lifetime_len = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(rest.len());
        let lifetime = &rest[..lifetime_len];
        if lifetime.is_empty() {
            return Err(
                Diagnostic::new("parse", "borrow lifetime annotation is missing a name")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + 1),
            );
        }
        validate_ident(lifetime, path, line_no, column + 2)?;
        let after_lifetime = rest[lifetime_len..].trim_start();
        let skipped_ws = rest[lifetime_len..].len() - after_lifetime.len();
        let inner_column = column + 2 + lifetime_len + skipped_ws;
        if after_lifetime.starts_with("mut [")
            && after_lifetime.ends_with(']')
            && matches!(find_matching_square(after_lifetime, 4), Some(close) if close == after_lifetime.len() - 1)
        {
            let inner = after_lifetime[5..after_lifetime.len() - 1].trim();
            if inner.is_empty() {
                return Err(Diagnostic::new(
                    "parse",
                    "mutable slice type is missing an inner type",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, inner_column + 5));
            }
            return Ok(TypeName::LifetimeMutSlice(
                lifetime.to_string(),
                Box::new(parse_type_name(inner, path, line_no, inner_column + 5)?),
            ));
        }
        if after_lifetime.starts_with('[')
            && after_lifetime.ends_with(']')
            && matches!(find_matching_square(after_lifetime, 0), Some(close) if close == after_lifetime.len() - 1)
        {
            let inner = after_lifetime[1..after_lifetime.len() - 1].trim();
            if inner.is_empty() {
                return Err(
                    Diagnostic::new("parse", "slice type is missing an inner type")
                        .with_path(path.display().to_string())
                        .with_span(line_no, inner_column + 1),
                );
            }
            return Ok(TypeName::LifetimeSlice(
                lifetime.to_string(),
                Box::new(parse_type_name(inner, path, line_no, inner_column + 1)?),
            ));
        }
        return Err(Diagnostic::new(
            "parse",
            "borrow lifetime annotations must use `&'a [T]` or `&'a mut [T]` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column));
    }
    if raw.starts_with("&mut [")
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 5), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[6..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "mutable slice type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::MutSlice(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 6,
        )?)));
    }
    if raw.starts_with("&[")
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 1), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[2..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "slice type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Slice(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 2,
        )?)));
    }
    if raw.starts_with("Option<")
        && raw.ends_with('>')
        && matches!(find_matching_angle(raw, 6), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[7..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "Option type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Option(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 7,
        )?)));
    }
    if raw.starts_with("Result<")
        && raw.ends_with('>')
        && matches!(find_matching_angle(raw, 6), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[7..raw.len() - 1].trim();
        let parts = split_top_level_type(inner, ',');
        if parts.len() != 2 {
            return Err(
                Diagnostic::new("parse", "Result type must use `Result<ok, err>` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        let ok_raw = parts[0].trim();
        let err_raw = parts[1].trim();
        if ok_raw.is_empty() || err_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "Result type is missing an ok or error type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Result(
            Box::new(parse_type_name(ok_raw, path, line_no, column + 7)?),
            Box::new(parse_type_name(err_raw, path, line_no, column + 7)?),
        ));
    }
    if raw.starts_with('{')
        && raw.ends_with('}')
        && matches!(find_matching_brace(raw, 0), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(Diagnostic::new("parse", "map type is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(inner, ':').ok_or_else(|| {
            Diagnostic::new("parse", "map type must use `{key: value}` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let key_raw = inner[..colon].trim();
        let value_raw = inner[colon + 1..].trim();
        if key_raw.is_empty() || value_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "map type is missing a key or value type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Map(
            Box::new(parse_type_name(key_raw, path, line_no, column + 1)?),
            Box::new(parse_type_name(
                value_raw,
                path,
                line_no,
                column + colon + 2,
            )?),
        ));
    }
    if is_wrapped_in_parens(raw) {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(Diagnostic::new("parse", "tuple type is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let items = split_top_level_type(inner, ',');
        if items.len() > 1 {
            return Ok(TypeName::Tuple(parse_tuple_type_names(
                inner,
                path,
                line_no,
                column + 1,
            )?));
        }
        return parse_type_name(inner, path, line_no, column + 1);
    }
    if raw.starts_with('[')
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 0), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "array type is missing an element type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Array(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 1,
        )?)));
    }
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(
                Diagnostic::new("parse", "generic types must use `Name<type>` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle),
            );
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let args_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if args_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "generic type is missing type arguments")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle + 1),
            );
        }
        let mut type_args = Vec::new();
        for arg in split_top_level_type(args_raw, ',') {
            type_args.push(parse_type_name(
                arg.trim(),
                path,
                line_no,
                column + open_angle + 1,
            )?);
        }
        if name == "ptr" {
            if type_args.len() != 1 {
                return Err(
                    Diagnostic::new("parse", "ptr types must use `ptr<type>` syntax")
                        .with_path(path.display().to_string())
                        .with_span(line_no, column),
                );
            }
            return Ok(TypeName::Ptr(Box::new(type_args.remove(0))));
        }
        if name == "mutptr" {
            if type_args.len() != 1 {
                return Err(Diagnostic::new(
                    "parse",
                    "mutptr types must use `mutptr<type>` syntax",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, column));
            }
            return Ok(TypeName::MutPtr(Box::new(type_args.remove(0))));
        }
        return Ok(TypeName::Named(name.to_string(), type_args));
    }
    match raw {
        "int" => Ok(TypeName::Int),
        "bool" => Ok(TypeName::Bool),
        "string" => Ok(TypeName::String),
        _ => {
            validate_ident(raw, path, line_no, column)?;
            Ok(TypeName::Named(raw.to_string(), Vec::new()))
        }
    }
}

fn parse_expr(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    let raw = raw.trim();
    if let Some((op, split_index)) = find_compare_operator(raw) {
        let lhs_raw = raw[..split_index].trim();
        let rhs_offset = split_index + op.lexeme().len();
        let rhs_raw = raw[rhs_offset..].trim();
        if lhs_raw.is_empty() || rhs_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "comparison expression is incomplete")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::BinaryCompare {
            op,
            lhs: Box::new(parse_add(lhs_raw, path, line_no, column)?),
            rhs: Box::new(parse_add(rhs_raw, path, line_no, column)?),
            line: line_no,
            column,
        });
    }
    parse_add(raw, path, line_no, column)
}

fn parse_add(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    let terms = split_top_level(raw, '+');
    if terms.len() > 1 {
        let mut expr = parse_term(terms[0].trim(), path, line_no, column)?;
        for term in &terms[1..] {
            let rhs = parse_term(term.trim(), path, line_no, column)?;
            expr = Expr::BinaryAdd {
                lhs: Box::new(expr),
                rhs: Box::new(rhs),
                line: line_no,
                column,
            };
        }
        return Ok(expr);
    }
    parse_term(raw.trim(), path, line_no, column)
}

fn parse_term(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    if raw.is_empty() {
        return Err(Diagnostic::new("parse", "expression is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }
    if raw.ends_with('?') {
        let inner = raw[..raw.len() - 1].trim_end();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "try expression is missing an operand")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Try {
            expr: Box::new(parse_term(inner, path, line_no, column)?),
            line: line_no,
            column: column + inner.len(),
        });
    }
    if let Some(inner) = raw.strip_prefix("await ") {
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "await expression is missing an operand")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Await {
            expr: Box::new(parse_term(inner, path, line_no, column + 6)?),
            line: line_no,
            column,
        });
    }
    if let Some(dot) = find_last_top_level_char(raw, '.') {
        let base_raw = raw[..dot].trim();
        let field = raw[dot + 1..].trim();
        if base_raw.is_empty() || field.is_empty() {
            return Err(Diagnostic::new("parse", "field access is incomplete")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        if field.ends_with(')')
            && let Some(open_paren) = find_top_level_char(field, '(')
            && matches!(find_matching_paren(field, open_paren), Some(close) if close == field.len() - 1)
        {
            let method_text = field[..open_paren].trim();
            let (method, type_args) =
                parse_call_name(method_text, path, line_no, column + dot + 1)?;
            validate_ident(method, path, line_no, column + dot + 1)?;
            let args = parse_call_args(
                &field[open_paren + 1..field.len() - 1],
                path,
                line_no,
                column + dot + 1,
            )?;
            return Ok(Expr::MethodCall {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                method: method.to_string(),
                type_args,
                args,
                line: line_no,
                column,
            });
        }
        if field.chars().all(|ch| ch.is_ascii_digit()) {
            let index = field.parse::<usize>().map_err(|_| {
                Diagnostic::new("parse", format!("invalid tuple index {field:?}"))
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + dot + 1)
            })?;
            return Ok(Expr::TupleIndex {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                index,
                line: line_no,
                column,
            });
        }
        validate_ident(field, path, line_no, column + dot + 1)?;
        return Ok(Expr::FieldAccess {
            base: Box::new(parse_term(base_raw, path, line_no, column)?),
            field: field.to_string(),
            line: line_no,
            column,
        });
    }
    if raw.ends_with(']')
        && let Some(open_bracket) = find_last_top_level_char(raw, '[')
        && matches!(find_matching_square(raw, open_bracket), Some(close) if close == raw.len() - 1)
    {
        let base_raw = raw[..open_bracket].trim();
        let index_raw = raw[open_bracket + 1..raw.len() - 1].trim();
        if base_raw.is_empty() {
            // This is an array literal, handled below.
        } else if let Some(colon) = find_top_level_char(index_raw, ':') {
            let start_raw = index_raw[..colon].trim();
            let end_raw = index_raw[colon + 1..].trim();
            return Ok(Expr::Slice {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                start: if start_raw.is_empty() {
                    None
                } else {
                    Some(Box::new(parse_expr(
                        start_raw,
                        path,
                        line_no,
                        column + open_bracket + 1,
                    )?))
                },
                end: if end_raw.is_empty() {
                    None
                } else {
                    Some(Box::new(parse_expr(
                        end_raw,
                        path,
                        line_no,
                        column + open_bracket + colon + 2,
                    )?))
                },
                line: line_no,
                column,
            });
        } else if index_raw.is_empty() {
            return Err(Diagnostic::new("parse", "array index is incomplete")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        } else {
            return Ok(Expr::Index {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                index: Box::new(parse_expr(
                    index_raw,
                    path,
                    line_no,
                    column + open_bracket + 1,
                )?),
                line: line_no,
                column,
            });
        }
    }
    if is_wrapped_in_parens(raw) {
        let inner = raw[1..raw.len() - 1].trim();
        if split_top_level(inner, ',').len() > 1 {
            return Ok(Expr::TupleLiteral {
                elements: parse_tuple_literal_elements(inner, path, line_no, column + 1)?,
                line: line_no,
                column,
            });
        }
        return parse_expr(&raw[1..raw.len() - 1], path, line_no, column + 1);
    }
    if raw.starts_with('{')
        && raw.ends_with('}')
        && matches!(find_matching_brace(raw, 0), Some(close) if close == raw.len() - 1)
    {
        return Ok(Expr::MapLiteral {
            entries: parse_map_literal_entries(&raw[1..raw.len() - 1], path, line_no, column + 1)?,
            line: line_no,
            column,
        });
    }
    if raw.starts_with('[')
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 0), Some(close) if close == raw.len() - 1)
    {
        return Ok(Expr::ArrayLiteral {
            elements: parse_array_literal_elements(&raw[1..raw.len() - 1], path, line_no, column)?,
            line: line_no,
            column,
        });
    }
    if raw.ends_with('}')
        && let Some(open_brace) = find_top_level_char(raw, '{')
        && matches!(find_matching_brace(raw, open_brace), Some(close) if close == raw.len() - 1)
    {
        let name = raw[..open_brace].trim();
        if !name.is_empty() {
            validate_ident(name, path, line_no, column)?;
            let fields = parse_struct_literal_fields(
                &raw[open_brace + 1..raw.len() - 1],
                path,
                line_no,
                column,
            )?;
            return Ok(Expr::StructLiteral {
                name: name.to_string(),
                fields,
                line: line_no,
                column,
            });
        }
    }
    if raw.starts_with('"') {
        let parsed = serde_json::from_str::<String>(raw).map_err(|_| {
            Diagnostic::new("parse", "invalid string literal")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        return Ok(Expr::Literal(Literal::String(parsed)));
    }
    if raw == "true" {
        return Ok(Expr::Literal(Literal::Bool(true)));
    }
    if raw == "false" {
        return Ok(Expr::Literal(Literal::Bool(false)));
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(Expr::Literal(Literal::Int(value)));
    }
    if raw.ends_with(')')
        && let Some(open_paren) = find_top_level_char(raw, '(')
    {
        let name_text = raw[..open_paren].trim();
        if !name_text.is_empty() {
            let (name, type_args) = parse_call_name(name_text, path, line_no, column)?;
            validate_ident(name, path, line_no, column)?;
            let args = parse_call_args(&raw[open_paren + 1..raw.len() - 1], path, line_no, column)?;
            return Ok(Expr::Call {
                name: name.to_string(),
                type_args,
                args,
                line: line_no,
                column,
            });
        }
    }
    validate_ident(raw, path, line_no, column)?;
    Ok(Expr::VarRef {
        name: raw.to_string(),
        line: line_no,
        column,
    })
}

fn parse_function_name<'a>(
    raw: &'a str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<String>), Diagnostic> {
    parse_decl_name(raw, "function", path, line_no, column)
}

fn parse_decl_name<'a>(
    raw: &'a str,
    kind: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<String>), Diagnostic> {
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(Diagnostic::new(
                "parse",
                format!("generic {kind} declarations must use `name<T>` syntax"),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle));
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let params_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if params_raw.is_empty() {
            return Err(Diagnostic::new(
                "parse",
                format!("generic {kind} is missing type parameters"),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle + 1));
        }
        let mut params = Vec::new();
        for param in split_top_level_type(params_raw, ',') {
            let param = param.trim();
            let type_param = if let Some(lifetime) = param.strip_prefix("'") {
                if lifetime.is_empty() {
                    return Err(
                        Diagnostic::new("parse", "lifetime parameter is missing a name")
                            .with_path(path.display().to_string())
                            .with_span(line_no, column + open_angle + 1),
                    );
                }
                validate_ident(lifetime, path, line_no, column + open_angle + 2)?;
                None
            } else {
                validate_ident(param, path, line_no, column + open_angle + 1)?;
                Some(param)
            };
            let Some(param) = type_param else {
                continue;
            };
            if params.iter().any(|existing| existing == param) {
                return Err(Diagnostic::new(
                    "parse",
                    format!("duplicate type parameter {param:?}"),
                )
                .with_path(path.display().to_string())
                .with_span(line_no, column + open_angle + 1));
            }
            params.push(param.to_string());
        }
        return Ok((name, params));
    }
    validate_ident(raw, path, line_no, column)?;
    Ok((raw, Vec::new()))
}

fn parse_call_name<'a>(
    raw: &'a str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<TypeName>), Diagnostic> {
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(Diagnostic::new(
                "parse",
                "generic calls must use `name<type>(args)` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle));
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let args_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if args_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "generic call is missing type arguments")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle + 1),
            );
        }
        let mut type_args = Vec::new();
        for arg in split_top_level_type(args_raw, ',') {
            type_args.push(parse_type_name(
                arg.trim(),
                path,
                line_no,
                column + open_angle + 1,
            )?);
        }
        return Ok((name, type_args));
    }
    Ok((raw, Vec::new()))
}

fn parse_array_literal_elements(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut elements = Vec::new();
    for element_text in split_top_level_type(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "array literal element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_expr(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_tuple_type_names(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<TypeName>, Diagnostic> {
    let mut elements = Vec::new();
    for element_text in split_top_level(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "tuple type element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_type_name(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_tuple_literal_elements(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    let mut elements = Vec::new();
    for element_text in split_top_level(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "tuple literal element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_expr(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_map_literal_entries(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<MapEntry>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry_text in split_top_level(raw, ',') {
        let entry_text = entry_text.trim();
        if entry_text.is_empty() {
            return Err(Diagnostic::new("parse", "map literal entry is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(entry_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "map literal entry is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let key = parse_expr(entry_text[..colon].trim(), path, line_no, column)?;
        let value = parse_expr(
            entry_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        entries.push(MapEntry {
            key,
            value,
            line: line_no,
            column,
        });
    }
    Ok(entries)
}

fn parse_struct_literal_fields(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<StructFieldValue>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut fields = Vec::new();
    for field_text in split_top_level_type(raw, ',') {
        let field_text = field_text.trim();
        if field_text.is_empty() {
            return Err(Diagnostic::new("parse", "struct literal field is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(field_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "struct literal field is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let name = field_text[..colon].trim();
        validate_ident(name, path, line_no, column)?;
        let expr = parse_expr(
            field_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        fields.push(StructFieldValue {
            name: name.to_string(),
            expr,
            line: line_no,
            column,
        });
    }
    Ok(fields)
}

fn parse_named_enum_payload_fields(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
    context: &str,
) -> Result<Vec<(String, TypeName)>, Diagnostic> {
    if raw.trim().is_empty() {
        return Err(Diagnostic::new("parse", format!("{context} is empty"))
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }
    let mut fields = Vec::new();
    for field_text in split_top_level(raw, ',') {
        let field_text = field_text.trim();
        if field_text.is_empty() {
            return Err(Diagnostic::new("parse", format!("{context} is empty"))
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(field_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", format!("{context} is missing ':'"))
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let name = field_text[..colon].trim();
        validate_ident(name, path, line_no, column)?;
        let ty = parse_type_name(
            field_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        fields.push((name.to_string(), ty));
    }
    Ok(fields)
}

fn parse_call_args(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut args = Vec::new();
    for arg_text in split_top_level(raw, ',') {
        let arg_text = arg_text.trim();
        if arg_text.is_empty() {
            return Err(Diagnostic::new("parse", "call argument is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        args.push(parse_expr(arg_text, path, line_no, column)?);
    }
    Ok(args)
}

fn validate_ident(
    value: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Diagnostic::new("parse", "identifier is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(
            Diagnostic::new("parse", format!("invalid identifier {value:?}"))
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    }
    Ok(())
}

fn find_nested_match_pattern_offset(raw: &str) -> Option<usize> {
    ['(', '{', '[', ':']
        .into_iter()
        .filter_map(|ch| find_top_level_char(raw, ch))
        .min()
}

fn split_top_level(raw: &str, delimiter: char) -> Vec<&str> {
    split_top_level_with_offsets(raw, delimiter)
        .into_iter()
        .map(|(_, part)| part)
        .collect()
}

fn split_top_level_with_offsets(raw: &str, delimiter: char) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut start = 0;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == delimiter && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                parts.push((start, &raw[start..index]));
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push((start, &raw[start..]));
    parts
}

fn find_top_level_keyword(raw: &str, keyword: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    for cursor in 0..chars.len() {
        let (index, ch) = chars[cursor];
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                angle_depth += 1;
            }
            '>' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                angle_depth = angle_depth.saturating_sub(1);
            }
            _ => {}
        }
        if paren_depth != 0 || brace_depth != 0 || bracket_depth != 0 || angle_depth != 0 {
            continue;
        }
        if !raw[index..].starts_with(keyword) {
            continue;
        }
        let before = if cursor == 0 {
            None
        } else {
            Some(chars[cursor - 1].1)
        };
        let after_index = index + keyword.len();
        let after = raw[after_index..].chars().next();
        if before.is_none_or(|ch| ch.is_ascii_whitespace())
            && after.is_none_or(|ch| ch.is_ascii_whitespace())
        {
            return Some(index);
        }
    }
    None
}

fn split_top_level_type(raw: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut start = 0;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            _ if ch == delimiter
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                parts.push(&raw[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&raw[start..]);
    parts
}

fn find_top_level_char(raw: &str, target: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if angle_depth > 0 {
            match ch {
                '<' => angle_depth += 1,
                '>' => angle_depth = angle_depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }
        match ch {
            '(' => {
                if target == '(' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                paren_depth += 1;
            }
            ')' => {
                if target == ')' && paren_depth == 1 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                paren_depth = paren_depth.saturating_sub(1);
            }
            '{' => {
                if target == '{' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                brace_depth += 1;
            }
            '}' => {
                if target == '}' && paren_depth == 0 && brace_depth == 1 && bracket_depth == 0 {
                    return Some(index);
                }
                brace_depth = brace_depth.saturating_sub(1);
            }
            '[' => {
                if target == '['
                    && paren_depth == 0
                    && brace_depth == 0
                    && bracket_depth == 0
                    && angle_depth == 0
                {
                    return Some(index);
                }
                bracket_depth += 1;
            }
            ']' => {
                if target == ']'
                    && paren_depth == 0
                    && brace_depth == 0
                    && bracket_depth == 1
                    && angle_depth == 0
                {
                    return Some(index);
                }
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            '<' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if target == '<' {
                    return Some(index);
                }
                angle_depth += 1;
            }
            '>' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if target == '>' {
                    return Some(index);
                }
                angle_depth = angle_depth.saturating_sub(1);
            }
            _ if ch == target
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn find_matching_paren(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_angle(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut angle_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '<' => angle_depth += 1,
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                if angle_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_brace(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' if paren_depth == 0 => brace_depth += 1,
            '}' if paren_depth == 0 => {
                brace_depth = brace_depth.saturating_sub(1);
                if brace_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_square(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' if paren_depth == 0 && brace_depth == 0 => bracket_depth += 1,
            ']' if paren_depth == 0 && brace_depth == 0 => {
                bracket_depth = bracket_depth.saturating_sub(1);
                if bracket_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_last_top_level_char(raw: &str, target: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut found = None;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => {
                if target == '[' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    found = Some(index);
                }
                bracket_depth += 1;
            }
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == target && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                found = Some(index)
            }
            _ => {}
        }
    }
    found
}

fn is_wrapped_in_parens(raw: &str) -> bool {
    raw.starts_with('(')
        && raw.ends_with(')')
        && matches!(find_matching_paren(raw, 0), Some(close) if close == raw.len() - 1)
}

fn find_compare_operator(raw: &str) -> Option<(CompareOp, usize)> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let mut cursor = 0;
    while cursor < chars.len() {
        let (index, ch) = chars[cursor];
        if escaped {
            escaped = false;
            cursor += 1;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            cursor += 1;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            cursor += 1;
            continue;
        }
        if in_string {
            cursor += 1;
            continue;
        }
        match ch {
            '(' => {
                paren_depth += 1;
                cursor += 1;
                continue;
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            '{' => {
                brace_depth += 1;
                cursor += 1;
                continue;
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            '[' => {
                bracket_depth += 1;
                cursor += 1;
                continue;
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            _ => {}
        }
        if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
            if ch == '<'
                && let Some(close_angle) = find_matching_angle(raw, index)
                && raw[close_angle + 1..].trim_start().starts_with('(')
            {
                cursor += 1;
                while cursor < chars.len() && chars[cursor].0 <= close_angle {
                    cursor += 1;
                }
                continue;
            }
            if let Some((_, next)) = chars.get(cursor + 1) {
                match (ch, *next) {
                    ('=', '=') => return Some((CompareOp::Eq, index)),
                    ('!', '=') => return Some((CompareOp::Ne, index)),
                    ('<', '=') => return Some((CompareOp::Le, index)),
                    ('>', '=') => return Some((CompareOp::Ge, index)),
                    _ => {}
                }
            }
            match ch {
                '<' => return Some((CompareOp::Lt, index)),
                '>' => return Some((CompareOp::Gt, index)),
                _ => {}
            }
        }
        cursor += 1;
    }
    None
}

fn skip_blank_lines(lines: &[&str], index: &mut usize) {
    while *index < lines.len() && lines[*index].trim().is_empty() {
        *index += 1;
    }
}

impl CompareOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }
}
