//! SWE-bench benchmark harness for Palace.
//!
//! This module provides integration with the SWE-bench benchmark suite,
//! allowing Palace to be evaluated against real-world GitHub issues.
//!
//! # Usage
//!
//! ```bash
//! pal bench --limit 10 --model "GLM-4.7" --timeout 120
//! ```

use crate::{Executor, ExecutorConfig};
use futures::{stream, StreamExt};
use llm_code_sdk::tools::ToolEvent;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A single SWE-bench instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SWEBenchInstance {
    pub instance_id: String,
    pub repo: String,
    pub base_commit: String,
    pub problem_statement: String,
    #[serde(default)]
    pub hints_text: String,
    #[serde(default)]
    pub patch: String,
    #[serde(default)]
    pub test_patch: String,
    #[serde(rename = "FAIL_TO_PASS", default)]
    pub fail_to_pass: String,
    #[serde(rename = "PASS_TO_PASS", default)]
    pub pass_to_pass: String,
    #[serde(default)]
    pub environment_setup_commit: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub created_at: String,
}

impl SWEBenchInstance {
    /// Get the list of tests that should change from failing to passing.
    pub fn failing_tests(&self) -> Vec<&str> {
        if self.fail_to_pass.is_empty() {
            return vec![];
        }
        // Parse JSON array format: ["test1", "test2"]
        serde_json::from_str::<Vec<String>>(&self.fail_to_pass)
            .map(|v| v.into_iter().map(|_| "").collect())
            .unwrap_or_else(|_| {
                // Fallback: split by newlines
                self.fail_to_pass.lines().collect()
            })
    }
}

/// Dataset variant to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatasetVariant {
    /// Full dataset (~2294 instances)
    Full,
    /// Lite dataset (300 instances, curated)
    #[default]
    Lite,
    /// Verified dataset (500 human-validated instances)
    Verified,
}

impl DatasetVariant {
    /// The split name in HuggingFace (all variants use "test" split)
    pub fn as_str(&self) -> &'static str {
        // All SWE-bench datasets use "test" as the split name
        "test"
    }

    pub fn dataset_name(&self) -> &'static str {
        match self {
            DatasetVariant::Full => "princeton-nlp/SWE-bench",
            DatasetVariant::Lite => "princeton-nlp/SWE-bench_Lite",
            DatasetVariant::Verified => "princeton-nlp/SWE-bench_Verified",
        }
    }
}

/// Loads SWE-bench instances from various sources.
pub struct SWEBenchLoader;

impl SWEBenchLoader {
    /// Load instances from a local JSONL file.
    pub fn from_file(path: PathBuf) -> Result<Vec<SWEBenchInstance>, String> {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        let mut instances = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let instance: SWEBenchInstance = serde_json::from_str(line)
                .map_err(|e| format!("Failed to parse line {}: {}", i + 1, e))?;
            instances.push(instance);
        }

        Ok(instances)
    }

    /// Load instances from HuggingFace datasets.
    pub fn from_huggingface(variant: DatasetVariant, limit: Option<usize>) -> Result<Vec<SWEBenchInstance>, String> {
        // Find Python with datasets installed
        let python_paths = [
            "/tmp/swebench-env/bin/python3",  // Our venv
            "python3",                         // System Python
            "python",                          // Fallback
        ];

        let python = python_paths.iter()
            .find(|p| {
                std::process::Command::new(p)
                    .args(["-c", "import datasets"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                "Python with 'datasets' package not found. Install with:\n\
                 python3 -m venv /tmp/swebench-env && \
                 /tmp/swebench-env/bin/pip install datasets".to_string()
            })?;

        let limit_arg = limit.map(|n| format!("[:{}]", n)).unwrap_or_default();
        let script = format!(r#"
import json
from datasets import load_dataset
ds = load_dataset("{}", split="{}{}")
for item in ds:
    print(json.dumps(dict(item)))
"#, variant.dataset_name(), variant.as_str(), limit_arg);

        let output = std::process::Command::new(python)
            .args(["-c", &script])
            .output()
            .map_err(|e| format!("Failed to run Python: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Python script failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut instances = Vec::new();
        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let instance: SWEBenchInstance = serde_json::from_str(line)
                .map_err(|e| format!("Failed to parse instance: {}", e))?;
            instances.push(instance);
        }

        Ok(instances)
    }
}

/// Configuration for the benchmark runner.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    /// LLM endpoint URL
    pub llm_url: String,
    /// API key (for Z.ai/Anthropic)
    pub api_key: Option<String>,
    /// Model name
    pub model: String,
    /// Timeout per instance
    pub timeout: Duration,
    /// Max tokens per request
    pub max_tokens: u32,
    /// Working directory for cloned repos
    pub work_dir: PathBuf,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            // Default to Z.ai API - use --llm-url for LM Studio
            llm_url: "https://api.z.ai/v1".to_string(),
            api_key: std::env::var("ZAI_API_KEY").ok(),
            model: "GLM-4.7".to_string(),
            timeout: Duration::from_secs(900),
            max_tokens: 65536,
            work_dir: PathBuf::from("/tmp/swebench-palace"),
        }
    }
}

