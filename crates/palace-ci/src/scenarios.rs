//! Scriptable test scenarios for end-to-end automation.
//!
//! This module provides the foundation for "Mountain" - a system that can
//! script and automate any program to verify behavior end-to-end.

use crate::error::{CIError, CIResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// A test scenario that scripts program behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario name.
    pub name: String,

    /// Description of what this scenario tests.
    pub description: Option<String>,

    /// Steps to execute in order.
    pub steps: Vec<ScenarioStep>,

    /// Timeout for the entire scenario.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Environment variables for this scenario.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Tags for filtering scenarios.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_timeout() -> u64 {
    300 // 5 minutes default
}

impl Scenario {
    /// Create a new scenario.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            steps: vec![],
            timeout_secs: default_timeout(),
            env: HashMap::new(),
            tags: vec![],
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a step.
    pub fn with_step(mut self, step: ScenarioStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Add multiple steps.
    pub fn with_steps(mut self, steps: Vec<ScenarioStep>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// Set timeout.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Individual step in a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScenarioStep {
    /// Wait for a duration.
    Wait { duration_ms: u64 },

    /// Send input to stdin.
    SendInput { text: String },

    /// Send a line to stdin (with newline).
    SendLine { text: String },

    /// Expect output to contain text.
    ExpectOutput { contains: String, timeout_ms: u64 },

    /// Expect output to match regex.
    ExpectRegex { pattern: String, timeout_ms: u64 },

    /// Expect the program to exit.
    ExpectExit { code: Option<i32>, timeout_ms: u64 },

    /// Send a signal to the process.
    SendSignal { signal: String },

    /// HTTP request (for web services).
    HttpRequest {
        method: String,
        url: String,
        body: Option<String>,
        headers: HashMap<String, String>,
        expect_status: u16,
    },

    /// Browser action (uses playwright via E2E module).
    BrowserAction {
        action: String, // navigate, click, fill, etc.
        selector: Option<String>,
        value: Option<String>,
    },

    /// Assert a condition.
    Assert {
        condition: String,
        message: Option<String>,
    },

    /// Run a shell command.
    Shell { command: String, expect_success: bool },

    /// Custom step (for extensibility).
    Custom {
        handler: String,
        params: serde_json::Value,
    },
}

impl ScenarioStep {
    /// Create a wait step.
    pub fn wait(duration: Duration) -> Self {
        Self::Wait {
            duration_ms: duration.as_millis() as u64,
        }
    }

    /// Create a send input step.
    pub fn send_input(text: impl Into<String>) -> Self {
        Self::SendInput { text: text.into() }
    }

    /// Create a send line step.
    pub fn send_line(text: impl Into<String>) -> Self {
        Self::SendLine { text: text.into() }
    }

    /// Create an expect output step.
    pub fn expect_output(contains: impl Into<String>, timeout: Duration) -> Self {
        Self::ExpectOutput {
            contains: contains.into(),
            timeout_ms: timeout.as_millis() as u64,
        }
    }

    /// Create an expect exit step.
    pub fn expect_exit(code: Option<i32>, timeout: Duration) -> Self {
        Self::ExpectExit {
            code,
            timeout_ms: timeout.as_millis() as u64,
        }
    }

    /// Create an HTTP request step.
    pub fn http_get(url: impl Into<String>, expect_status: u16) -> Self {
        Self::HttpRequest {
            method: "GET".into(),
            url: url.into(),
            body: None,
            headers: HashMap::new(),
            expect_status,
        }
    }

    /// Create a browser navigation step.
    pub fn browser_navigate(url: impl Into<String>) -> Self {
        Self::BrowserAction {
            action: "navigate".into(),
            selector: None,
            value: Some(url.into()),
        }
    }

    /// Create a browser click step.
    pub fn browser_click(selector: impl Into<String>) -> Self {
        Self::BrowserAction {
            action: "click".into(),
            selector: Some(selector.into()),
            value: None,
        }
    }

    /// Create a shell command step.
    pub fn shell(command: impl Into<String>) -> Self {
        Self::Shell {
            command: command.into(),
            expect_success: true,
        }
    }
}

/// Result of running a scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Scenario name.
    pub name: String,

    /// Whether the scenario passed.
    pub passed: bool,

    /// Results for each step.
    pub step_results: Vec<StepRunResult>,

    /// Total duration.
    pub duration: Duration,

    /// Error message if failed.
    pub error: Option<String>,
}

/// Result of running a single step.
#[derive(Debug, Clone)]
pub struct StepRunResult {
    /// Step index.
    pub index: usize,

