//! Program state capture and streaming.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A snapshot of program state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Timestamp when the state was captured.
    pub timestamp_ms: u64,

    /// Process ID.
    pub pid: Option<u32>,

    /// Current execution point (function/line if available).
    pub execution_point: Option<String>,

    /// Standard output since last snapshot.
    pub stdout: String,

    /// Standard error since last snapshot.
    pub stderr: String,

    /// Custom state variables.
    pub variables: HashMap<String, serde_json::Value>,

    /// Recent events/actions.
    pub events: Vec<StateEvent>,

    /// Memory usage in bytes.
    pub memory_bytes: Option<u64>,

    /// CPU usage percentage.
    pub cpu_percent: Option<f32>,
}

impl StateSnapshot {
    /// Create a new empty state snapshot.
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            timestamp_ms: timestamp,
            pid: None,
            execution_point: None,
            stdout: String::new(),
            stderr: String::new(),
            variables: HashMap::new(),
            events: vec![],
            memory_bytes: None,
            cpu_percent: None,
        }
    }

    /// Set the process ID.
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Set the execution point.
    pub fn with_execution_point(mut self, point: impl Into<String>) -> Self {
        self.execution_point = Some(point.into());
        self
    }

    /// Add stdout output.
    pub fn with_stdout(mut self, output: impl Into<String>) -> Self {
        self.stdout = output.into();
        self
    }

    /// Add stderr output.
    pub fn with_stderr(mut self, output: impl Into<String>) -> Self {
        self.stderr = output.into();
        self
    }

    /// Set a variable.
    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    /// Add an event.
    pub fn with_event(mut self, event: StateEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Get age of this snapshot.
    pub fn age(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.timestamp_ms))
    }
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// An event that occurred in the program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEvent {
    /// Event type.
    pub event_type: String,

    /// Event data.
    pub data: serde_json::Value,

    /// Timestamp offset from snapshot (ms).
    pub offset_ms: u64,
}

impl StateEvent {
    /// Create a new event.
    pub fn new(event_type: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            event_type: event_type.into(),
            data,
            offset_ms: 0,
        }
    }

    /// Set the timestamp offset.
    pub fn with_offset(mut self, offset_ms: u64) -> Self {
        self.offset_ms = offset_ms;
        self
    }
}

/// Continuous stream of program state.
pub struct StateStream {
    snapshots: Vec<StateSnapshot>,
    max_history: usize,
}

impl StateStream {
    /// Create a new state stream.
    pub fn new(max_history: usize) -> Self {
        Self {
            snapshots: vec![],
            max_history,
        }
    }

    /// Push a new snapshot.
    pub fn push(&mut self, snapshot: StateSnapshot) {
        self.snapshots.push(snapshot);

        // Trim old snapshots
        while self.snapshots.len() > self.max_history {
            self.snapshots.remove(0);
        }
    }

    /// Get the latest snapshot.
    pub fn latest(&self) -> Option<&StateSnapshot> {
        self.snapshots.last()
    }

    /// Get snapshot at a specific delay from now.
    pub fn at_delay(&self, delay: Duration) -> Option<&StateSnapshot> {
        let target_age = delay;

        // Find snapshot closest to the target age
        self.snapshots
            .iter()
            .rev()
            .find(|s| s.age() >= target_age)
    }

    /// Get all snapshots in a time range.
    pub fn range(&self, start_age: Duration, end_age: Duration) -> Vec<&StateSnapshot> {
        self.snapshots
            .iter()
            .filter(|s| {
                let age = s.age();
                age >= end_age && age <= start_age
            })
            .collect()
    }

    /// Get snapshot count.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Clear all snapshots.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for StateStream {
    fn default() -> Self {
        Self::new(1000) // Keep last 1000 snapshots by default
    }
}

/// Captures state from a running program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramState {
    /// Program name.
    pub name: String,

    /// Working directory.
    pub cwd: Option<String>,

    /// Environment variables.
    pub env: HashMap<String, String>,

    /// Command line arguments.
    pub args: Vec<String>,

    /// Whether the program is running.
    pub running: bool,

    /// Exit code if terminated.
    pub exit_code: Option<i32>,
}

impl ProgramState {
    /// Create a new program state.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cwd: None,
            env: HashMap::new(),
            args: vec![],
            running: false,
            exit_code: None,
        }
    }

    /// Add metadata key-value pair.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_snapshot() {
        let snapshot = StateSnapshot::new()
            .with_pid(1234)
            .with_stdout("Hello")
            .with_variable("counter", serde_json::json!(42));

        assert_eq!(snapshot.pid, Some(1234));
        assert_eq!(snapshot.stdout, "Hello");
        assert_eq!(snapshot.variables.get("counter"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_state_stream() {
        let mut stream = StateStream::new(3);

        stream.push(StateSnapshot::new().with_stdout("1"));
        stream.push(StateSnapshot::new().with_stdout("2"));
        stream.push(StateSnapshot::new().with_stdout("3"));
        stream.push(StateSnapshot::new().with_stdout("4"));

        // Should only keep last 3
        assert_eq!(stream.len(), 3);
        assert_eq!(stream.latest().unwrap().stdout, "4");
    }
}
