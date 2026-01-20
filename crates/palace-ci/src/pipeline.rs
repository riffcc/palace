//! Pipeline orchestration with Dagger integration.

use crate::config::ProjectConfig;
use crate::error::{CIError, CIResult};
use crate::levels::{CILevel, CIStep};
use crate::rust::RustPipeline;
use crate::scenarios::{ScenarioResult, ScenarioRunner};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Result of running a single CI step.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// The step that was run.
    pub step: CIStep,

    /// Whether the step passed.
    pub passed: bool,

    /// Duration of the step.
    pub duration: Duration,

    /// Output from the step (stdout).
    pub stdout: String,

    /// Error output from the step (stderr).
    pub stderr: String,

    /// Error message if failed.
    pub error: Option<String>,
}

impl StepResult {
    /// Create a successful step result.
    pub fn success(step: CIStep, duration: Duration, stdout: String, stderr: String) -> Self {
        Self {
            step,
            passed: true,
            duration,
            stdout,
            stderr,
            error: None,
        }
    }

    /// Create a failed step result.
    pub fn failure(
        step: CIStep,
        duration: Duration,
        stdout: String,
        stderr: String,
        error: String,
    ) -> Self {
        Self {
            step,
            passed: false,
            duration,
            stdout,
            stderr,
            error: Some(error),
        }
    }
}

/// Result of running an entire pipeline.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Project name.
    pub project: String,

    /// CI level that was run.
    pub level: CILevel,

    /// Whether all steps passed.
    pub passed: bool,

    /// Results for each step.
    pub steps: Vec<StepResult>,

    /// Scenario results (if scenarios were run).
    pub scenarios: Vec<ScenarioResult>,

    /// Total duration.
    pub total_duration: Duration,
}

impl PipelineResult {
    /// Get the number of passed steps.
    pub fn passed_count(&self) -> usize {
        self.steps.iter().filter(|s| s.passed).count()
    }

    /// Get the number of failed steps.
    pub fn failed_count(&self) -> usize {
        self.steps.iter().filter(|s| !s.passed).count()
    }

    /// Get the first failed step, if any.
    pub fn first_failure(&self) -> Option<&StepResult> {
        self.steps.iter().find(|s| !s.passed)
    }

    /// Format a summary of the pipeline result.
    pub fn summary(&self) -> String {
        let status = if self.passed { "PASSED" } else { "FAILED" };
        let steps = format!("{}/{} steps", self.passed_count(), self.steps.len());
        let duration = format!("{:.2}s", self.total_duration.as_secs_f64());

        format!(
            "[{}] {} - {} ({}) - {}",
            self.level.to_string(),
            self.project,
            status,
            steps,
            duration
        )
    }
}

impl std::fmt::Display for CILevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CILevel::Simple => write!(f, "simple"),
            CILevel::Lint => write!(f, "lint"),
            CILevel::Basic => write!(f, "basic"),
            CILevel::BasicLong => write!(f, "basic-long"),
            CILevel::Run => write!(f, "run"),
            CILevel::RunProd => write!(f, "run-prod"),
            CILevel::Scenarios => write!(f, "scenarios"),
        }
    }
}

/// A CI pipeline that runs steps according to the configured level.
pub struct Pipeline {
    config: ProjectConfig,
}

impl Pipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: ProjectConfig) -> Self {
        Self { config }
    }

    /// Run the pipeline using Dagger.
    pub async fn run(&self) -> CIResult<PipelineResult> {
        let start = Instant::now();
        let steps_to_run = self.config.level.steps();

        tracing::info!(
            "Starting CI pipeline for '{}' at level '{}'",
            self.config.name,
            self.config.level
        );

        // Shared state for collecting results
        let step_results = Arc::new(Mutex::new(Vec::new()));
        let all_passed = Arc::new(Mutex::new(true));
        let config = self.config.clone();
        let results_clone = step_results.clone();
        let passed_clone = all_passed.clone();

        // Run pipeline inside Dagger connection
        let dagger_result = dagger_sdk::connect(move |client| async move {
            let rust_pipeline = RustPipeline::new(&config);

            for step in &steps_to_run {
                let step_start = Instant::now();

                // Skip if previous step failed (except scenarios)
                {
                    let passed = passed_clone.lock().await;
                    if !*passed && *step != CIStep::Scenarios {
                        let mut results = results_clone.lock().await;
                        results.push(StepResult {
                            step: *step,
                            passed: false,
                            duration: Duration::ZERO,
                            stdout: String::new(),
                            stderr: String::new(),
                            error: Some("Skipped due to previous failure".into()),
                        });
                        continue;
                    }
                }

                tracing::info!("Running step: {}", step);

                let result = rust_pipeline.run_step(&client, *step).await;

                match result {
                    Ok((stdout, stderr)) => {
                        let mut results = results_clone.lock().await;
                        results.push(StepResult::success(*step, step_start.elapsed(), stdout, stderr));
                        tracing::info!("Step {} passed", step);
                    }
                    Err(e) => {
                        let mut passed = passed_clone.lock().await;
                        *passed = false;

                        let mut results = results_clone.lock().await;
                        results.push(StepResult::failure(
                            *step,
                            step_start.elapsed(),
                            String::new(),
                            String::new(),
                            e.to_string(),
                        ));
                        tracing::error!("Step {} failed: {}", step, e);
                    }
                }
            }

            Ok(())
        })
        .await;

        // Handle Dagger connection errors
        if let Err(e) = dagger_result {
            return Err(CIError::DaggerConnection(e.to_string()));
        }

        // Extract results
        let final_steps = step_results.lock().await.clone();
        let final_passed = *all_passed.lock().await;

        // Run scenarios if configured (outside of Dagger)
        let mut scenario_results = vec![];
        let mut scenarios_passed = true;
        if self.config.level.scenarios() && !self.config.scenarios.is_empty() {
            let mut runner = ScenarioRunner::new();
            runner.add_scenarios(self.config.scenarios.clone());

            match runner.run_all().await {
                Ok(results) => {
                    for result in &results {
                        if !result.passed {
                            scenarios_passed = false;
                        }
                    }
                    scenario_results = results;
                }
                Err(e) => {
                    scenarios_passed = false;
                    tracing::error!("Scenario execution failed: {}", e);
                }
            }
        }

        Ok(PipelineResult {
            project: self.config.name.clone(),
            level: self.config.level,
            passed: final_passed && scenarios_passed,
            steps: final_steps,
            scenarios: scenario_results,
            total_duration: start.elapsed(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_result() {
        let result = StepResult::success(
            CIStep::Compile,
            Duration::from_secs(5),
            "Build succeeded".into(),
            String::new(),
        );

        assert!(result.passed);
        assert_eq!(result.step, CIStep::Compile);
    }

    #[test]
    fn test_pipeline_result_summary() {
        let result = PipelineResult {
            project: "test-project".into(),
            level: CILevel::Basic,
            passed: true,
            steps: vec![
                StepResult::success(CIStep::Compile, Duration::from_secs(10), String::new(), String::new()),
                StepResult::success(CIStep::Lint, Duration::from_secs(5), String::new(), String::new()),
                StepResult::success(CIStep::Test, Duration::from_secs(30), String::new(), String::new()),
            ],
            scenarios: vec![],
            total_duration: Duration::from_secs(45),
        };

        let summary = result.summary();
        assert!(summary.contains("PASSED"));
        assert!(summary.contains("3/3"));
        assert!(summary.contains("basic"));
    }
}
