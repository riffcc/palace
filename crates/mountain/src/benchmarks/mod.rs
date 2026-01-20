//! Mountain benchmarks for testing cascading LLM control.
//!
//! These benchmarks use real programs to validate Mountain's ability to
//! control software in realtime through time-delayed execution.
//!
//! For GBA-specific benchmarks (PokeBench), see the `palace-gba` crate.

/// Trait for Mountain benchmarks.
#[async_trait::async_trait]
pub trait Benchmark: Send + Sync {
    /// Benchmark name.
    fn name(&self) -> &str;

    /// Benchmark description.
    fn description(&self) -> &str;

    /// Run the benchmark.
    async fn run(&mut self) -> crate::MountainResult<BenchmarkResult>;

    /// Get current progress (0.0 - 1.0).
    fn progress(&self) -> f32;

    /// Whether the benchmark supports checkpointing.
    fn supports_checkpoints(&self) -> bool;
}

/// Result from running a benchmark.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Benchmark name.
    pub name: String,

    /// Whether the benchmark completed successfully.
    pub success: bool,

    /// Completion percentage (0.0 - 1.0).
    pub completion: f32,

    /// Total time elapsed.
    pub elapsed: std::time::Duration,

    /// Number of decisions made by the cascade.
    pub decisions_made: u64,

    /// Average cascade latency.
    pub avg_cascade_latency_ms: f32,

    /// Number of model vetoes.
    pub vetoes: u64,

    /// Detailed metrics.
    pub metrics: std::collections::HashMap<String, serde_json::Value>,

    /// Any errors encountered.
    pub errors: Vec<String>,
}

impl Default for BenchmarkResult {
    fn default() -> Self {
        Self {
            name: String::new(),
            success: false,
            completion: 0.0,
            elapsed: std::time::Duration::ZERO,
            decisions_made: 0,
            avg_cascade_latency_ms: 0.0,
            vetoes: 0,
            metrics: std::collections::HashMap::new(),
            errors: vec![],
        }
    }
}