/// A single event in the execution trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Timestamp (ms from start)
    pub time_ms: u64,
    /// Event type: "text", "tool_call", "tool_result"
    pub event_type: String,
    /// Tool name (for tool events)
    pub tool: Option<String>,
    /// Input/output content
    pub content: String,
    /// Success flag (for tool results)
    pub success: Option<bool>,
}

/// Result of running a single instance.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub instance_id: String,
    pub success: bool,
    pub patch: Option<String>,
    pub duration: Duration,
    pub error: Option<String>,
    /// Full execution trace
    pub trace: Vec<TraceEvent>,
}

/// Prediction in SWE-bench format.
#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub instance_id: String,
    pub model_name_or_path: String,
    pub model_patch: String,
}

/// Summary of a benchmark run.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub total_duration: Duration,
    pub avg_duration: Duration,
}

/// Runs SWE-bench instances using Palace's executor.
pub struct SWEBenchRunner {
    config: BenchmarkConfig,
    results: Vec<RunResult>,
}

impl SWEBenchRunner {
    pub fn new(config: BenchmarkConfig) -> Self {
        // Ensure work directory exists
        std::fs::create_dir_all(&config.work_dir).ok();

        Self {
            config,
            results: Vec::new(),
        }
    }

    /// Run a batch of instances.
    pub async fn run_batch(&mut self, instances: &[SWEBenchInstance]) -> RunSummary {
        tracing::info!("Running SWE-bench batch: {} instances", instances.len());

        let start = Instant::now();

        for (i, instance) in instances.iter().enumerate() {
            tracing::info!("[{}/{}] {}", i + 1, instances.len(), instance.instance_id);

            let result = self.run_instance(instance).await;

            let status = if result.success { "✓" } else { "✗" };
            tracing::info!("  {} ({:.1}s)", status, result.duration.as_secs_f32());

            self.results.push(result);
        }

        let total_duration = start.elapsed();
        let success = self.results.iter().filter(|r| r.success).count();
        let failed = self.results.len() - success;

        RunSummary {
            total: self.results.len(),
            success,
            failed,
            total_duration,
            avg_duration: total_duration / self.results.len().max(1) as u32,
        }
    }

