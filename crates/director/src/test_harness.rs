//! Interactive AI-driven test harness for emergent behavior validation.
//!
//! This module provides a staged execution framework where an AI agent (Claude)
//! can set up scenarios, run agent loops step-by-step, inspect intermediate states,
//! and validate emergent behavior interactively.
//!
//! Unlike traditional tests with predefined assertions, this harness supports
//! exploratory testing where the AI can make decisions about what to probe next.
//!
//! # Key Concepts
//!
//! - **Staged execution**: Tests progress through numbered stages
//! - **Event logging**: All state changes are recorded for inspection
//! - **Probes**: Query system state without changing it
//! - **Observations**: AI can annotate what it notices
//! - **Scenarios**: Pre-built patterns for common test setups
//!
//! # Example
//!
//! ```ignore
//! let mut harness = TestHarness::new();
//!
//! // Stage 1: Setup
//! harness.create_session("worker", SessionTarget::issue("PAL-42")).await?;
//! harness.set_status("worker", SessionStatus::Running).await?;
//! harness.next_stage();
//!
//! // Stage 2: Verify behavior
//! assert!(!harness.would_create_succeed(&SessionTarget::issue("PAL-42")).await);
//! harness.complete_session("worker").await?;
//!
//! // Stage 3: Verify cleanup
//! assert!(harness.would_create_succeed(&SessionTarget::issue("PAL-42")).await);
//! ```

use crate::{Session, SessionManager, SessionTarget, SessionStrategy, SessionStatus};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// A staged test scenario for interactive exploration.
pub struct TestHarness {
    /// Session manager under test.
    pub manager: SessionManager,
    /// Named sessions for easy reference.
    pub sessions: HashMap<String, Session>,
    /// Event log for inspection.
    pub events: Vec<TestEvent>,
    /// Current stage number.
    pub stage: usize,
}

/// Events that occur during test execution.
#[derive(Debug, Clone)]
pub enum TestEvent {
    /// Session was created.
    SessionCreated { name: String, target: String },
    /// Session status changed.
    StatusChanged { name: String, from: SessionStatus, to: SessionStatus },
    /// Session creation was rejected (overlap).
    SessionRejected { target: String, reason: String },
    /// Session completed successfully.
    SessionCompleted { name: String },
    /// Session failed.
    SessionFailed { name: String, reason: String },
    /// Session was aborted.
    SessionAborted { name: String },
    /// Tool was called.
    ToolCalled { session: String, tool: String, input: String },
    /// Tool returned result.
    ToolResult { session: String, tool: String, success: bool },
    /// Custom observation.
    Observation { message: String },
    /// Stage advanced.
    StageAdvanced { from: usize, to: usize },
    /// Assertion passed.
    AssertionPassed { message: String },
    /// Assertion failed.
    AssertionFailed { message: String },
}

impl TestHarness {
    /// Create a new test harness with a fresh session manager.
    pub fn new() -> Self {
        Self {
            manager: SessionManager::new(PathBuf::from("/tmp/test-harness")),
            sessions: HashMap::new(),
            events: Vec::new(),
            stage: 0,
        }
    }

    /// Set up: Create a session with a friendly name.
    pub async fn create_session(&mut self, name: &str, target: SessionTarget) -> Result<(), String> {
        match self.manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await {
            session => {
                self.events.push(TestEvent::SessionCreated {
                    name: name.to_string(),
                    target: target.to_string(),
                });
                self.sessions.insert(name.to_string(), session);
                Ok(())
            }
        }
    }

    /// Set up: Mark a session as a specific status.
    pub async fn set_status(&mut self, name: &str, status: SessionStatus) -> Result<(), String> {
        let session = self.sessions.get(name).ok_or_else(|| format!("Unknown session: {}", name))?;
        let old_status = session.status;
        self.manager.update_status(session.id, status).await;

        // Update our local copy
        if let Some(s) = self.sessions.get_mut(name) {
            s.status = status;
        }

        self.events.push(TestEvent::StatusChanged {
            name: name.to_string(),
            from: old_status,
            to: status,
        });
        Ok(())
    }

