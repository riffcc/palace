//! Interactive AI-driven test harness for emergent behavior validation.
//!
//! This module provides a staged execution framework where an AI agent (Claude)
//! can set up scenarios, run agent loops step-by-step, inspect intermediate states,
//! and validate emergent behavior interactively.
//!
//! Unlike traditional tests with predefined assertions, this harness supports
//! exploratory testing where the AI can make decisions about what to probe next.

use crate::{Session, SessionManager, SessionTarget, SessionStrategy, SessionStatus};
use std::collections::HashMap;
use std::path::PathBuf;

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
    /// Tool was called.
    ToolCalled { session: String, tool: String, input: String },
    /// Tool returned result.
    ToolResult { session: String, tool: String, success: bool },
    /// Custom observation.
    Observation { message: String },
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
        self.stage += 1;
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
    pub fn assert_true(&self, condition: bool, message: &str) -> Result<(), String> {
        if condition {
            Ok(())
        } else {
            Err(format!("Assertion failed: {}", message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