    /// Run batch in parallel with controlled concurrency.
    pub async fn run_batch_parallel(&mut self, instances: &[SWEBenchInstance], concurrency: usize) -> RunSummary {
        tracing::info!("Running SWE-bench batch: {} instances with {} parallel workers", instances.len(), concurrency);

        let start = Instant::now();
        let config = self.config.clone();

        // Create tasks for each instance
        let results: Vec<RunResult> = stream::iter(instances.iter().enumerate())
            .map(|(i, instance)| {
                let config = config.clone();
                let instance = instance.clone();
                let total = instances.len();
                async move {
                    tracing::info!("[{}/{}] Starting {}", i + 1, total, instance.instance_id);
                    let result = Self::run_instance_standalone(&config, &instance).await;
                    let status = if result.success { "✓" } else { "✗" };
                    tracing::info!("[{}/{}] {} {} ({:.1}s)", i + 1, total, status, instance.instance_id, result.duration.as_secs_f32());
                    result
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        self.results.extend(results);

        let total_duration = start.elapsed();
        let success = self.results.iter().filter(|r| r.success).count();
        let failed = self.results.len() - success;

        RunSummary {
            total: self.results.len(),
            success,
            failed,
            total_duration,
            avg_duration: total_duration / self.results.len().max(1) as u32,
        }
    }

    /// Run a single instance standalone (for parallel execution).
    async fn run_instance_standalone(config: &BenchmarkConfig, instance: &SWEBenchInstance) -> RunResult {
        let start = Instant::now();

        // Prepare the repository
        let repo_dir = match Self::prepare_repo_standalone(config, instance).await {
            Ok(dir) => dir,
            Err(e) => {
                return RunResult {
                    instance_id: instance.instance_id.clone(),
                    success: false,
                    patch: None,
                    duration: start.elapsed(),
                    error: Some(e),
                    trace: vec![],
                };
            }
        };

        tracing::info!("Running instance {} in {:?}", instance.instance_id, repo_dir);

        // Executor config (created fresh for each retry)
        let executor_config = ExecutorConfig {
            project_path: repo_dir.clone(),
            llm_url: config.llm_url.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            dry_run: false,
            stock_tools: false,
        };

        // Create trace collector
        let trace: Arc<Mutex<Vec<TraceEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let trace_clone = trace.clone();
        let start_time = start.clone();
        let instance_id = instance.instance_id.clone();

        // Create callback
        let callback = Arc::new(move |event: ToolEvent| {
            let time_ms = start_time.elapsed().as_millis() as u64;
            let trace_event = match event {
                ToolEvent::ToolCall { name, input } => {
                    let input_str = serde_json::to_string(&input).unwrap_or_default();
                    let preview = if input_str.len() > 80 { format!("{}...", &input_str[..80]) } else { input_str.clone() };
                    tracing::info!("[{}] [{:>6}ms] 🔧 {} {}", instance_id, time_ms, name, preview);
                    TraceEvent {
                        time_ms,
                        event_type: "tool_call".to_string(),
                        tool: Some(name),
                        content: input_str,
                        success: None,
                    }
                }
                ToolEvent::ToolResult { name, output, success } => {
                    let preview = if output.len() > 80 { format!("{}...", &output[..80]) } else { output.clone() };
                    let status = if success { "✓" } else { "✗" };
                    tracing::info!("[{}] [{:>6}ms] {} {} -> {}", instance_id, time_ms, status, name, preview);
                    TraceEvent {
                        time_ms,
                        event_type: "tool_result".to_string(),
                        tool: Some(name),
                        content: output,
                        success: Some(success),
                    }
                }
            };
            if let Ok(mut t) = trace_clone.lock() {
                t.push(trace_event);
            }
        });

        // Build task prompt
        let task = format!(
            "# SWE-bench Task\n\n\
            Repository: {}\n\
            Working directory: {}\n\n\
            ## Problem Statement\n\n\
            {}\n\n\
            ## Instructions\n\n\
            1. Find the buggy code using grep to locate relevant functions/classes\n\
            2. Read the source with read_file to understand the bug\n\
            3. Make the MINIMAL fix with edit_file (old_string must match EXACTLY)\n\n\
            CRITICAL RULES:\n\
            - Do NOT run pip install or try to install dependencies\n\
            - Do NOT try to run Python or import the code - it won't work\n\
            - Do NOT write test files - just fix the existing code\n\
            - Focus ONLY on finding and fixing the bug in the source\n\
            - The official test harness will verify your fix later\n\n\
            Your goal: Produce a minimal diff that fixes the bug. Nothing more.",
            instance.repo, repo_dir.display(), instance.problem_statement
        );

        // Run with timeout
        let timeout = config.timeout;
        let executor = Executor::new(executor_config);
        match tokio::time::timeout(timeout, executor.run_task_with_callback(&task, callback)).await {
            Ok(Ok(_)) => {
                // Get the git diff
                let patch = Self::collect_patch_standalone(&repo_dir).await.ok();
                let success = patch.is_some();
                RunResult {
                    instance_id: instance.instance_id.clone(),
                    success,
                    patch,
                    duration: start.elapsed(),
                    error: None,
                    trace: trace.lock().map(|t| t.clone()).unwrap_or_default(),
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Executor failed for {}: {}", instance.instance_id, e);
                RunResult {
                    instance_id: instance.instance_id.clone(),
                    success: false,
                    patch: None,
                    duration: start.elapsed(),
                    error: Some(e.to_string()),
                    trace: trace.lock().map(|t| t.clone()).unwrap_or_default(),
                }
            }
            Err(_) => {
                tracing::warn!("Timeout for {} after {:?}", instance.instance_id, timeout);
                RunResult {
                    instance_id: instance.instance_id.clone(),
                    success: false,
                    patch: None,
                    duration: start.elapsed(),
                    error: Some("Timeout".to_string()),
                    trace: trace.lock().map(|t| t.clone()).unwrap_or_default(),
                }
            }
        }
    }

    /// Collect the git diff as a patch (standalone version).
    async fn collect_patch_standalone(repo_dir: &PathBuf) -> Result<String, String> {
        let output = tokio::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .await
            .map_err(|e| format!("Git diff failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git diff failed: {}", stderr));
        }

        let patch = String::from_utf8_lossy(&output.stdout).to_string();
        if patch.trim().is_empty() {
            return Err("No changes detected".to_string());
        }
        Ok(patch)
    }

    /// Prepare repository standalone (for parallel execution).
    async fn prepare_repo_standalone(config: &BenchmarkConfig, instance: &SWEBenchInstance) -> Result<PathBuf, String> {
        let repo_name = instance.repo.replace("/", "_").replace("-", "_");
        let instance_dir = format!("{}_{}", repo_name, instance.instance_id.replace("-", "_"));
        let repo_dir = config.work_dir.join(&instance_dir);

        if repo_dir.exists() {
            // Reset to base commit
            let output = tokio::process::Command::new("git")
                .args(["checkout", &instance.base_commit])
                .current_dir(&repo_dir)
                .output()
                .await
                .map_err(|e| format!("Git checkout failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Git checkout failed: {}", stderr));
            }

            // Clean any local changes
            let _ = tokio::process::Command::new("git")
                .args(["checkout", "."])
                .current_dir(&repo_dir)
                .output()
                .await;

            return Ok(repo_dir);
        }

        // Clone the repository
        let github_url = format!("https://github.com/{}.git", instance.repo);
        let output = tokio::process::Command::new("git")
            .args(["clone", "--depth", "1", &github_url, repo_dir.to_str().unwrap()])
            .output()
            .await
            .map_err(|e| format!("Git clone failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git clone failed: {}", stderr));
        }

        // Fetch the specific commit
        let output = tokio::process::Command::new("git")
            .args(["fetch", "--depth", "1", "origin", &instance.base_commit])
            .current_dir(&repo_dir)
            .output()
            .await
            .map_err(|e| format!("Git fetch failed: {}", e))?;

        if !output.status.success() {
            // Try full fetch if shallow fails - this is needed for old commits
            tracing::info!("Shallow fetch failed, trying full fetch for {}", instance.instance_id);
            let unshallow = tokio::process::Command::new("git")
                .args(["fetch", "--unshallow"])
                .current_dir(&repo_dir)
                .output()
                .await;

            if unshallow.is_err() || !unshallow.unwrap().status.success() {
                // If unshallow fails, try fetching the specific commit with full depth
                let _ = tokio::process::Command::new("git")
                    .args(["fetch", "origin", &instance.base_commit])
                    .current_dir(&repo_dir)
                    .output()
                    .await;
            }

        }

        // Checkout base commit
        let output = tokio::process::Command::new("git")
            .args(["checkout", &instance.base_commit])
            .current_dir(&repo_dir)
            .output()
            .await
            .map_err(|e| format!("Git checkout failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git checkout failed: {}", stderr));
        }

        Ok(repo_dir)
    }

