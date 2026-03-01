//! Custom tracing layer that captures log events into a ring buffer
//! instead of writing to stdout, for TUI display.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing_subscriber::Layer;

pub struct RingBufferLayer {
    buffer: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl RingBufferLayer {
    pub fn new(capacity: usize) -> (Self, Arc<Mutex<VecDeque<String>>>) {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        let layer = Self {
            buffer: Arc::clone(&buffer),
            capacity,
        };
        (layer, buffer)
    }
}

impl<S> Layer<S> for RingBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let level = event.metadata().level();
        let target = event.metadata().target();
        let line = format!(
            "{} {:>5} {} {}",
            chrono::Local::now().format("%H:%M:%S"),
            level,
            target,
            visitor.message
        );
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= self.capacity {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}
