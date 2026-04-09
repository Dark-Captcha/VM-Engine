//! Structured execution trace with filters and bounded memory.

// ============================================================================
// Imports
// ============================================================================

use std::collections::VecDeque;

use crate::ir::{FuncId, Var};
use crate::value::{ObjectId, Value};

use super::state::Cursor;

// ============================================================================
// TraceEvent
// ============================================================================

/// One event emitted during IR execution.
#[derive(Debug, Clone)]
pub enum TraceEvent {
    /// An instruction was executed.
    Step {
        cursor: Cursor,
        op_name: String,
        source_pc: Option<usize>,
    },
    /// A variable was assigned.
    VarWrite { var: Var, value: Value },
    /// A property was read from the heap.
    PropGet { obj: ObjectId, key: String, value: Value },
    /// A property was written to the heap.
    PropSet { obj: ObjectId, key: String, value: Value },
    /// A function call began.
    CallEnter { func: FuncId, arg_count: usize },
    /// A function call returned.
    CallReturn { func: FuncId, result: Value },
    /// Execution halted.
    Halted { instruction_count: u64 },
}

// ============================================================================
// TraceFilter
// ============================================================================

/// Filter configuration for the trace recorder.
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    pub include_steps: bool,
    pub include_var_writes: bool,
    pub include_prop_access: bool,
    pub include_calls: bool,
    /// Only record events for specific functions.
    pub func_filter: Option<Vec<FuncId>>,
    /// Only record var writes for specific variables.
    pub var_filter: Option<Vec<Var>>,
    /// Only record property access for keys matching this predicate.
    pub key_filter: Option<Vec<String>>,
}

impl TraceFilter {
    /// Accept all events.
    pub fn all() -> Self {
        Self {
            include_steps: true,
            include_var_writes: true,
            include_prop_access: true,
            include_calls: true,
            func_filter: None,
            var_filter: None,
            key_filter: None,
        }
    }
}

// ============================================================================
// TraceRecorder
// ============================================================================

/// Records execution events in a bounded ring buffer.
#[derive(Debug, Clone)]
pub struct TraceRecorder {
    enabled: bool,
    capacity: usize,
    filter: TraceFilter,
    events: VecDeque<TraceEvent>,
}

impl TraceRecorder {
    pub fn new() -> Self {
        Self {
            enabled: false,
            capacity: 0,
            filter: TraceFilter::default(),
            events: VecDeque::new(),
        }
    }

    /// Enable tracing with a maximum capacity.
    pub fn enable(&mut self, capacity: usize) {
        self.enabled = true;
        self.capacity = capacity;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_filter(&mut self, filter: TraceFilter) {
        self.filter = filter;
    }

    /// Record an event (if enabled and passes filter).
    pub fn record(&mut self, event: TraceEvent) {
        if !self.enabled {
            return;
        }
        if !self.should_record(&event) {
            return;
        }
        if self.capacity > 0 && self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn events(&self) -> impl Iterator<Item = &TraceEvent> {
        self.events.iter()
    }

    pub fn into_events(self) -> Vec<TraceEvent> {
        self.events.into_iter().collect()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    fn should_record(&self, event: &TraceEvent) -> bool {
        match event {
            TraceEvent::Step { .. } => self.filter.include_steps,
            TraceEvent::VarWrite { var, .. } => {
                self.filter.include_var_writes
                    && self.filter.var_filter.as_ref()
                        .is_none_or(|vf| vf.contains(var))
            }
            TraceEvent::PropGet { key, .. } | TraceEvent::PropSet { key, .. } => {
                self.filter.include_prop_access
                    && self.filter.key_filter.as_ref()
                        .is_none_or(|kf| kf.iter().any(|k| k == key))
            }
            TraceEvent::CallEnter { func, .. } | TraceEvent::CallReturn { func, .. } => {
                self.filter.include_calls
                    && self.filter.func_filter.as_ref()
                        .is_none_or(|ff| ff.contains(func))
            }
            TraceEvent::Halted { .. } => true,
        }
    }
}

impl Default for TraceRecorder {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_records_nothing() {
        let mut tr = TraceRecorder::new();
        tr.record(TraceEvent::Halted { instruction_count: 0 });
        assert!(tr.is_empty());
    }

    #[test]
    fn enabled_records_events() {
        let mut tr = TraceRecorder::new();
        tr.enable(100);
        tr.set_filter(TraceFilter::all());
        tr.record(TraceEvent::Halted { instruction_count: 42 });
        assert_eq!(tr.len(), 1);
    }

    #[test]
    fn capacity_bounded() {
        let mut tr = TraceRecorder::new();
        tr.enable(3);
        tr.set_filter(TraceFilter::all());
        for i in 0..10 {
            tr.record(TraceEvent::Halted { instruction_count: i });
        }
        assert!(tr.len() <= 3);
    }

    #[test]
    fn filter_by_var() {
        let mut tr = TraceRecorder::new();
        tr.enable(100);
        tr.set_filter(TraceFilter {
            include_var_writes: true,
            var_filter: Some(vec![Var(5)]),
            ..Default::default()
        });
        tr.record(TraceEvent::VarWrite { var: Var(5), value: Value::number(1.0) });
        tr.record(TraceEvent::VarWrite { var: Var(10), value: Value::number(2.0) });
        assert_eq!(tr.len(), 1); // only Var(5) passes
    }
}