    /// Probe: Check if creating a session for target would succeed.
    pub async fn would_create_succeed(&self, target: &SessionTarget) -> bool {
        !self.manager.has_active_session_for_target(target).await
    }

    /// Probe: List all active sessions.
    pub async fn active_sessions(&self) -> Vec<String> {
        self.manager
            .list_active()
            .await
            .iter()
            .filter_map(|s| {
                self.sessions.iter()
                    .find(|(_, session)| session.id == s.id)
                    .map(|(name, _)| name.clone())
            })
            .collect()
    }

    /// Probe: Check overlap between a target and existing sessions.
    pub async fn check_overlap(&self, target: &SessionTarget) -> Vec<String> {
        let overlapping = self.manager.find_sessions_for_target(target).await;
        overlapping
            .iter()
            .filter_map(|s| {
                self.sessions.iter()
                    .find(|(_, session)| session.id == s.id)
                    .map(|(name, _)| name.clone())
            })
            .collect()
    }

    /// Observe: Add a custom observation to the event log.
    pub fn observe(&mut self, message: &str) {
        self.events.push(TestEvent::Observation {
            message: message.to_string(),
        });
    }

    /// Advance to the next stage.
    pub fn next_stage(&mut self) {
        let from = self.stage;
        self.stage += 1;
        self.events.push(TestEvent::StageAdvanced { from, to: self.stage });
    }

    /// Complete a session successfully.
    pub async fn complete_session(&mut self, name: &str) -> Result<(), String> {
        let session = self.sessions.get(name).ok_or_else(|| format!("Unknown session: {}", name))?;
        self.manager.update_status(session.id, SessionStatus::Completed).await;

        if let Some(s) = self.sessions.get_mut(name) {
            s.status = SessionStatus::Completed;
        }

        self.events.push(TestEvent::SessionCompleted { name: name.to_string() });
        Ok(())
    }

    /// Fail a session with a reason.
    pub async fn fail_session(&mut self, name: &str, reason: &str) -> Result<(), String> {
        let session = self.sessions.get(name).ok_or_else(|| format!("Unknown session: {}", name))?;
        self.manager.update_status(session.id, SessionStatus::Failed).await;

        if let Some(s) = self.sessions.get_mut(name) {
            s.status = SessionStatus::Failed;
        }

        self.events.push(TestEvent::SessionFailed {
            name: name.to_string(),
            reason: reason.to_string()
        });
        Ok(())
    }

    /// Cancel a session.
    pub async fn cancel_session(&mut self, name: &str) -> Result<(), String> {
        let session = self.sessions.get(name).ok_or_else(|| format!("Unknown session: {}", name))?;
        self.manager.update_status(session.id, SessionStatus::Cancelled).await;

        if let Some(s) = self.sessions.get_mut(name) {
            s.status = SessionStatus::Cancelled;
        }

        self.events.push(TestEvent::SessionAborted { name: name.to_string() });
        Ok(())
    }

    /// Get a session by name.
    pub fn get_session(&self, name: &str) -> Option<&Session> {
        self.sessions.get(name)
    }

    /// Get all sessions with a specific status.
    pub fn sessions_with_status(&self, status: SessionStatus) -> Vec<&str> {
        self.sessions
            .iter()
            .filter(|(_, s)| s.status == status)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Count sessions by status.
    pub fn count_by_status(&self) -> StatusCounts {
        let mut counts = StatusCounts::default();
        for session in self.sessions.values() {
            match session.status {
                SessionStatus::Starting => counts.starting += 1,
                SessionStatus::Running => counts.running += 1,
                SessionStatus::Paused => counts.paused += 1,
                SessionStatus::Completed => counts.completed += 1,
                SessionStatus::Failed => counts.failed += 1,
                SessionStatus::Cancelled => counts.cancelled += 1,
            }
        }
        counts
    }

    /// Get events of a specific type.
    pub fn events_of_type<F>(&self, predicate: F) -> Vec<&TestEvent>
    where
        F: Fn(&TestEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(e)).collect()
    }