    /// Whether the step passed.
    pub passed: bool,

    /// Step duration.
    pub duration: Duration,

    /// Captured output (if any).
    pub output: Option<String>,

    /// Error message if failed.
    pub error: Option<String>,
}

/// Runner for executing scenarios.
pub struct ScenarioRunner {
    scenarios: Vec<Scenario>,
}

impl ScenarioRunner {
    /// Create a new scenario runner.
    pub fn new() -> Self {
        Self { scenarios: vec![] }
    }

    /// Add a scenario.
    pub fn add_scenario(&mut self, scenario: Scenario) {
        self.scenarios.push(scenario);
    }

    /// Add multiple scenarios.
    pub fn add_scenarios(&mut self, scenarios: Vec<Scenario>) {
        self.scenarios.extend(scenarios);
    }

    /// Get scenarios by tag.
    pub fn scenarios_with_tag(&self, tag: &str) -> Vec<&Scenario> {
        self.scenarios
            .iter()
            .filter(|s| s.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Run all scenarios.
    pub async fn run_all(&self) -> CIResult<Vec<ScenarioResult>> {
        let mut results = vec![];

        for scenario in &self.scenarios {
            let result = self.run_scenario(scenario).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Run a single scenario.
    pub async fn run_scenario(&self, scenario: &Scenario) -> CIResult<ScenarioResult> {
        let start = std::time::Instant::now();
        let mut step_results = vec![];
        let mut passed = true;
        let mut error = None;

        for (index, step) in scenario.steps.iter().enumerate() {
            let step_start = std::time::Instant::now();

            match self.run_step(step).await {
                Ok(output) => {
                    step_results.push(StepRunResult {
                        index,
                        passed: true,
                        duration: step_start.elapsed(),
                        output,
                        error: None,
                    });
                }
                Err(e) => {
                    passed = false;
                    error = Some(e.to_string());
                    step_results.push(StepRunResult {
                        index,
                        passed: false,
                        duration: step_start.elapsed(),
                        output: None,
                        error: Some(e.to_string()),
                    });
                    break;
                }
            }
        }

        Ok(ScenarioResult {
            name: scenario.name.clone(),
            passed,
            step_results,
            duration: start.elapsed(),
            error,
        })
    }

    /// Run a single step (stub implementation - real impl in Mountain crate).
    async fn run_step(&self, step: &ScenarioStep) -> CIResult<Option<String>> {
        match step {
            ScenarioStep::Wait { duration_ms } => {
                tokio::time::sleep(Duration::from_millis(*duration_ms)).await;
                Ok(None)
            }
            ScenarioStep::Shell { command, expect_success } => {
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .await?;

                if *expect_success && !output.status.success() {
                    return Err(CIError::ScenarioFailed(
                        "shell".into(),
                        format!("Command failed: {}", command),
                    ));
                }

                Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
            }
            // Other steps will be implemented in the Mountain crate
            _ => {
                tracing::warn!("Step type not yet implemented: {:?}", step);
                Ok(None)
            }
        }
    }
}

impl Default for ScenarioRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_builder() {
        let scenario = Scenario::new("test-login")
            .with_description("Test user login flow")
            .with_step(ScenarioStep::browser_navigate("http://localhost:3000"))
            .with_step(ScenarioStep::browser_click("#login-button"))
            .with_tags(vec!["auth".into(), "smoke".into()]);

        assert_eq!(scenario.name, "test-login");
        assert_eq!(scenario.steps.len(), 2);
        assert!(scenario.tags.contains(&"auth".to_string()));
    }

    #[test]
    fn test_step_serialization() {
        let step = ScenarioStep::http_get("http://localhost/health", 200);
        let json = serde_json::to_string(&step).unwrap();
        assert!(json.contains("http_request"));
        assert!(json.contains("GET"));
    }
}
