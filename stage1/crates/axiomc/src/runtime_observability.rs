//! Bounded, exporter-neutral runtime observability primitives.
//!
//! This module is deliberately synchronous. It provides executable log event
//! filtering, typed fields, redaction, bounded queueing, sink failure latching,
//! and deterministic drain/flush ordering without claiming task propagation,
//! metrics, or a preemptible asynchronous exporter. Only sanitized JSON bytes
//! enter a sink queue.

use serde::Serialize;
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const EVIDENCE_SCHEMA_VERSION: &str = "axiom.runtime_observability.evidence.v1";
pub const DEFAULT_MAX_EVENT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_FIELDS: usize = 32;
pub const DEFAULT_QUEUE_CAPACITY: usize = 1024;
pub const DEFAULT_MAX_QUEUED_BYTES: usize = 1024 * 1024;
pub const DEFAULT_FLUSH_TIMEOUT_MS: u64 = 2_000;
pub const REDACTION_REPLACEMENT: &str = "[REDACTED]";

const MAX_TARGET_BYTES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_FIELD_KEY_BYTES: usize = 128;
const MAX_PUBLIC_TEXT_BYTES: usize = 16 * 1024;
const MAX_FILTER_TARGETS: usize = 128;

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    DropNewest,
    DropOldest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Running,
    Draining,
    Flushed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOrigin {
    Native,
    Compiler,
    Test,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SinkKind {
    AuditJsonl,
    Stderr,
    Stdout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventDisposition {
    Accepted,
    Filtered,
    Dropped,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveClass {
    CapabilityProtected,
    Credential,
    PersonalData,
    UntrustedText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationError {
    code: &'static str,
}

impl ValidationError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for ValidationError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SinkFailure {
    code: String,
    error_kind: String,
}

impl SinkFailure {
    pub fn new(code: impl Into<String>, error_kind: impl Into<String>) -> Self {
        let code = code.into();
        let error_kind = error_kind.into();
        Self {
            code: sanitized_sink_failure_code(&code).to_owned(),
            error_kind: sanitized_sink_error_kind(&error_kind).to_owned(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn error_kind(&self) -> &str {
        &self.error_kind
    }

    fn write(error: std::io::Error) -> Self {
        Self::new(
            "observability.sink_write_failed",
            io_error_kind(error.kind()),
        )
    }

    fn flush(error: std::io::Error) -> Self {
        Self::new(
            "observability.sink_flush_failed",
            io_error_kind(error.kind()),
        )
    }

    fn for_write(self) -> Self {
        Self::new("observability.sink_write_failed", self.error_kind)
    }

    fn for_flush(self) -> Self {
        Self::new("observability.sink_flush_failed", self.error_kind)
    }
}

fn io_error_kind(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::ConnectionRefused => "connection_refused",
        std::io::ErrorKind::ConnectionReset => "connection_reset",
        std::io::ErrorKind::ConnectionAborted => "connection_aborted",
        std::io::ErrorKind::NotConnected => "not_connected",
        std::io::ErrorKind::AddrInUse => "address_in_use",
        std::io::ErrorKind::AddrNotAvailable => "address_not_available",
        std::io::ErrorKind::BrokenPipe => "broken_pipe",
        std::io::ErrorKind::AlreadyExists => "already_exists",
        std::io::ErrorKind::WouldBlock => "would_block",
        std::io::ErrorKind::InvalidInput => "invalid_input",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::TimedOut => "timed_out",
        std::io::ErrorKind::WriteZero => "write_zero",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        std::io::ErrorKind::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}

fn sanitized_sink_failure_code(value: &str) -> &'static str {
    match value {
        "observability.shutdown_timeout" => "observability.shutdown_timeout",
        "observability.sink_failed" => "observability.sink_failed",
        "observability.sink_flush_failed" => "observability.sink_flush_failed",
        "observability.sink_write_failed" => "observability.sink_write_failed",
        _ => "observability.sink_failed",
    }
}

fn sanitized_sink_error_kind(value: &str) -> &'static str {
    match value {
        "address_in_use" => "address_in_use",
        "address_not_available" => "address_not_available",
        "already_exists" => "already_exists",
        "broken_pipe" => "broken_pipe",
        "connection_aborted" => "connection_aborted",
        "connection_refused" => "connection_refused",
        "connection_reset" => "connection_reset",
        "interrupted" => "interrupted",
        "invalid_data" => "invalid_data",
        "invalid_input" => "invalid_input",
        "not_connected" => "not_connected",
        "not_found" => "not_found",
        "other" => "other",
        "out_of_memory" => "out_of_memory",
        "permission_denied" => "permission_denied",
        "timed_out" => "timed_out",
        "unexpected_eof" => "unexpected_eof",
        "would_block" => "would_block",
        "write_zero" => "write_zero",
        _ => "other",
    }
}

pub trait EventSink {
    fn kind(&self) -> SinkKind;
    fn write_event(&mut self, event_json: &[u8]) -> Result<(), SinkFailure>;
    fn flush(&mut self) -> Result<(), SinkFailure>;
}

pub struct JsonLinesSink<W> {
    writer: W,
    kind: SinkKind,
}

impl<W> JsonLinesSink<W> {
    pub fn new(writer: W, kind: SinkKind) -> Self {
        Self { writer, kind }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> EventSink for JsonLinesSink<W> {
    fn kind(&self) -> SinkKind {
        self.kind
    }

    fn write_event(&mut self, event_json: &[u8]) -> Result<(), SinkFailure> {
        self.writer
            .write_all(event_json)
            .map_err(SinkFailure::write)?;
        self.writer.write_all(b"\n").map_err(SinkFailure::write)
    }

    fn flush(&mut self) -> Result<(), SinkFailure> {
        self.writer.flush().map_err(SinkFailure::flush)
    }
}

pub trait RuntimeClock {
    fn now_unix_millis(&self) -> u64;
    fn now_monotonic_millis(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl RuntimeClock for SystemClock {
    fn now_unix_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }

    fn now_monotonic_millis(&self) -> u64 {
        let origin = MONOTONIC_ORIGIN.get_or_init(Instant::now);
        u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicMessage(String);

impl PublicMessage {
    /// Marks a non-interpolated message template as public log data.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_text(&value, MAX_MESSAGE_BYTES, "observability.invalid_message")?;
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
enum FieldStorage {
    Null,
    Boolean(bool),
    Integer(i64),
    Unsigned(u64),
    Float(Number),
    PublicText(String),
    Sensitive(SensitiveClass),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldValue(FieldStorage);

impl FieldValue {
    pub fn null() -> Self {
        Self(FieldStorage::Null)
    }

    pub fn boolean(value: bool) -> Self {
        Self(FieldStorage::Boolean(value))
    }

    pub fn integer(value: i64) -> Self {
        Self(FieldStorage::Integer(value))
    }

    pub fn unsigned(value: u64) -> Self {
        Self(FieldStorage::Unsigned(value))
    }

    pub fn float(value: f64) -> Result<Self, ValidationError> {
        Number::from_f64(value)
            .map(FieldStorage::Float)
            .map(Self)
            .ok_or_else(|| ValidationError::new("observability.non_finite_float"))
    }

    /// Explicitly marks bounded text as safe for sink emission.
    pub fn public_text(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_text(
            &value,
            MAX_PUBLIC_TEXT_BYTES,
            "observability.invalid_public_text",
        )?;
        Ok(Self(FieldStorage::PublicText(value)))
    }

    /// Records the classification without retaining the protected value.
    pub fn sensitive(class: SensitiveClass) -> Self {
        Self(FieldStorage::Sensitive(class))
    }

    fn sanitize(
        &self,
        key_is_sensitive: bool,
        forced_reason: Option<RedactionReason>,
    ) -> (TypedField, bool) {
        if key_is_sensitive
            || forced_reason.is_some()
            || matches!(self.0, FieldStorage::Sensitive(_))
        {
            let reason = forced_reason.unwrap_or(if key_is_sensitive {
                RedactionReason::SensitiveKey
            } else {
                RedactionReason::SensitiveValue
            });
            let sensitivity_class = match self.0 {
                FieldStorage::Sensitive(class) => Some(class),
                _ => None,
            };
            return (
                TypedField {
                    field_type: "redacted",
                    value: Value::String(REDACTION_REPLACEMENT.into()),
                    redaction: Some(RedactionEvidence {
                        reason,
                        sensitivity_class,
                    }),
                },
                true,
            );
        }
        let (field_type, value) = match &self.0 {
            FieldStorage::Null => ("null", Value::Null),
            FieldStorage::Boolean(value) => ("boolean", Value::Bool(*value)),
            FieldStorage::Integer(value) => ("integer", Value::Number((*value).into())),
            FieldStorage::Unsigned(value) => ("unsigned", Value::Number((*value).into())),
            FieldStorage::Float(value) => ("float", Value::Number(value.clone())),
            FieldStorage::PublicText(value) => ("string", Value::String(value.clone())),
            FieldStorage::Sensitive(_) => unreachable!("sensitive values are redacted above"),
        };
        (
            TypedField {
                field_type,
                value,
                redaction: None,
            },
            false,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ErrorContext {
    code: String,
    category: String,
    message: Option<FieldValue>,
}

impl ErrorContext {
    pub fn new(
        code: impl Into<String>,
        category: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let code = code.into();
        let category = category.into();
        validate_identifier(&code, "observability.invalid_error_code")?;
        validate_identifier(&category, "observability.invalid_error_category")?;
        Ok(Self {
            code,
            category,
            message: None,
        })
    }

    pub fn with_message(mut self, message: FieldValue) -> Self {
        self.message = Some(message);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CorrelationEvidence {
    pub request_id: String,
    pub runtime_origin: RuntimeOrigin,
    pub span_id: String,
    pub trace_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationContext {
    runtime_instance: u64,
    evidence: CorrelationEvidence,
}

impl CorrelationContext {
    pub fn evidence(&self) -> &CorrelationEvidence {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FilterInspection {
    pub minimum_level: Level,
    pub target_levels: BTreeMap<String, Level>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeFilter {
    minimum_level: Level,
    target_levels: BTreeMap<String, Level>,
}

impl RuntimeFilter {
    pub fn new(minimum_level: Level) -> Self {
        Self {
            minimum_level,
            target_levels: BTreeMap::new(),
        }
    }

    pub fn set_target(
        &mut self,
        target: impl Into<String>,
        minimum_level: Level,
    ) -> Result<(), ValidationError> {
        let target = target.into();
        validate_target(&target)?;
        if !self.target_levels.contains_key(&target)
            && self.target_levels.len() >= MAX_FILTER_TARGETS
        {
            return Err(ValidationError::new(
                "observability.too_many_filter_targets",
            ));
        }
        self.target_levels.insert(target, minimum_level);
        Ok(())
    }

    pub fn remove_target(&mut self, target: &str) {
        self.target_levels.remove(target);
    }

    fn enabled(&self, target: &str, level: Level) -> bool {
        level
            >= self
                .target_levels
                .get(target)
                .copied()
                .unwrap_or(self.minimum_level)
    }

    fn inspect(&self) -> FilterInspection {
        FilterInspection {
            minimum_level: self.minimum_level,
            target_levels: self.target_levels.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    queue_capacity: usize,
    max_queued_bytes: usize,
    max_event_bytes: usize,
    max_fields: usize,
    flush_timeout_ms: u64,
    overflow_policy: OverflowPolicy,
    filter: RuntimeFilter,
}

impl RuntimeConfig {
    pub fn bounded(
        queue_capacity: usize,
        max_queued_bytes: usize,
        max_event_bytes: usize,
        max_fields: usize,
        flush_timeout_ms: u64,
        overflow_policy: OverflowPolicy,
        filter: RuntimeFilter,
    ) -> Result<Self, ValidationError> {
        let config = Self {
            queue_capacity,
            max_queued_bytes,
            max_event_bytes,
            max_fields,
            flush_timeout_ms,
            overflow_policy,
            filter,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if self.queue_capacity == 0 || self.queue_capacity > 4096 {
            return Err(ValidationError::new("observability.invalid_queue_capacity"));
        }
        if self.max_queued_bytes == 0 || self.max_queued_bytes > 64 * 1024 * 1024 {
            return Err(ValidationError::new(
                "observability.invalid_queue_byte_limit",
            ));
        }
        if self.max_event_bytes == 0 || self.max_event_bytes > DEFAULT_MAX_EVENT_BYTES {
            return Err(ValidationError::new(
                "observability.invalid_event_byte_limit",
            ));
        }
        if self.max_fields == 0 || self.max_fields > DEFAULT_MAX_FIELDS {
            return Err(ValidationError::new("observability.invalid_field_limit"));
        }
        if self.flush_timeout_ms == 0 || self.flush_timeout_ms > 60_000 {
            return Err(ValidationError::new("observability.invalid_flush_timeout"));
        }
        if self.filter.target_levels.len() > MAX_FILTER_TARGETS {
            return Err(ValidationError::new(
                "observability.too_many_filter_targets",
            ));
        }
        for target in self.filter.target_levels.keys() {
            validate_target(target)?;
        }
        Ok(())
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            max_queued_bytes: DEFAULT_MAX_QUEUED_BYTES,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            max_fields: DEFAULT_MAX_FIELDS,
            flush_timeout_ms: DEFAULT_FLUSH_TIMEOUT_MS,
            overflow_policy: OverflowPolicy::DropNewest,
            filter: RuntimeFilter::new(Level::Info),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventRequest {
    level: Level,
    target: String,
    message: PublicMessage,
    fields: BTreeMap<String, FieldValue>,
    error: Option<ErrorContext>,
    correlation: Option<CorrelationContext>,
}

impl EventRequest {
    pub fn new(
        level: Level,
        target: impl Into<String>,
        message: PublicMessage,
    ) -> Result<Self, ValidationError> {
        let target = target.into();
        validate_target(&target)?;
        Ok(Self {
            level,
            target,
            message,
            fields: BTreeMap::new(),
            error: None,
            correlation: None,
        })
    }

    pub fn insert_field(
        &mut self,
        key: impl Into<String>,
        value: FieldValue,
    ) -> Result<Option<FieldValue>, ValidationError> {
        let key = key.into();
        validate_field_key(&key)?;
        if !self.fields.contains_key(&key) && self.fields.len() >= DEFAULT_MAX_FIELDS {
            return Err(ValidationError::new("observability.too_many_fields"));
        }
        Ok(self.fields.insert(key, value))
    }

    pub fn with_error(mut self, error: ErrorContext) -> Self {
        self.error = Some(error);
        self
    }

    pub fn with_correlation(mut self, correlation: CorrelationContext) -> Self {
        self.correlation = Some(correlation);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct TypedField {
    #[serde(rename = "type")]
    field_type: &'static str,
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    redaction: Option<RedactionEvidence>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RedactionReason {
    ErrorMessage,
    SensitiveKey,
    SensitiveValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RedactionEvidence {
    reason: RedactionReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    sensitivity_class: Option<SensitiveClass>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct SanitizedErrorContext {
    category: String,
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<TypedField>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct RuntimeEvent {
    schema_version: &'static str,
    kind: &'static str,
    sequence: u64,
    timestamp_unix_ms: u64,
    level: Level,
    target: String,
    message: String,
    fields: BTreeMap<String, TypedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<SanitizedErrorContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<CorrelationEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EmitReceipt {
    pub schema_version: &'static str,
    pub kind: &'static str,
    pub disposition: EventDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_sha256: Option<String>,
    pub event_bytes: usize,
    pub redacted_fields: usize,
    pub evicted_events: u64,
    pub queue_depth: usize,
    pub queued_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeCounters {
    pub attempted: u64,
    pub accepted: u64,
    pub filtered: u64,
    pub dropped: u64,
    pub rejected: u64,
    pub written: u64,
    pub sink_failures: u64,
}

impl RuntimeCounters {
    fn new() -> Self {
        Self {
            attempted: 0,
            accepted: 0,
            filtered: 0,
            dropped: 0,
            rejected: 0,
            written: 0,
            sink_failures: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeInspection {
    pub schema_version: &'static str,
    pub kind: &'static str,
    pub state: RuntimeState,
    pub health: &'static str,
    pub sink_kind: SinkKind,
    pub queue_capacity: usize,
    pub queue_depth: usize,
    pub max_queued_bytes: usize,
    pub queued_bytes: usize,
    pub max_event_bytes: usize,
    pub max_fields: usize,
    pub overflow_policy: OverflowPolicy,
    pub flush_timeout_ms: u64,
    pub filter_revision: u64,
    pub filter: FilterInspection,
    pub counters: RuntimeCounters,
    pub last_sink_failure: Option<SinkFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DrainReport {
    pub schema_version: &'static str,
    pub kind: &'static str,
    pub state: RuntimeState,
    pub attempted: u64,
    pub written: u64,
    pub queue_remaining: usize,
    pub queued_bytes: usize,
    pub sink_failure: Option<SinkFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ShutdownReport {
    pub schema_version: &'static str,
    pub kind: &'static str,
    pub state: RuntimeState,
    pub queue_remaining: usize,
    pub queued_bytes: usize,
    pub flush_attempted: bool,
    pub deadline_exceeded: bool,
    pub counters: RuntimeCounters,
    pub sink_failure: Option<SinkFailure>,
}

#[derive(Clone, Debug)]
struct QueuedEvent {
    bytes: Vec<u8>,
}

pub struct ObservabilityRuntime<S, C = SystemClock> {
    sink: S,
    clock: C,
    config: RuntimeConfig,
    runtime_origin: RuntimeOrigin,
    runtime_instance: u64,
    next_sequence: u64,
    next_context: u64,
    filter_revision: u64,
    state: RuntimeState,
    queue: VecDeque<QueuedEvent>,
    queued_bytes: usize,
    counters: RuntimeCounters,
    last_sink_failure: Option<SinkFailure>,
    terminal_flush_attempted: bool,
    terminal_deadline_exceeded: bool,
}

impl<S: EventSink> ObservabilityRuntime<S, SystemClock> {
    pub fn new(sink: S, runtime_origin: RuntimeOrigin) -> Result<Self, ValidationError> {
        Self::with_clock(sink, RuntimeConfig::default(), SystemClock, runtime_origin)
    }

    pub fn with_config(
        sink: S,
        config: RuntimeConfig,
        runtime_origin: RuntimeOrigin,
    ) -> Result<Self, ValidationError> {
        Self::with_clock(sink, config, SystemClock, runtime_origin)
    }
}

impl<S: EventSink, C: RuntimeClock> ObservabilityRuntime<S, C> {
    pub fn with_clock(
        sink: S,
        config: RuntimeConfig,
        clock: C,
        runtime_origin: RuntimeOrigin,
    ) -> Result<Self, ValidationError> {
        config.validate()?;
        Ok(Self {
            sink,
            clock,
            config,
            runtime_origin,
            runtime_instance: next_runtime_instance()?,
            next_sequence: 1,
            next_context: 1,
            filter_revision: 1,
            state: RuntimeState::Running,
            queue: VecDeque::new(),
            queued_bytes: 0,
            counters: RuntimeCounters::new(),
            last_sink_failure: None,
            terminal_flush_attempted: false,
            terminal_deadline_exceeded: false,
        })
    }

    pub fn start_context(&mut self) -> CorrelationContext {
        let context = self.next_context;
        self.next_context = self.next_context.saturating_add(1);
        CorrelationContext {
            runtime_instance: self.runtime_instance,
            evidence: CorrelationEvidence {
                request_id: format!("req-{:016x}-{:016x}", self.runtime_instance, context),
                runtime_origin: self.runtime_origin,
                trace_id: format!("{:016x}{:016x}", self.runtime_instance, context),
                span_id: format!("{:016x}", context),
            },
        }
    }

    pub fn child_span(
        &mut self,
        parent: &CorrelationContext,
    ) -> Result<CorrelationContext, ValidationError> {
        if parent.runtime_instance != self.runtime_instance {
            return Err(ValidationError::new(
                "observability.foreign_correlation_context",
            ));
        }
        let context = self.next_context;
        self.next_context = self.next_context.saturating_add(1);
        Ok(CorrelationContext {
            runtime_instance: self.runtime_instance,
            evidence: CorrelationEvidence {
                request_id: parent.evidence.request_id.clone(),
                runtime_origin: parent.evidence.runtime_origin,
                trace_id: parent.evidence.trace_id.clone(),
                span_id: format!("{:016x}", context),
            },
        })
    }

    pub fn update_filter(&mut self, filter: RuntimeFilter) -> Result<u64, ValidationError> {
        if self.state != RuntimeState::Running {
            return Err(ValidationError::new("observability.runtime_not_running"));
        }
        if filter.target_levels.len() > MAX_FILTER_TARGETS {
            return Err(ValidationError::new(
                "observability.too_many_filter_targets",
            ));
        }
        for target in filter.target_levels.keys() {
            validate_target(target)?;
        }
        self.config.filter = filter;
        self.filter_revision = self.filter_revision.saturating_add(1);
        Ok(self.filter_revision)
    }

    pub fn emit(&mut self, request: EventRequest) -> EmitReceipt {
        self.counters.attempted = self.counters.attempted.saturating_add(1);
        if self.state != RuntimeState::Running {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return self.receipt(
                EventDisposition::Rejected,
                None,
                None,
                0,
                0,
                0,
                Some("observability.runtime_not_running"),
            );
        }
        if !self.config.filter.enabled(&request.target, request.level) {
            self.counters.filtered = self.counters.filtered.saturating_add(1);
            return self.receipt(EventDisposition::Filtered, None, None, 0, 0, 0, None);
        }
        if request.fields.len() > self.config.max_fields {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return self.receipt(
                EventDisposition::Rejected,
                None,
                None,
                0,
                0,
                0,
                Some("observability.too_many_fields"),
            );
        }
        if request
            .correlation
            .as_ref()
            .is_some_and(|context| context.runtime_instance != self.runtime_instance)
        {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return self.receipt(
                EventDisposition::Rejected,
                None,
                None,
                0,
                0,
                0,
                Some("observability.foreign_correlation_context"),
            );
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let (event, redacted_fields) = self.sanitize_event(sequence, request);
        let bytes = match serde_json::to_vec(&event) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.counters.rejected = self.counters.rejected.saturating_add(1);
                return self.receipt(
                    EventDisposition::Rejected,
                    Some(sequence),
                    None,
                    0,
                    redacted_fields,
                    0,
                    Some("observability.event_serialization_failed"),
                );
            }
        };
        if bytes.len() > self.config.max_event_bytes {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return self.receipt(
                EventDisposition::Rejected,
                Some(sequence),
                None,
                bytes.len(),
                redacted_fields,
                0,
                Some("observability.event_too_large"),
            );
        }
        let digest = hex_digest(&bytes);
        if bytes.len() > self.config.max_queued_bytes {
            self.counters.dropped = self.counters.dropped.saturating_add(1);
            return self.receipt(
                EventDisposition::Dropped,
                Some(sequence),
                Some(digest),
                bytes.len(),
                redacted_fields,
                0,
                Some("observability.event_exceeds_queue_byte_limit"),
            );
        }
        let mut evicted_events = 0u64;
        if !self.queue_fits(bytes.len()) {
            match self.config.overflow_policy {
                OverflowPolicy::DropNewest => {
                    self.counters.dropped = self.counters.dropped.saturating_add(1);
                    return self.receipt(
                        EventDisposition::Dropped,
                        Some(sequence),
                        Some(digest),
                        bytes.len(),
                        redacted_fields,
                        0,
                        Some("observability.queue_full_drop_newest"),
                    );
                }
                OverflowPolicy::DropOldest => {
                    while !self.queue_fits(bytes.len()) && !self.queue.is_empty() {
                        if let Some(evicted) = self.queue.pop_front() {
                            self.queued_bytes =
                                self.queued_bytes.saturating_sub(evicted.bytes.len());
                            self.counters.dropped = self.counters.dropped.saturating_add(1);
                            evicted_events = evicted_events.saturating_add(1);
                        }
                    }
                    if !self.queue_fits(bytes.len()) {
                        self.counters.dropped = self.counters.dropped.saturating_add(1);
                        return self.receipt(
                            EventDisposition::Dropped,
                            Some(sequence),
                            Some(digest),
                            bytes.len(),
                            redacted_fields,
                            evicted_events,
                            Some("observability.event_exceeds_queue_byte_limit"),
                        );
                    }
                }
            }
        }
        self.queued_bytes = self.queued_bytes.saturating_add(bytes.len());
        self.queue.push_back(QueuedEvent { bytes });
        self.counters.accepted = self.counters.accepted.saturating_add(1);
        self.receipt(
            EventDisposition::Accepted,
            Some(sequence),
            Some(digest),
            self.queue.back().map_or(0, |event| event.bytes.len()),
            redacted_fields,
            evicted_events,
            None,
        )
    }

    pub fn drain(&mut self, max_events: usize) -> DrainReport {
        let mut attempted = 0u64;
        let mut written = 0u64;
        if self.state == RuntimeState::Flushed || self.state == RuntimeState::Failed {
            return self.drain_report(attempted, written);
        }
        while attempted < max_events as u64 {
            let Some(event) = self.queue.front() else {
                break;
            };
            attempted = attempted.saturating_add(1);
            if let Err(failure) = self.sink.write_event(&event.bytes) {
                self.latch_sink_failure(failure.for_write());
                break;
            }
            let event = self.queue.pop_front().expect("front event still exists");
            self.queued_bytes = self.queued_bytes.saturating_sub(event.bytes.len());
            self.counters.written = self.counters.written.saturating_add(1);
            written = written.saturating_add(1);
        }
        self.drain_report(attempted, written)
    }

    pub fn shutdown(&mut self) -> ShutdownReport {
        if self.state == RuntimeState::Flushed || self.state == RuntimeState::Failed {
            return self.shutdown_report(
                self.terminal_flush_attempted,
                self.terminal_deadline_exceeded,
            );
        }
        self.state = RuntimeState::Draining;
        let started_at = self.clock.now_monotonic_millis();
        while let Some(event) = self.queue.front() {
            let result = self.sink.write_event(&event.bytes);
            let operation_deadline_exceeded = deadline_exceeded(
                started_at,
                self.clock.now_monotonic_millis(),
                self.config.flush_timeout_ms,
            );
            if let Err(failure) = result {
                if operation_deadline_exceeded {
                    self.latch_sink_failure(SinkFailure::new(
                        "observability.shutdown_timeout",
                        "timed_out",
                    ));
                    return self.finish_shutdown(false, true);
                }
                self.latch_sink_failure(failure.for_write());
                return self.finish_shutdown(false, false);
            }
            let event = self.queue.pop_front().expect("front event still exists");
            self.queued_bytes = self.queued_bytes.saturating_sub(event.bytes.len());
            self.counters.written = self.counters.written.saturating_add(1);
            if operation_deadline_exceeded {
                self.latch_sink_failure(SinkFailure::new(
                    "observability.shutdown_timeout",
                    "timed_out",
                ));
                return self.finish_shutdown(false, true);
            }
        }
        let flush_attempted = true;
        let result = self.sink.flush();
        let operation_deadline_exceeded = deadline_exceeded(
            started_at,
            self.clock.now_monotonic_millis(),
            self.config.flush_timeout_ms,
        );
        if operation_deadline_exceeded {
            self.latch_sink_failure(SinkFailure::new(
                "observability.shutdown_timeout",
                "timed_out",
            ));
            return self.finish_shutdown(flush_attempted, true);
        }
        if let Err(failure) = result {
            self.latch_sink_failure(failure.for_flush());
            return self.finish_shutdown(flush_attempted, false);
        }
        self.state = RuntimeState::Flushed;
        self.finish_shutdown(flush_attempted, false)
    }

    pub fn inspect(&self) -> RuntimeInspection {
        let health = match self.state {
            RuntimeState::Running
                if self.queue.len() < self.config.queue_capacity
                    && self.queued_bytes < self.config.max_queued_bytes =>
            {
                "ready"
            }
            RuntimeState::Running | RuntimeState::Draining => "degraded",
            RuntimeState::Flushed => "stopped",
            RuntimeState::Failed => "failed",
        };
        RuntimeInspection {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            kind: "inspection",
            state: self.state,
            health,
            sink_kind: self.sink.kind(),
            queue_capacity: self.config.queue_capacity,
            queue_depth: self.queue.len(),
            max_queued_bytes: self.config.max_queued_bytes,
            queued_bytes: self.queued_bytes,
            max_event_bytes: self.config.max_event_bytes,
            max_fields: self.config.max_fields,
            overflow_policy: self.config.overflow_policy,
            flush_timeout_ms: self.config.flush_timeout_ms,
            filter_revision: self.filter_revision,
            filter: self.config.filter.inspect(),
            counters: self.counters.clone(),
            last_sink_failure: self.last_sink_failure.clone(),
        }
    }

    pub fn into_sink(self) -> S {
        self.sink
    }

    fn sanitize_event(&self, sequence: u64, request: EventRequest) -> (RuntimeEvent, usize) {
        let mut redacted_fields = 0usize;
        let fields = request
            .fields
            .into_iter()
            .map(|(key, value)| {
                let (value, redacted) = value.sanitize(sensitive_key(&key), None);
                redacted_fields += usize::from(redacted);
                (key, value)
            })
            .collect();
        let error = request.error.map(|error| {
            let message = error.message.map(|message| {
                let (message, redacted) =
                    message.sanitize(false, Some(RedactionReason::ErrorMessage));
                redacted_fields += usize::from(redacted);
                message
            });
            SanitizedErrorContext {
                category: error.category,
                code: error.code,
                message,
            }
        });
        (
            RuntimeEvent {
                schema_version: EVIDENCE_SCHEMA_VERSION,
                kind: "event",
                sequence,
                timestamp_unix_ms: self.clock.now_unix_millis(),
                level: request.level,
                target: request.target,
                message: request.message.as_str().to_owned(),
                fields,
                error,
                correlation: request.correlation.map(|context| context.evidence),
            },
            redacted_fields,
        )
    }

    fn queue_fits(&self, event_bytes: usize) -> bool {
        self.queue.len() < self.config.queue_capacity
            && self
                .queued_bytes
                .checked_add(event_bytes)
                .is_some_and(|bytes| bytes <= self.config.max_queued_bytes)
    }

    #[allow(clippy::too_many_arguments)]
    fn receipt(
        &self,
        disposition: EventDisposition,
        sequence: Option<u64>,
        event_sha256: Option<String>,
        event_bytes: usize,
        redacted_fields: usize,
        evicted_events: u64,
        reason: Option<&str>,
    ) -> EmitReceipt {
        EmitReceipt {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            kind: "emit_receipt",
            disposition,
            sequence,
            event_sha256,
            event_bytes,
            redacted_fields,
            evicted_events,
            queue_depth: self.queue.len(),
            queued_bytes: self.queued_bytes,
            reason: reason.map(str::to_owned),
        }
    }

    fn latch_sink_failure(&mut self, failure: SinkFailure) {
        if self.last_sink_failure.is_none() {
            self.counters.sink_failures = self.counters.sink_failures.saturating_add(1);
            self.last_sink_failure = Some(failure);
        }
        self.state = RuntimeState::Failed;
    }

    fn drain_report(&self, attempted: u64, written: u64) -> DrainReport {
        DrainReport {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            kind: "drain_report",
            state: self.state,
            attempted,
            written,
            queue_remaining: self.queue.len(),
            queued_bytes: self.queued_bytes,
            sink_failure: self.last_sink_failure.clone(),
        }
    }

    fn shutdown_report(&self, flush_attempted: bool, deadline_exceeded: bool) -> ShutdownReport {
        ShutdownReport {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            kind: "shutdown_report",
            state: self.state,
            queue_remaining: self.queue.len(),
            queued_bytes: self.queued_bytes,
            flush_attempted,
            deadline_exceeded,
            counters: self.counters.clone(),
            sink_failure: self.last_sink_failure.clone(),
        }
    }

    fn finish_shutdown(
        &mut self,
        flush_attempted: bool,
        deadline_exceeded: bool,
    ) -> ShutdownReport {
        self.terminal_flush_attempted = flush_attempted;
        self.terminal_deadline_exceeded = deadline_exceeded;
        self.shutdown_report(flush_attempted, deadline_exceeded)
    }
}

fn next_runtime_instance() -> Result<u64, ValidationError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|_| ValidationError::new("observability.runtime_entropy_unavailable"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn validate_text(value: &str, limit: usize, code: &'static str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > limit
        || value
            .chars()
            .any(|character| character.is_control() && character != '\t')
    {
        return Err(ValidationError::new(code));
    }
    Ok(())
}

fn validate_target(value: &str) -> Result<(), ValidationError> {
    validate_text(value, MAX_TARGET_BYTES, "observability.invalid_target")?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'))
    {
        return Err(ValidationError::new("observability.invalid_target"));
    }
    Ok(())
}

fn validate_field_key(value: &str) -> Result<(), ValidationError> {
    validate_text(
        value,
        MAX_FIELD_KEY_BYTES,
        "observability.invalid_field_key",
    )?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(ValidationError::new("observability.invalid_field_key"));
    }
    Ok(())
}

fn validate_identifier(value: &str, code: &'static str) -> Result<(), ValidationError> {
    validate_text(value, MAX_FIELD_KEY_BYTES, code)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(ValidationError::new(code));
    }
    Ok(())
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let normalized = String::from_utf8(normalized).expect("ASCII normalization remains UTF-8");
    [
        "apikey",
        "authorization",
        "cookie",
        "credential",
        "password",
        "passwd",
        "privatekey",
        "secret",
        "sessiontoken",
        "token",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn deadline_exceeded(started_at: u64, now: u64, timeout_ms: u64) -> bool {
    now.saturating_sub(started_at) >= timeout_ms
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct ManualClock {
        wall: Rc<RefCell<u64>>,
        monotonic: Rc<RefCell<u64>>,
    }

    impl ManualClock {
        fn at(value: u64) -> Self {
            Self {
                wall: Rc::new(RefCell::new(value)),
                monotonic: Rc::new(RefCell::new(value)),
            }
        }

        fn advance(&self, millis: u64) {
            let mut wall = self.wall.borrow_mut();
            *wall = wall.saturating_add(millis);
            let mut monotonic = self.monotonic.borrow_mut();
            *monotonic = monotonic.saturating_add(millis);
        }

        fn rewind_wall(&self, millis: u64) {
            let mut wall = self.wall.borrow_mut();
            *wall = wall.saturating_sub(millis);
        }
    }

    impl RuntimeClock for ManualClock {
        fn now_unix_millis(&self) -> u64 {
            *self.wall.borrow()
        }

        fn now_monotonic_millis(&self) -> u64 {
            *self.monotonic.borrow()
        }
    }

    #[derive(Clone, Default)]
    struct RecordingState {
        events: Vec<Vec<u8>>,
        operations: Vec<&'static str>,
    }

    #[derive(Clone)]
    struct RecordingSink {
        state: Rc<RefCell<RecordingState>>,
        fail_write_at: Option<usize>,
        write_failure: Option<SinkFailure>,
        fail_flush: bool,
        flush_failure: Option<SinkFailure>,
        clock: Option<ManualClock>,
        write_advance_ms: u64,
        write_wall_rewind_ms: u64,
        flush_advance_ms: u64,
    }

    impl RecordingSink {
        fn new() -> (Self, Rc<RefCell<RecordingState>>) {
            let state = Rc::new(RefCell::new(RecordingState::default()));
            (
                Self {
                    state: Rc::clone(&state),
                    fail_write_at: None,
                    write_failure: None,
                    fail_flush: false,
                    flush_failure: None,
                    clock: None,
                    write_advance_ms: 0,
                    write_wall_rewind_ms: 0,
                    flush_advance_ms: 0,
                },
                state,
            )
        }
    }

    impl EventSink for RecordingSink {
        fn kind(&self) -> SinkKind {
            SinkKind::AuditJsonl
        }

        fn write_event(&mut self, event_json: &[u8]) -> Result<(), SinkFailure> {
            self.state.borrow_mut().operations.push("write");
            if let Some(clock) = &self.clock {
                clock.advance(self.write_advance_ms);
                clock.rewind_wall(self.write_wall_rewind_ms);
            }
            if self
                .fail_write_at
                .is_some_and(|index| self.state.borrow().events.len() == index)
            {
                return Err(self.write_failure.clone().unwrap_or_else(|| {
                    SinkFailure::new("observability.sink_write_failed", "broken_pipe")
                }));
            }
            self.state.borrow_mut().events.push(event_json.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), SinkFailure> {
            self.state.borrow_mut().operations.push("flush");
            if let Some(clock) = &self.clock {
                clock.advance(self.flush_advance_ms);
            }
            if self.fail_flush {
                return Err(self.flush_failure.clone().unwrap_or_else(|| {
                    SinkFailure::new("observability.sink_flush_failed", "write_zero")
                }));
            }
            Ok(())
        }
    }

    fn message(value: &str) -> PublicMessage {
        PublicMessage::new(value).unwrap()
    }

    fn request(level: Level, message_value: &str) -> EventRequest {
        EventRequest::new(level, "runtime.test", message(message_value)).unwrap()
    }

    fn config(
        capacity: usize,
        max_queued_bytes: usize,
        overflow: OverflowPolicy,
        timeout: u64,
    ) -> RuntimeConfig {
        RuntimeConfig::bounded(
            capacity,
            max_queued_bytes,
            DEFAULT_MAX_EVENT_BYTES,
            DEFAULT_MAX_FIELDS,
            timeout,
            overflow,
            RuntimeFilter::new(Level::Info),
        )
        .unwrap()
    }

    #[test]
    fn runtime_filter_updates_take_effect_without_restarting() {
        let (sink, _) = RecordingSink::new();
        let clock = ManualClock::at(1_000);
        let mut runtime = ObservabilityRuntime::with_clock(
            sink,
            RuntimeConfig::default(),
            clock,
            RuntimeOrigin::Test,
        )
        .unwrap();

        assert_eq!(
            runtime.emit(request(Level::Debug, "hidden")).disposition,
            EventDisposition::Filtered
        );
        let mut filter = RuntimeFilter::new(Level::Warn);
        filter.set_target("runtime.test", Level::Trace).unwrap();
        assert_eq!(runtime.update_filter(filter).unwrap(), 2);
        assert_eq!(
            runtime.emit(request(Level::Debug, "visible")).disposition,
            EventDisposition::Accepted
        );
        let inspection = runtime.inspect();
        assert_eq!(inspection.filter_revision, 2);
        assert_eq!(inspection.counters.filtered, 1);
        assert_eq!(inspection.counters.accepted, 1);
    }

    #[test]
    fn typed_fields_and_runtime_correlation_are_machine_readable() {
        let (sink, state) = RecordingSink::new();
        let clock = ManualClock::at(42);
        let mut runtime = ObservabilityRuntime::with_clock(
            sink,
            RuntimeConfig::default(),
            clock,
            RuntimeOrigin::Native,
        )
        .unwrap();
        let context = runtime.start_context();
        let child = runtime.child_span(&context).unwrap();
        assert_eq!(context.evidence().trace_id, child.evidence().trace_id);
        assert_ne!(context.evidence().span_id, child.evidence().span_id);

        let mut event = request(Level::Info, "typed").with_correlation(child);
        event.insert_field("none", FieldValue::null()).unwrap();
        event
            .insert_field("ready", FieldValue::boolean(true))
            .unwrap();
        event
            .insert_field("attempt", FieldValue::integer(-2))
            .unwrap();
        event
            .insert_field("bytes", FieldValue::unsigned(9))
            .unwrap();
        event
            .insert_field("ratio", FieldValue::float(1.5).unwrap())
            .unwrap();
        event
            .insert_field("component", FieldValue::public_text("worker").unwrap())
            .unwrap();
        let receipt = runtime.emit(event);
        assert_eq!(receipt.disposition, EventDisposition::Accepted);
        assert_eq!(runtime.drain(1).written, 1);

        let value: Value = serde_json::from_slice(&state.borrow().events[0]).unwrap();
        assert_eq!(value["schema_version"], EVIDENCE_SCHEMA_VERSION);
        assert_eq!(value["kind"], "event");
        assert_eq!(value["timestamp_unix_ms"], 42);
        assert_eq!(value["fields"]["ready"]["type"], "boolean");
        assert_eq!(value["fields"]["attempt"]["value"], -2);
        assert_eq!(value["fields"]["bytes"]["type"], "unsigned");
        assert_eq!(value["fields"]["ratio"]["value"], 1.5);
        assert_eq!(value["fields"]["component"]["value"], "worker");
        assert_eq!(
            value["correlation"]["trace_id"],
            context.evidence().trace_id
        );
    }

    #[test]
    fn redaction_happens_before_queueing_and_covers_keys_values_and_errors() {
        let (sink, state) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        let mut event = request(Level::Info, "login attempted").with_error(
            ErrorContext::new("auth.denied", "authorization")
                .unwrap()
                .with_message(FieldValue::sensitive(SensitiveClass::UntrustedText)),
        );
        event
            .insert_field(
                "Authorization-Token",
                FieldValue::public_text("marker-key-secret").unwrap(),
            )
            .unwrap();
        event
            .insert_field(
                "user_note",
                FieldValue::sensitive(SensitiveClass::PersonalData),
            )
            .unwrap();
        event
            .insert_field("attempt", FieldValue::integer(2))
            .unwrap();

        let receipt = runtime.emit(event);
        assert_eq!(receipt.redacted_fields, 3);
        runtime.shutdown();
        let event = String::from_utf8(state.borrow().events[0].clone()).unwrap();
        assert!(!event.contains("marker-key-secret"));
        assert!(event.matches(REDACTION_REPLACEMENT).count() >= 3);
        assert!(event.contains("auth.denied"));
        assert!(event.contains("\"attempt\""));
    }

    #[test]
    fn event_bounds_reject_excess_fields_and_bytes_without_sink_delivery() {
        let (sink, state) = RecordingSink::new();
        let bounded = RuntimeConfig::bounded(
            2,
            512,
            256,
            2,
            500,
            OverflowPolicy::DropNewest,
            RuntimeFilter::new(Level::Info),
        )
        .unwrap();
        let mut runtime =
            ObservabilityRuntime::with_config(sink, bounded, RuntimeOrigin::Test).unwrap();
        let mut fields = request(Level::Info, "fields");
        for index in 0..3 {
            fields
                .insert_field(format!("field_{index}"), FieldValue::integer(index))
                .unwrap();
        }
        let fields_receipt = runtime.emit(fields);
        assert_eq!(fields_receipt.disposition, EventDisposition::Rejected);
        assert_eq!(
            fields_receipt.reason.as_deref(),
            Some("observability.too_many_fields")
        );

        let mut oversized = request(Level::Info, "large");
        oversized
            .insert_field("payload", FieldValue::public_text("x".repeat(200)).unwrap())
            .unwrap();
        let oversized_receipt = runtime.emit(oversized);
        assert_eq!(oversized_receipt.disposition, EventDisposition::Rejected);
        assert_eq!(
            oversized_receipt.reason.as_deref(),
            Some("observability.event_too_large")
        );
        assert!(state.borrow().events.is_empty());
        assert_eq!(runtime.inspect().queue_depth, 0);
    }

    #[test]
    fn event_request_caps_distinct_fields_before_unbounded_allocation() {
        let mut event = request(Level::Info, "bounded fields");
        for index in 0..DEFAULT_MAX_FIELDS {
            event
                .insert_field(format!("field_{index}"), FieldValue::integer(index as i64))
                .unwrap();
        }
        assert!(
            event
                .insert_field("field_0", FieldValue::integer(99))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            event
                .insert_field("one_too_many", FieldValue::integer(100))
                .unwrap_err()
                .code(),
            "observability.too_many_fields"
        );
    }

    #[test]
    fn drop_newest_preserves_accepted_order_and_exposes_count() {
        let (sink, state) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::with_config(
            sink,
            config(2, DEFAULT_MAX_QUEUED_BYTES, OverflowPolicy::DropNewest, 500),
            RuntimeOrigin::Test,
        )
        .unwrap();
        assert_eq!(
            runtime.emit(request(Level::Info, "one")).disposition,
            EventDisposition::Accepted
        );
        assert_eq!(
            runtime.emit(request(Level::Info, "two")).disposition,
            EventDisposition::Accepted
        );
        let dropped = runtime.emit(request(Level::Info, "three"));
        assert_eq!(dropped.disposition, EventDisposition::Dropped);
        assert_eq!(runtime.inspect().counters.dropped, 1);
        assert_eq!(runtime.shutdown().state, RuntimeState::Flushed);
        let events = state
            .borrow()
            .events
            .iter()
            .map(|bytes| {
                serde_json::from_slice::<Value>(bytes).unwrap()["message"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(events, ["one", "two"]);
    }

    #[test]
    fn drop_oldest_evicts_until_both_queue_bounds_fit() {
        let (sink, state) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::with_config(
            sink,
            config(2, DEFAULT_MAX_QUEUED_BYTES, OverflowPolicy::DropOldest, 500),
            RuntimeOrigin::Test,
        )
        .unwrap();
        runtime.emit(request(Level::Info, "one"));
        runtime.emit(request(Level::Info, "two"));
        let receipt = runtime.emit(request(Level::Info, "three"));
        assert_eq!(receipt.disposition, EventDisposition::Accepted);
        assert_eq!(receipt.evicted_events, 1);
        runtime.shutdown();
        let events = state
            .borrow()
            .events
            .iter()
            .map(|bytes| {
                serde_json::from_slice::<Value>(bytes).unwrap()["message"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(events, ["two", "three"]);
        assert_eq!(runtime.inspect().counters.dropped, 1);
    }

    #[test]
    fn queue_byte_limit_applies_even_when_event_count_has_capacity() {
        let large_message = "x".repeat(300);
        let (sink, _) = RecordingSink::new();
        let mut drop_newest = ObservabilityRuntime::with_config(
            sink,
            config(8, 512, OverflowPolicy::DropNewest, 500),
            RuntimeOrigin::Test,
        )
        .unwrap();
        assert_eq!(
            drop_newest
                .emit(request(Level::Info, &large_message))
                .disposition,
            EventDisposition::Accepted
        );
        let dropped = drop_newest.emit(request(Level::Info, &large_message));
        assert_eq!(dropped.disposition, EventDisposition::Dropped);
        assert_eq!(dropped.queue_depth, 1);
        assert!(dropped.queued_bytes <= 512);

        let (sink, state) = RecordingSink::new();
        let mut drop_oldest = ObservabilityRuntime::with_config(
            sink,
            config(8, 512, OverflowPolicy::DropOldest, 500),
            RuntimeOrigin::Test,
        )
        .unwrap();
        drop_oldest.emit(request(Level::Info, &large_message));
        let accepted = drop_oldest.emit(request(Level::Info, "replacement"));
        assert_eq!(accepted.disposition, EventDisposition::Accepted);
        assert_eq!(accepted.evicted_events, 1);
        assert_eq!(accepted.queue_depth, 1);
        drop_oldest.shutdown();
        let event: Value = serde_json::from_slice(&state.borrow().events[0]).unwrap();
        assert_eq!(event["message"], "replacement");
    }

    #[test]
    fn drop_oldest_preserves_queued_events_when_new_event_can_never_fit() {
        let (sink, state) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::with_config(
            sink,
            config(8, 512, OverflowPolicy::DropOldest, 500),
            RuntimeOrigin::Test,
        )
        .unwrap();
        assert_eq!(
            runtime.emit(request(Level::Info, "keep")).disposition,
            EventDisposition::Accepted
        );
        let oversized = runtime.emit(request(Level::Info, &"x".repeat(600)));
        assert_eq!(oversized.disposition, EventDisposition::Dropped);
        assert_eq!(oversized.evicted_events, 0);
        assert_eq!(
            oversized.reason.as_deref(),
            Some("observability.event_exceeds_queue_byte_limit")
        );
        assert_eq!(runtime.inspect().queue_depth, 1);
        runtime.shutdown();
        let event: Value = serde_json::from_slice(&state.borrow().events[0]).unwrap();
        assert_eq!(event["message"], "keep");
    }

    #[test]
    fn sink_failure_is_latched_once_and_never_reported_as_flushed() {
        let (mut sink, state) = RecordingSink::new();
        sink.fail_write_at = Some(0);
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        runtime.emit(request(Level::Info, "one"));
        runtime.emit(request(Level::Info, "two"));
        let first = runtime.drain(10);
        assert_eq!(first.state, RuntimeState::Failed);
        assert_eq!(first.queue_remaining, 2);
        let second = runtime.drain(10);
        assert_eq!(second.attempted, 0);
        let shutdown = runtime.shutdown();
        assert_eq!(shutdown.state, RuntimeState::Failed);
        assert!(!shutdown.flush_attempted);
        assert_eq!(shutdown.counters.sink_failures, 1);
        assert_eq!(state.borrow().operations, ["write"]);
    }

    #[test]
    fn shutdown_stops_intake_drains_in_order_then_flushes() {
        let (sink, state) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        runtime.emit(request(Level::Info, "one"));
        runtime.emit(request(Level::Info, "two"));
        let report = runtime.shutdown();
        assert_eq!(report.state, RuntimeState::Flushed);
        assert_eq!(report.queue_remaining, 0);
        assert!(report.flush_attempted);
        assert_eq!(state.borrow().operations, ["write", "write", "flush"]);
        let rejected = runtime.emit(request(Level::Info, "late"));
        assert_eq!(rejected.disposition, EventDisposition::Rejected);
        assert_eq!(
            rejected.reason.as_deref(),
            Some("observability.runtime_not_running")
        );
        let repeated = runtime.shutdown();
        assert_eq!(repeated.state, RuntimeState::Flushed);
        assert!(repeated.flush_attempted);
        assert!(!repeated.deadline_exceeded);
        assert_eq!(repeated.counters.rejected, 1);
        assert_eq!(state.borrow().operations, ["write", "write", "flush"]);
    }

    #[test]
    fn shutdown_deadline_failure_is_observable_and_not_flushed() {
        let clock = ManualClock::at(100);
        let (mut sink, state) = RecordingSink::new();
        sink.clock = Some(clock.clone());
        sink.write_advance_ms = 50;
        sink.write_wall_rewind_ms = 100;
        let mut runtime = ObservabilityRuntime::with_clock(
            sink,
            config(2, DEFAULT_MAX_QUEUED_BYTES, OverflowPolicy::DropNewest, 50),
            clock,
            RuntimeOrigin::Test,
        )
        .unwrap();
        runtime.emit(request(Level::Info, "one"));
        let report = runtime.shutdown();
        assert_eq!(report.state, RuntimeState::Failed);
        assert!(report.deadline_exceeded);
        assert!(!report.flush_attempted);
        assert_eq!(report.queue_remaining, 0);
        assert_eq!(state.borrow().operations, ["write"]);
        assert_eq!(
            report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.shutdown_timeout")
        );
    }

    #[test]
    fn flush_failure_is_latched_and_cannot_claim_shutdown_success() {
        let (mut sink, state) = RecordingSink::new();
        sink.fail_flush = true;
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        runtime.emit(request(Level::Info, "one"));
        let report = runtime.shutdown();
        assert_eq!(report.state, RuntimeState::Failed);
        assert!(report.flush_attempted);
        assert!(!report.deadline_exceeded);
        assert_eq!(report.queue_remaining, 0);
        assert_eq!(report.counters.written, 1);
        assert_eq!(report.counters.sink_failures, 1);
        assert_eq!(state.borrow().operations, ["write", "flush"]);
        assert_eq!(
            report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.sink_flush_failed")
        );
    }

    #[test]
    fn shutdown_normalizes_custom_failures_to_the_attempted_sink_operation() {
        let (mut write_sink, _) = RecordingSink::new();
        write_sink.fail_write_at = Some(0);
        write_sink.write_failure = Some(SinkFailure::new(
            "observability.sink_flush_failed",
            "broken_pipe",
        ));
        let mut write_runtime = ObservabilityRuntime::new(write_sink, RuntimeOrigin::Test).unwrap();
        write_runtime.emit(request(Level::Info, "write"));
        let write_report = write_runtime.shutdown();
        assert_eq!(
            write_report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.sink_write_failed")
        );
        assert!(!write_report.flush_attempted);

        let (mut flush_sink, _) = RecordingSink::new();
        flush_sink.fail_flush = true;
        flush_sink.flush_failure = Some(SinkFailure::new(
            "observability.sink_write_failed",
            "write_zero",
        ));
        let mut flush_runtime = ObservabilityRuntime::new(flush_sink, RuntimeOrigin::Test).unwrap();
        let flush_report = flush_runtime.shutdown();
        assert_eq!(
            flush_report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.sink_flush_failed")
        );
        assert!(flush_report.flush_attempted);
    }

    #[test]
    fn shutdown_deadline_takes_precedence_when_a_sink_operation_returns_an_error() {
        let write_clock = ManualClock::at(100);
        let (mut write_sink, _) = RecordingSink::new();
        write_sink.clock = Some(write_clock.clone());
        write_sink.write_advance_ms = 51;
        write_sink.fail_write_at = Some(0);
        let mut write_runtime = ObservabilityRuntime::with_clock(
            write_sink,
            config(1, DEFAULT_MAX_QUEUED_BYTES, OverflowPolicy::DropNewest, 50),
            write_clock,
            RuntimeOrigin::Test,
        )
        .unwrap();
        write_runtime.emit(request(Level::Info, "write timeout"));
        let write_report = write_runtime.shutdown();
        assert!(write_report.deadline_exceeded);
        assert_eq!(
            write_report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.shutdown_timeout")
        );

        let flush_clock = ManualClock::at(100);
        let (mut flush_sink, _) = RecordingSink::new();
        flush_sink.clock = Some(flush_clock.clone());
        flush_sink.flush_advance_ms = 51;
        flush_sink.fail_flush = true;
        let mut flush_runtime = ObservabilityRuntime::with_clock(
            flush_sink,
            config(1, DEFAULT_MAX_QUEUED_BYTES, OverflowPolicy::DropNewest, 50),
            flush_clock,
            RuntimeOrigin::Test,
        )
        .unwrap();
        let flush_report = flush_runtime.shutdown();
        assert!(flush_report.deadline_exceeded);
        assert_eq!(
            flush_report.sink_failure.as_ref().map(SinkFailure::code),
            Some("observability.shutdown_timeout")
        );
    }

    #[test]
    fn foreign_runtime_context_is_rejected_without_leaking_identity() {
        let (sink_a, _) = RecordingSink::new();
        let (sink_b, state_b) = RecordingSink::new();
        let mut runtime_a = ObservabilityRuntime::new(sink_a, RuntimeOrigin::Native).unwrap();
        let mut runtime_b = ObservabilityRuntime::new(sink_b, RuntimeOrigin::Compiler).unwrap();
        let context = runtime_a.start_context();
        let independent = runtime_b.start_context();
        assert_ne!(
            context.evidence().request_id,
            independent.evidence().request_id
        );
        assert_ne!(context.evidence().trace_id, independent.evidence().trace_id);
        assert_eq!(
            runtime_b.child_span(&context).unwrap_err().code(),
            "observability.foreign_correlation_context"
        );
        let receipt = runtime_b.emit(request(Level::Info, "foreign").with_correlation(context));
        assert_eq!(receipt.disposition, EventDisposition::Rejected);
        assert!(state_b.borrow().events.is_empty());
    }

    #[test]
    fn json_lines_sink_emits_exactly_one_line_per_sanitized_event() {
        let sink = JsonLinesSink::new(Vec::<u8>::new(), SinkKind::AuditJsonl);
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        runtime.emit(request(Level::Info, "one"));
        runtime.emit(request(Level::Warn, "two"));
        assert_eq!(runtime.shutdown().state, RuntimeState::Flushed);
        let bytes = runtime.into_sink().into_inner();
        let lines = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(&lines[0]).unwrap()["message"],
            "one"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&lines[1]).unwrap()["message"],
            "two"
        );
    }

    #[test]
    fn inspection_is_closed_machine_readable_health_without_event_values() {
        let (sink, _) = RecordingSink::new();
        let mut runtime = ObservabilityRuntime::new(sink, RuntimeOrigin::Test).unwrap();
        let mut event = request(Level::Info, "safe template");
        event
            .insert_field(
                "password",
                FieldValue::public_text("must-not-appear-in-inspection").unwrap(),
            )
            .unwrap();
        runtime.emit(event);
        let inspection = serde_json::to_string(&runtime.inspect()).unwrap();
        assert!(inspection.contains("\"health\":\"ready\""));
        assert!(inspection.contains("\"queue_depth\":1"));
        assert!(!inspection.contains("must-not-appear-in-inspection"));
        assert!(!inspection.contains("safe template"));
    }

    #[test]
    fn validators_reject_control_characters_non_finite_values_and_unbounded_config() {
        assert_eq!(
            PublicMessage::new("bad\nmessage").unwrap_err().code(),
            "observability.invalid_message"
        );
        assert_eq!(
            FieldValue::float(f64::NAN).unwrap_err().code(),
            "observability.non_finite_float"
        );
        assert_eq!(
            EventRequest::new(Level::Info, "bad target!", message("safe"))
                .unwrap_err()
                .code(),
            "observability.invalid_target"
        );
        assert_eq!(
            RuntimeConfig::bounded(
                0,
                1,
                1,
                1,
                1,
                OverflowPolicy::DropNewest,
                RuntimeFilter::new(Level::Info),
            )
            .unwrap_err()
            .code(),
            "observability.invalid_queue_capacity"
        );
        let failure = SinkFailure::new(
            "observability.runtime-token-must-not-survive",
            "credential-must-not-survive",
        );
        assert_eq!(failure.code(), "observability.sink_failed");
        assert_eq!(failure.error_kind(), "other");
        let serialized_failure = serde_json::to_string(&failure).unwrap();
        assert!(!serialized_failure.contains("runtime-token-must-not-survive"));
        assert!(!serialized_failure.contains("credential-must-not-survive"));
    }

    #[test]
    fn every_machine_evidence_variant_validates_against_the_published_schema() {
        let schema: Value = serde_json::from_str(include_str!(
            "../../../compiler-contracts/schemas/axiom.runtime_observability_evidence.v1.schema.json"
        ))
        .unwrap();
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&schema)
            .expect("compile observability evidence schema");
        let (sink, state) = RecordingSink::new();
        let clock = ManualClock::at(10);
        let mut runtime = ObservabilityRuntime::with_clock(
            sink,
            RuntimeConfig::default(),
            clock,
            RuntimeOrigin::Test,
        )
        .unwrap();
        let receipt = runtime.emit(request(Level::Info, "schema proof"));
        let inspection = runtime.inspect();
        let drain = runtime.drain(1);
        let event: Value = serde_json::from_slice(&state.borrow().events[0]).unwrap();
        let shutdown = runtime.shutdown();
        let shutdown_value = serde_json::to_value(&shutdown).unwrap();
        let (mut failing_sink, _) = RecordingSink::new();
        failing_sink.fail_write_at = Some(0);
        failing_sink.write_failure = Some(SinkFailure::new(
            "observability.runtime-token-must-not-survive",
            "credential-must-not-survive",
        ));
        let mut failing_runtime =
            ObservabilityRuntime::new(failing_sink, RuntimeOrigin::Test).unwrap();
        failing_runtime.emit(request(Level::Info, "schema failure proof"));
        failing_runtime.drain(1);
        let sanitized_failure_inspection = failing_runtime.inspect();

        for (name, value) in [
            ("event", event),
            ("emit receipt", serde_json::to_value(receipt).unwrap()),
            ("inspection", serde_json::to_value(inspection).unwrap()),
            (
                "sanitized failure inspection",
                serde_json::to_value(sanitized_failure_inspection).unwrap(),
            ),
            ("drain report", serde_json::to_value(drain).unwrap()),
            ("shutdown report", shutdown_value.clone()),
        ] {
            let errors = validator
                .iter_errors(&value)
                .map(|error| error.to_string())
                .collect::<Vec<_>>();
            assert!(errors.is_empty(), "{name} schema errors: {errors:?}");
        }

        for (name, value) in [
            ("nonempty flushed queue", {
                let mut value = shutdown_value.clone();
                value["queue_remaining"] = Value::from(1);
                value
            }),
            ("flushed without a flush", {
                let mut value = shutdown_value.clone();
                value["flush_attempted"] = Value::from(false);
                value
            }),
            ("running shutdown", {
                let mut value = shutdown_value.clone();
                value["state"] = Value::from("running");
                value
            }),
            ("failed without failure evidence", {
                let mut value = shutdown_value.clone();
                value["state"] = Value::from("failed");
                value
            }),
        ] {
            assert!(
                !validator.is_valid(&value),
                "{name} must be rejected by the shutdown evidence schema"
            );
        }
    }

    #[test]
    fn checked_runtime_golden_is_produced_by_the_executable_state_machine() {
        let golden: Value = serde_json::from_str(include_str!(
            "../../../compiler-contracts/fixtures/observability-v1/runtime-core-golden.json"
        ))
        .unwrap();
        let (sink, state) = RecordingSink::new();
        let clock = ManualClock::at(42);
        let mut runtime = ObservabilityRuntime::with_clock(
            sink,
            RuntimeConfig::default(),
            clock,
            RuntimeOrigin::Native,
        )
        .unwrap();
        let correlation = CorrelationContext {
            runtime_instance: runtime.runtime_instance,
            evidence: CorrelationEvidence {
                request_id: "req-0000000000000001-0000000000000001".to_string(),
                runtime_origin: RuntimeOrigin::Native,
                span_id: "0000000000000001".to_string(),
                trace_id: "00000000000000010000000000000001".to_string(),
            },
        };
        let mut event = request(Level::Info, "login attempted").with_error(
            ErrorContext::new("auth.denied", "authorization")
                .unwrap()
                .with_message(FieldValue::sensitive(SensitiveClass::UntrustedText)),
        ).with_correlation(correlation);
        event
            .insert_field("attempt", FieldValue::integer(2))
            .unwrap();
        event
            .insert_field(
                "password",
                FieldValue::public_text("golden-must-not-contain-this").unwrap(),
            )
            .unwrap();
        let receipt = runtime.emit(event);
        assert_eq!(receipt.redacted_fields, 2);
        let shutdown = runtime.shutdown();
        let produced_event: Value = serde_json::from_slice(&state.borrow().events[0]).unwrap();
        assert_eq!(produced_event, golden["event"]);
        assert_eq!(
            serde_json::to_value(shutdown).unwrap(),
            golden["shutdown_report"]
        );
        let serialized = serde_json::to_string(&golden).unwrap();
        assert!(!serialized.contains("golden-must-not-contain-this"));
    }
}