    /// Check if a specific event occurred.
    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&TestEvent) -> bool,
    {
        self.events.iter().any(predicate)
    }

    /// Clear all events (useful between stages).
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Get a summary of the current state for AI inspection.
    pub async fn summary(&self) -> String {
        let mut lines = vec![
            format!("=== Test Harness Stage {} ===", self.stage),
            format!("Sessions: {}", self.sessions.len()),
        ];

        for (name, session) in &self.sessions {
            lines.push(format!("  - {}: {} ({:?})", name, session.target, session.status));
        }

        let active = self.active_sessions().await;
        lines.push(format!("Active: {:?}", active));

        lines.push(format!("Events ({}):", self.events.len()));
        for event in self.events.iter().rev().take(5) {
            lines.push(format!("  {:?}", event));
        }

        lines.join("\n")
    }

    /// Assert helper for AI-driven validation.
    pub fn assert_true(&mut self, condition: bool, message: &str) -> Result<(), String> {
        if condition {
            self.events.push(TestEvent::AssertionPassed { message: message.to_string() });
            Ok(())
        } else {
            self.events.push(TestEvent::AssertionFailed { message: message.to_string() });
            Err(format!("Assertion failed: {}", message))
        }
    }

    /// Assert two values are equal.
    pub fn assert_eq<T: PartialEq + std::fmt::Debug>(&mut self, left: T, right: T, message: &str) -> Result<(), String> {
        if left == right {
            self.events.push(TestEvent::AssertionPassed { message: message.to_string() });
            Ok(())
        } else {
            let msg = format!("{}: {:?} != {:?}", message, left, right);
            self.events.push(TestEvent::AssertionFailed { message: msg.clone() });
            Err(msg)
        }
    }
}

/// Counts of sessions by status.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub starting: usize,
    pub running: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
}

/// Scenario builders for common test patterns.
pub mod scenarios {
    use super::*;

    /// Create a harness with N sessions for different issues.
    pub async fn concurrent_issues(n: usize) -> TestHarness {
        let mut harness = TestHarness::new();
        for i in 0..n {
            let name = format!("s{}", i);
            let target = SessionTarget::issue(&format!("PAL-{}", 100 + i));
            harness.create_session(&name, target).await.unwrap();
            harness.set_status(&name, SessionStatus::Running).await.unwrap();
        }
        harness
    }

    /// Create a harness with a multi-target session.
    pub async fn multi_target(issues: &[&str]) -> TestHarness {
        let mut harness = TestHarness::new();
        let targets: Vec<_> = issues
            .iter()
            .map(|id| crate::session::SingleTarget::Issue((*id).to_string()))
            .collect();
        let multi = SessionTarget::Multi(targets);
        harness.create_session("multi", multi).await.unwrap();
        harness.set_status("multi", SessionStatus::Running).await.unwrap();
        harness
    }

