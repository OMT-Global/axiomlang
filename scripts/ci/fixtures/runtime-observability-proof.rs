use axiomc::runtime_observability::{
    DEFAULT_FLUSH_TIMEOUT_MS, DEFAULT_MAX_EVENT_BYTES, DEFAULT_MAX_FIELDS,
    DEFAULT_MAX_QUEUED_BYTES, ErrorContext, EventDisposition, EventRequest, EventSink, FieldValue,
    Level, ObservabilityRuntime, OverflowPolicy, PublicMessage, RuntimeConfig, RuntimeFilter,
    RuntimeOrigin, RuntimeState, SensitiveClass, SinkFailure, SinkKind,
};
use serde_json::{Value, json};

#[derive(Default)]
struct RecordingSink {
    events: Vec<Vec<u8>>,
    operations: Vec<&'static str>,
}

impl EventSink for RecordingSink {
    fn kind(&self) -> SinkKind {
        SinkKind::AuditJsonl
    }

    fn write_event(&mut self, event_json: &[u8]) -> Result<(), SinkFailure> {
        self.operations.push("write");
        self.events.push(event_json.to_vec());
        Ok(())
    }

    fn flush(&mut self) -> Result<(), SinkFailure> {
        self.operations.push("flush");
        Ok(())
    }
}

fn bounded_config() -> RuntimeConfig {
    RuntimeConfig::bounded(
        1,
        DEFAULT_MAX_QUEUED_BYTES,
        DEFAULT_MAX_EVENT_BYTES,
        DEFAULT_MAX_FIELDS,
        DEFAULT_FLUSH_TIMEOUT_MS,
        OverflowPolicy::DropNewest,
        RuntimeFilter::new(Level::Info),
    )
    .expect("valid bounded observability configuration")
}

fn main() {
    const SECRET: &str = "trusted-harness-secret-must-not-reach-the-sink";

    let mut runtime = ObservabilityRuntime::with_config(
        RecordingSink::default(),
        bounded_config(),
        RuntimeOrigin::Test,
    )
    .expect("runtime entropy and configuration are available");
    let context = runtime.start_context();
    let expected_correlation = context.evidence().clone();

    let mut event = EventRequest::new(
        Level::Info,
        "runtime.trusted_harness",
        PublicMessage::new("trusted runtime proof").expect("valid public message"),
    )
    .expect("valid event request")
    .with_correlation(context)
    .with_error(
        ErrorContext::new("runtime.proof", "validation")
            .expect("valid error identity")
            .with_message(FieldValue::sensitive(SensitiveClass::UntrustedText)),
    );
    event
        .insert_field(
            "password",
            FieldValue::public_text(SECRET).expect("bounded marker secret"),
        )
        .expect("valid secret-shaped field key");

    let receipt = runtime.emit(event);
    assert_eq!(receipt.disposition, EventDisposition::Accepted);
    assert_eq!(receipt.redacted_fields, 2);
    assert_eq!(receipt.queue_depth, 1);

    let shutdown = runtime.shutdown();
    assert_eq!(shutdown.state, RuntimeState::Flushed);
    assert_eq!(shutdown.queue_remaining, 0);
    assert_eq!(shutdown.queued_bytes, 0);
    assert!(shutdown.flush_attempted);
    assert!(!shutdown.deadline_exceeded);
    assert!(shutdown.sink_failure.is_none());
    assert_eq!(shutdown.counters.attempted, 1);
    assert_eq!(shutdown.counters.accepted, 1);
    assert_eq!(shutdown.counters.dropped, 0);
    assert_eq!(shutdown.counters.written, 1);
    assert_eq!(shutdown.counters.sink_failures, 0);

    let sink = runtime.into_sink();
    assert_eq!(sink.operations, ["write", "flush"]);
    assert_eq!(sink.events.len(), 1);
    let output = String::from_utf8(sink.events[0].clone()).expect("JSONL sink output is UTF-8");
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains(SECRET));

    let event: Value = serde_json::from_str(&output).expect("runtime event is JSON");
    assert_eq!(
        event.get("correlation"),
        Some(&serde_json::to_value(&expected_correlation).expect("correlation serializes")),
        "runtime event correlation must exactly match the issued context",
    );
    let proof = json!({
        "schema_version": "axiom.runtime_observability.runtime_proof.v1",
        "delivery": "executed_rust_runtime",
        "event": event,
        "shutdown_report": shutdown,
        "assertions": [
            "only sanitized bytes entered the bounded queue",
            "secret-key and sensitive-error values were redacted before sink write",
            "shutdown drained the accepted event before exactly one successful flush"
        ]
    });

    let mut pressure_runtime = ObservabilityRuntime::with_config(
        RecordingSink::default(),
        bounded_config(),
        RuntimeOrigin::Test,
    )
    .expect("runtime entropy and configuration are available");
    let first = pressure_runtime.emit(
        EventRequest::new(
            Level::Info,
            "runtime.trusted_harness",
            PublicMessage::new("first").expect("valid public message"),
        )
        .expect("valid event request"),
    );
    let overflow = pressure_runtime.emit(
        EventRequest::new(
            Level::Info,
            "runtime.trusted_harness",
            PublicMessage::new("overflow").expect("valid public message"),
        )
        .expect("valid event request"),
    );
    assert_eq!(first.disposition, EventDisposition::Accepted);
    assert_eq!(overflow.disposition, EventDisposition::Dropped);
    assert_eq!(overflow.queue_depth, 1);
    let pressure_shutdown = pressure_runtime.shutdown();
    assert_eq!(pressure_shutdown.counters.attempted, 2);
    assert_eq!(pressure_shutdown.counters.accepted, 1);
    assert_eq!(pressure_shutdown.counters.dropped, 1);
    let pressure_sink = pressure_runtime.into_sink();
    assert_eq!(pressure_sink.operations, ["write", "flush"]);
    assert_eq!(pressure_sink.events.len(), 1);

    println!(
        "{}",
        serde_json::to_string(&proof).expect("runtime proof serializes")
    );
}
