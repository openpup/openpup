use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

pub trait EventSink: Send + Sync {
    fn emit_value(&self, event: &str, payload: Value);
}

pub type SharedEventSink = Arc<dyn EventSink>;

pub fn emit_event<T: Serialize>(sink: &dyn EventSink, event: &str, payload: T) {
    let value = serde_json::to_value(payload).unwrap_or(Value::Null);
    sink.emit_value(event, value);
}

#[derive(Default)]
pub struct NullEventSink;

impl EventSink for NullEventSink {
    fn emit_value(&self, _event: &str, _payload: Value) {}
}