    /// Create a harness with sessions in various lifecycle states.
    pub async fn mixed_lifecycle() -> TestHarness {
        let mut harness = TestHarness::new();

        // Starting (initial state)
        harness.create_session("starting", SessionTarget::issue("PAL-200")).await.unwrap();

        // Running
        harness.create_session("running", SessionTarget::issue("PAL-201")).await.unwrap();
        harness.set_status("running", SessionStatus::Running).await.unwrap();

        // Completed
        harness.create_session("completed", SessionTarget::issue("PAL-202")).await.unwrap();
        harness.set_status("completed", SessionStatus::Running).await.unwrap();
        harness.complete_session("completed").await.unwrap();

        // Failed
        harness.create_session("failed", SessionTarget::issue("PAL-203")).await.unwrap();
        harness.set_status("failed", SessionStatus::Running).await.unwrap();
        harness.fail_session("failed", "test failure").await.unwrap();

        harness
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::scenarios::*;

    #[tokio::test]
    async fn test_harness_basic_scenario() {
        let mut harness = TestHarness::new();

        // Stage 1: Create a session
        harness.create_session("s1", SessionTarget::issue("PAL-60")).await.unwrap();
        harness.set_status("s1", SessionStatus::Running).await.unwrap();
        harness.next_stage();

        // Stage 2: Verify overlap detection
        let would_succeed = harness.would_create_succeed(&SessionTarget::issue("PAL-60")).await;
        harness.observe(&format!("Creating PAL-60 would succeed: {}", would_succeed));

        assert!(!would_succeed, "Should detect overlap with running session");

        // Stage 3: Different target should work
        let other_target = SessionTarget::issue("PAL-61");
        let other_would_succeed = harness.would_create_succeed(&other_target).await;
        assert!(other_would_succeed, "Different target should not overlap");

        // Print summary for AI inspection
        println!("{}", harness.summary().await);
    }

    #[tokio::test]
    async fn test_harness_multi_overlap() {
        let mut harness = TestHarness::new();

        // Create multi-target session
        let multi = SessionTarget::multi(vec![
            crate::session::SingleTarget::Issue("PAL-1".into()),
            crate::session::SingleTarget::Issue("PAL-2".into()),
        ]);
        harness.create_session("multi", multi).await.unwrap();
        harness.set_status("multi", SessionStatus::Running).await.unwrap();

        // Check that individual targets show overlap
        let overlap_1 = harness.check_overlap(&SessionTarget::issue("PAL-1")).await;
        let overlap_2 = harness.check_overlap(&SessionTarget::issue("PAL-2")).await;
        let overlap_3 = harness.check_overlap(&SessionTarget::issue("PAL-3")).await;

        assert!(overlap_1.contains(&"multi".to_string()));
        assert!(overlap_2.contains(&"multi".to_string()));
        assert!(overlap_3.is_empty());

        println!("{}", harness.summary().await);
    }

    /// Test: Completed sessions free up their target for new sessions.
    #[tokio::test]
    async fn test_completion_frees_target() {
        let mut harness = TestHarness::new();

        // Stage 1: Create and run session
        harness.create_session("worker", SessionTarget::issue("PAL-42")).await.unwrap();
        harness.set_status("worker", SessionStatus::Running).await.unwrap();
        harness.next_stage();

        // Stage 2: Verify target is blocked
        harness.assert_true(
            !harness.would_create_succeed(&SessionTarget::issue("PAL-42")).await,
            "Running session should block target"
        ).unwrap();
        harness.next_stage();

        // Stage 3: Complete the session
        harness.complete_session("worker").await.unwrap();
        harness.next_stage();

        // Stage 4: Verify target is now available
        harness.assert_true(
            harness.would_create_succeed(&SessionTarget::issue("PAL-42")).await,
            "Completed session should free target"
        ).unwrap();

        // Verify we can create a new session for same target
        harness.create_session("worker2", SessionTarget::issue("PAL-42")).await.unwrap();
        harness.assert_eq(harness.sessions.len(), 2, "Should have two sessions").unwrap();

        println!("{}", harness.summary().await);
    }

    /// Test: Failed sessions also free up their target.
    #[tokio::test]
    async fn test_failure_frees_target() {
        let mut harness = TestHarness::new();

        harness.create_session("worker", SessionTarget::issue("PAL-43")).await.unwrap();
        harness.set_status("worker", SessionStatus::Running).await.unwrap();

        // Fail the session
        harness.fail_session("worker", "simulated failure").await.unwrap();

        // Target should be free
        harness.assert_true(
            harness.would_create_succeed(&SessionTarget::issue("PAL-43")).await,
            "Failed session should free target"
        ).unwrap();

        // Can retry
        harness.create_session("retry", SessionTarget::issue("PAL-43")).await.unwrap();
        harness.assert_eq(harness.sessions.len(), 2, "Should have original + retry").unwrap();
    }

    /// Test: Cancelled sessions free up their target.
    #[tokio::test]
    async fn test_cancel_frees_target() {
        let mut harness = TestHarness::new();

        harness.create_session("worker", SessionTarget::issue("PAL-44")).await.unwrap();
        harness.set_status("worker", SessionStatus::Running).await.unwrap();

        harness.cancel_session("worker").await.unwrap();

        harness.assert_true(
            harness.would_create_succeed(&SessionTarget::issue("PAL-44")).await,
            "Cancelled session should free target"
        ).unwrap();
    }

    /// Test: Concurrent non-overlapping sessions work independently.
    #[tokio::test]
    async fn test_concurrent_sessions() {
        let mut harness = concurrent_issues(5).await;

        // All 5 sessions should be running
        let active = harness.active_sessions().await;
        assert_eq!(active.len(), 5, "Should have 5 active sessions");

        // Complete some, fail others
        harness.complete_session("s0").await.unwrap();
        harness.complete_session("s1").await.unwrap();
        harness.fail_session("s2", "test failure").await.unwrap();

        // Check counts
        let counts = harness.count_by_status();
        assert_eq!(counts.completed, 2);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.running, 2);

        // Active should now be 2
        let active = harness.active_sessions().await;
        assert_eq!(active.len(), 2, "Should have 2 active sessions after completions");
    }