    /// Run a single instance.
    pub async fn run_instance(&mut self, instance: &SWEBenchInstance) -> RunResult {
        let start = Instant::now();

        // Prepare the repository
        let repo_dir = match self.prepare_repo(instance).await {
            Ok(dir) => dir,
            Err(e) => {
                return RunResult {
                    instance_id: instance.instance_id.clone(),
                    success: false,
                    patch: None,
                    duration: start.elapsed(),
                    error: Some(e),
                    trace: vec![],
                };
            }
        };

        tracing::info!("Running SWE-bench instance: {} in {:?}", instance.instance_id, repo_dir);

        // Create executor config
        let executor_config = ExecutorConfig {
            project_path: repo_dir.clone(),
            llm_url: self.config.llm_url.clone(),
            api_key: self.config.api_key.clone(),
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            dry_run: false,
            stock_tools: false,
        };
        let executor = Executor::new(executor_config);

        // Create trace collector
        let trace: Arc<Mutex<Vec<TraceEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let trace_clone = trace.clone();
        let start_time = start.clone();

        // Create callback to capture all events
        let callback = Arc::new(move |event: ToolEvent| {
            let time_ms = start_time.elapsed().as_millis() as u64;
            let trace_event = match event {
                ToolEvent::ToolCall { name, input } => {
                    let input_str = serde_json::to_string(&input).unwrap_or_default();
                    let preview = if input_str.len() > 100 {
                        // JSON is ASCII-safe but be consistent
                        let mut end = 100;
                        while end > 0 && !input_str.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &input_str[..end])
                    } else {
                        input_str.clone()
                    };
                    tracing::info!("[{:>6}ms] 🔧 {} {}", time_ms, name, preview);
                    TraceEvent {
                        time_ms,
                        event_type: "tool_call".to_string(),
                        tool: Some(name),
                        content: input_str,
                        success: None,
                    }
                }
                ToolEvent::ToolResult { name, success, output } => {
                    let icon = if success { "✓" } else { "✗" };
                    let preview = if output.len() > 100 {
                        // Find a safe UTF-8 boundary for truncation
                        let mut end = 100;
                        while end > 0 && !output.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &output[..end])
                    } else {
                        output.clone()
                    };
                    tracing::info!("[{:>6}ms] {} {} -> {}", time_ms, icon, name, preview);
                    TraceEvent {
                        time_ms,
                        event_type: "tool_result".to_string(),
                        tool: Some(name),
                        content: output,
                        success: Some(success),
                    }
                }
            };
            trace_clone.lock().unwrap().push(trace_event);
        });

        // Create the task prompt - focused on making edits, not verification
        let task = format!(
            "# SWE-bench Task\n\n\
            Repository: {}\n\
            Working directory: {}\n\n\
            ## Problem Statement\n\n\
            {}\n\n\
            ## Instructions\n\n\
            1. Find the buggy code using grep to locate relevant functions/classes\n\
            2. Read the source with read_file to understand the bug\n\
            3. Make the MINIMAL fix with edit_file (old_string must match EXACTLY)\n\n\
            CRITICAL RULES:\n\
            - Do NOT run pip install or try to install dependencies\n\
            - Do NOT try to run Python or import the code - it won't work\n\
            - Do NOT write test files - just fix the existing code\n\
            - Focus ONLY on finding and fixing the bug in the source\n\
            - The official test harness will verify your fix later\n\n\
            Your goal: Produce a minimal diff that fixes the bug. Nothing more.",
            instance.repo,
            repo_dir.display(),
            instance.problem_statement
        );

        // Run with timeout and callback
        let run_result = tokio::time::timeout(
            self.config.timeout,
            executor.run_task_with_callback(&task, callback)
        ).await;

        let duration = start.elapsed();
        let final_trace = trace.lock().unwrap().clone();

        match run_result {
            Ok(Ok(_output)) => {
                // Collect the patch
                match self.collect_patch(&repo_dir).await {
                    Ok(patch) if !patch.is_empty() => RunResult {
                        instance_id: instance.instance_id.clone(),
                        success: true,
                        patch: Some(patch),
                        duration,
                        error: None,
                        trace: final_trace,
                    },
                    Ok(_) => RunResult {
                        instance_id: instance.instance_id.clone(),
                        success: false,
                        patch: None,
                        duration,
                        error: Some("No changes made".to_string()),
                        trace: final_trace,
                    },
                    Err(e) => RunResult {
                        instance_id: instance.instance_id.clone(),
                        success: false,
                        patch: None,
                        duration,
                        error: Some(e),
                        trace: final_trace,
                    },
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Executor failed for {}: {}", instance.instance_id, e);
                // Still try to collect any partial patch
                let patch = self.collect_patch(&repo_dir).await.ok();
                let success = patch.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
                RunResult {
                    instance_id: instance.instance_id.clone(),
                    success,
                    patch,
                    duration,
                    error: Some(e.to_string()),
                    trace: final_trace,
                }
            }
            Err(_) => {
                tracing::warn!("Timeout for {}", instance.instance_id);
                // Collect any partial patch
                let patch = self.collect_patch(&repo_dir).await.ok();
                let success = patch.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
                RunResult {
                    instance_id: instance.instance_id.clone(),
                    success,
                    patch,
                    duration,
                    error: if success { None } else { Some("Timeout".to_string()) },
                    trace: final_trace,
                }
            }
        }
    }

    /// Prepare the repository for an instance.
    async fn prepare_repo(&self, instance: &SWEBenchInstance) -> Result<PathBuf, String> {
        let repo_name = instance.repo.replace('/', "_");
        let instance_dir = instance.instance_id.replace('/', "_").replace('-', "_");
        let repo_dir = self.config.work_dir.join(format!("{}_{}", repo_name, instance_dir));

        // Clone if not exists
        if !repo_dir.exists() {
            let url = format!("https://github.com/{}.git", instance.repo);
            tracing::info!("Cloning {} to {:?}", url, repo_dir);

            let output = tokio::process::Command::new("git")
                .args(["clone", "--depth", "100", &url, repo_dir.to_str().unwrap()])
                .output()
                .await
                .map_err(|e| format!("Git clone failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Git clone failed: {}", stderr));
            }
        }

        // Checkout base commit
        let output = tokio::process::Command::new("git")
            .args(["checkout", &instance.base_commit])
            .current_dir(&repo_dir)
            .output()
            .await
            .map_err(|e| format!("Git checkout failed: {}", e))?;

        if !output.status.success() {
            // Try fetching more history
            let _ = tokio::process::Command::new("git")
                .args(["fetch", "--unshallow"])
                .current_dir(&repo_dir)
                .output()
                .await;

            let output = tokio::process::Command::new("git")
                .args(["checkout", &instance.base_commit])
                .current_dir(&repo_dir)
                .output()
                .await
                .map_err(|e| format!("Git checkout failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Git checkout failed: {}", stderr));
            }
        }

        // Clean working directory
        let _ = tokio::process::Command::new("git")
            .args(["clean", "-fd"])
            .current_dir(&repo_dir)
            .output()
            .await;

        let _ = tokio::process::Command::new("git")
            .args(["checkout", "--", "."])
            .current_dir(&repo_dir)
            .output()
            .await;

        Ok(repo_dir)
    }

    /// Collect the git diff as a patch.
    async fn collect_patch(&self, repo_dir: &PathBuf) -> Result<String, String> {
        let output = tokio::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .await
            .map_err(|e| format!("Git diff failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git diff failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get all results.
    pub fn results(&self) -> &[RunResult] {
        &self.results
    }

    /// Export predictions in SWE-bench format.
    pub fn export_predictions(&self, model_name: &str) -> Vec<Prediction> {
        self.results
            .iter()
            .filter_map(|r| {
                r.patch.as_ref().map(|patch| Prediction {
                    instance_id: r.instance_id.clone(),
                    model_name_or_path: model_name.to_string(),
                    model_patch: patch.clone(),
                })
            })
            .collect()
    }

    /// Write predictions to a JSONL file.
    pub fn write_predictions(&self, path: &PathBuf, model_name: &str) -> Result<(), String> {
        let predictions = self.export_predictions(model_name);
        let mut content = String::new();
        for pred in predictions {
            let line = serde_json::to_string(&pred)
                .map_err(|e| format!("Failed to serialize prediction: {}", e))?;
            content.push_str(&line);
            content.push('\n');
        }
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }
}

/// Run the official SWE-bench evaluation harness.
pub async fn run_swebench_evaluation(
    predictions_path: &PathBuf,
    dataset: DatasetVariant,
    max_workers: usize,
    run_id: Option<&str>,
) -> Result<String, String> {
    let python = "/tmp/swebench-env/bin/python";

    // Check if swebench is installed
    let check = tokio::process::Command::new(python)
        .args(["-c", "import swebench"])
        .output()
        .await
        .map_err(|e| format!("Python check failed: {}", e))?;

    if !check.status.success() {
        return Err("swebench not installed. Run: /tmp/swebench-env/bin/pip install swebench".into());
    }

    let dataset_name = dataset.dataset_name();
    let mut args = vec![
        "-m".to_string(),
        "swebench.harness.run_evaluation".to_string(),
        "--dataset_name".to_string(),
        dataset_name.to_string(),
        "--predictions_path".to_string(),
        predictions_path.to_str().unwrap().to_string(),
        "--max_workers".to_string(),
        max_workers.to_string(),
        "--cache_level".to_string(),
        "env".to_string(),
    ];

    // run_id is required by swebench
    let run_id = match run_id {
        Some(id) => id.to_string(),
        None => format!("palace_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()),
    };
    args.push("--run_id".to_string());
    args.push(run_id);

    tracing::info!("Running SWE-bench evaluation with {} workers...", max_workers);

    let output = tokio::process::Command::new(python)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("Evaluation failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(format!("Evaluation failed:\nstdout: {}\nstderr: {}", stdout, stderr));
    }

    Ok(format!("{}\n{}", stdout, stderr))
}

/// Analysis of SmartRead opportunities in a trace.
#[derive(Debug, Default)]
pub struct SmartReadAnalysis {
    /// Total tool calls
    pub total_calls: usize,
    /// Grep calls that could use SmartRead symbols layer
    pub grep_to_symbols: Vec<(u64, String)>,
    /// Read calls that could use SmartRead AST layer
    pub read_to_ast: Vec<(u64, String)>,
    /// Find/ls calls that could use SmartRead structure
    pub find_to_structure: Vec<(u64, String)>,
    /// Wasted exploration (docs instead of code, etc.)
    pub wasted_exploration: Vec<(u64, String)>,
    /// JECJIT injection points (where context could jump to solve)
    pub jecjit_points: Vec<(u64, String)>,
}

/// Analyze a trace for SmartRead optimization opportunities.
pub fn analyze_trace_for_smartread(events: &[TraceEvent]) -> String {
    let mut analysis = SmartReadAnalysis::default();
    let mut output = String::new();

    output.push_str("═══════════════════════════════════════════════════════\n");
    output.push_str("📊 SmartRead Opportunity Analysis\n");
    output.push_str("═══════════════════════════════════════════════════════\n\n");

    for event in events {
        if event.event_type != "tool_call" {
            continue;
        }

        analysis.total_calls += 1;
        let tool = event.tool.as_deref().unwrap_or("");

        // Analyze grep calls
        if tool == "grep" {
            // Pattern matching function/class definitions
            if event.content.contains("def ") || event.content.contains("class ")
                || event.content.contains("function ") || event.content.contains("fn ")
            {
                analysis.grep_to_symbols.push((
                    event.time_ms,
                    format!("grep for definition → smart_read symbols: {}", &event.content[..event.content.len().min(60)]),
                ));
            }
        }

        // Analyze read_file calls
        if tool == "read_file" {
            // Reading code files could use AST layer
            if event.content.contains(".py") || event.content.contains(".rs")
                || event.content.contains(".js") || event.content.contains(".ts")
            {
                analysis.read_to_ast.push((
                    event.time_ms,
                    format!("read_file → smart_read ast: {}", &event.content[..event.content.len().min(60)]),
                ));
            }
        }

        // Analyze bash find/ls calls
        if tool == "bash" && (event.content.contains("find ") || event.content.contains("ls ")) {
            analysis.find_to_structure.push((
                event.time_ms,
                format!("bash find/ls → smart_read structure: {}", &event.content[..event.content.len().min(60)]),
            ));
        }

        // Detect wasted exploration (docs, tests when looking for impl)
        if tool == "grep" || tool == "read_file" {
            if event.content.contains("/docs/") || event.content.contains("/test")
                || event.content.contains(".txt") || event.content.contains(".rst")
                || event.content.contains(".md")
            {
                analysis.wasted_exploration.push((
                    event.time_ms,
                    format!("Docs/test exploration: {}", &event.content[..event.content.len().min(60)]),
                ));
            }
        }
    }

    // Output analysis
    output.push_str(&format!("Total tool calls: {}\n\n", analysis.total_calls));

    if !analysis.grep_to_symbols.is_empty() {
        output.push_str(&format!("🔍 Grep → SmartRead Symbols ({} opportunities):\n", analysis.grep_to_symbols.len()));
        for (ms, desc) in &analysis.grep_to_symbols[..analysis.grep_to_symbols.len().min(5)] {
            output.push_str(&format!("   [{:>6}ms] {}\n", ms, desc));
        }
        if analysis.grep_to_symbols.len() > 5 {
            output.push_str(&format!("   ... and {} more\n", analysis.grep_to_symbols.len() - 5));
        }
        output.push('\n');
    }

    if !analysis.read_to_ast.is_empty() {
        output.push_str(&format!("📖 Read → SmartRead AST ({} opportunities):\n", analysis.read_to_ast.len()));
        for (ms, desc) in &analysis.read_to_ast[..analysis.read_to_ast.len().min(5)] {
            output.push_str(&format!("   [{:>6}ms] {}\n", ms, desc));
        }
        if analysis.read_to_ast.len() > 5 {
            output.push_str(&format!("   ... and {} more\n", analysis.read_to_ast.len() - 5));
        }
        output.push('\n');
    }

    if !analysis.find_to_structure.is_empty() {
        output.push_str(&format!("📁 Find/Ls → SmartRead Structure ({} opportunities):\n", analysis.find_to_structure.len()));
        for (ms, desc) in &analysis.find_to_structure[..analysis.find_to_structure.len().min(5)] {
            output.push_str(&format!("   [{:>6}ms] {}\n", ms, desc));
        }
        if analysis.find_to_structure.len() > 5 {
            output.push_str(&format!("   ... and {} more\n", analysis.find_to_structure.len() - 5));
        }
        output.push('\n');
    }

    if !analysis.wasted_exploration.is_empty() {
        output.push_str(&format!("⚠️  Wasted Exploration ({} instances):\n", analysis.wasted_exploration.len()));
        for (ms, desc) in &analysis.wasted_exploration[..analysis.wasted_exploration.len().min(5)] {
            output.push_str(&format!("   [{:>6}ms] {}\n", ms, desc));
        }
        if analysis.wasted_exploration.len() > 5 {
            output.push_str(&format!("   ... and {} more\n", analysis.wasted_exploration.len() - 5));
        }
        output.push('\n');
    }

    // Summary
    let potential_savings = analysis.grep_to_symbols.len() + analysis.read_to_ast.len() + analysis.find_to_structure.len();
    output.push_str("═══════════════════════════════════════════════════════\n");
    output.push_str(&format!("💡 Potential SmartRead optimizations: {}\n", potential_savings));
    output.push_str(&format!("⚠️  Wasted exploration calls: {}\n", analysis.wasted_exploration.len()));
    output.push_str("═══════════════════════════════════════════════════════\n");

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_instance() {
        let json = r#"{"instance_id":"astropy__astropy-12907","repo":"astropy/astropy","base_commit":"abc123","problem_statement":"Fix bug","FAIL_TO_PASS":"[\"test_foo\"]","PASS_TO_PASS":"[]"}"#;
        let instance: SWEBenchInstance = serde_json::from_str(json).unwrap();
        assert_eq!(instance.instance_id, "astropy__astropy-12907");
        assert_eq!(instance.repo, "astropy/astropy");
    }

    #[test]
    fn test_dataset_variant() {
        assert_eq!(DatasetVariant::Lite.as_str(), "lite");
        assert_eq!(DatasetVariant::Full.as_str(), "test");
        assert_eq!(DatasetVariant::Verified.as_str(), "verified");
    }
}