    /// Test: Multi-target session blocks all its targets.
    #[tokio::test]
    async fn test_multi_target_blocks_all() {
        let harness = multi_target(&["PAL-10", "PAL-11", "PAL-12"]).await;

        // All individual targets should be blocked
        for id in ["PAL-10", "PAL-11", "PAL-12"] {
            assert!(
                !harness.would_create_succeed(&SessionTarget::issue(id)).await,
                "Multi-target session should block {}", id
            );
        }

        // Unrelated target should be free
        assert!(
            harness.would_create_succeed(&SessionTarget::issue("PAL-99")).await,
            "Unrelated target should be free"
        );
    }

    /// Test: Mixed lifecycle scenario.
    #[tokio::test]
    async fn test_mixed_lifecycle() {
        let harness = mixed_lifecycle().await;

        // Check session states
        assert_eq!(harness.sessions_with_status(SessionStatus::Starting).len(), 1);
        assert_eq!(harness.sessions_with_status(SessionStatus::Running).len(), 1);
        assert_eq!(harness.sessions_with_status(SessionStatus::Completed).len(), 1);
        assert_eq!(harness.sessions_with_status(SessionStatus::Failed).len(), 1);

        // Only running and starting should block
        assert!(
            !harness.would_create_succeed(&SessionTarget::issue("PAL-201")).await,
            "Running should block"
        );

        // Completed and failed should not block
        assert!(
            harness.would_create_succeed(&SessionTarget::issue("PAL-202")).await,
            "Completed should not block"
        );
        assert!(
            harness.would_create_succeed(&SessionTarget::issue("PAL-203")).await,
            "Failed should not block"
        );

        println!("{}", harness.summary().await);
    }

    /// Test: Event logging captures full history.
    #[tokio::test]
    async fn test_event_logging() {
        let mut harness = TestHarness::new();

        harness.create_session("test", SessionTarget::issue("PAL-50")).await.unwrap();
        harness.set_status("test", SessionStatus::Running).await.unwrap();
        harness.next_stage();
        harness.observe("Testing event logging");
        harness.complete_session("test").await.unwrap();

        // Check event types were logged
        assert!(harness.has_event(|e| matches!(e, TestEvent::SessionCreated { .. })));
        assert!(harness.has_event(|e| matches!(e, TestEvent::StatusChanged { .. })));
        assert!(harness.has_event(|e| matches!(e, TestEvent::StageAdvanced { .. })));
        assert!(harness.has_event(|e| matches!(e, TestEvent::Observation { .. })));
        assert!(harness.has_event(|e| matches!(e, TestEvent::SessionCompleted { .. })));

        // Count specific events
        let status_changes = harness.events_of_type(|e| matches!(e, TestEvent::StatusChanged { .. }));
        assert_eq!(status_changes.len(), 1, "Should have 1 status change (starting->running)");
    }

    /// Test: Starting sessions also block their target.
    #[tokio::test]
    async fn test_starting_blocks_target() {
        let mut harness = TestHarness::new();

        // Create but don't set to running
        harness.create_session("starting", SessionTarget::issue("PAL-55")).await.unwrap();
        // Status is Starting by default

        // Should still block (prevents race conditions)
        assert!(
            !harness.would_create_succeed(&SessionTarget::issue("PAL-55")).await,
            "Starting session should block target"
        );
    }
}
