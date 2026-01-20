//! Palace CI - Configurable build/test/run pipelines powered by Dagger.
//!
//! This crate provides a high-level API for defining CI pipelines with
//! configurable granularity levels.
//!
//! # CI Levels
//!
//! - **Simple**: Just check that the project compiles
//! - **Lint**: Zero warnings and linter errors
//! - **Basic**: Run tests
//! - **BasicLong**: Run ALL tests including ignored ones
//! - **Run**: Basic + run the binary
//! - **RunProd**: Run the binary in release mode
//!
//! # Attribution
//!
//! This crate integrates with [Dagger](https://dagger.io/) (Apache 2.0 Licensed).
//! See: https://github.com/dagger/dagger/tree/main/sdk/rust
//!
//! # Example
//!
//! ```rust,ignore
//! use palace_ci::{Pipeline, CILevel, ProjectConfig};
//!
//! #[tokio::main]
//! async fn main() -> eyre::Result<()> {
//!     let config = ProjectConfig::rust("./my-project")
//!         .with_level(CILevel::Basic);
//!
//!     let pipeline = Pipeline::new(config);
//!     let result = pipeline.run().await?;
//!
//!     println!("CI Result: {:?}", result);
//!     Ok(())
//! }
//! ```

mod config;
mod error;
mod levels;
mod pipeline;
mod rust;
mod scenarios;

pub use config::{ProjectConfig, ProjectType};
pub use error::{CIError, CIResult};
pub use levels::CILevel;
pub use pipeline::{Pipeline, PipelineResult, StepResult};
pub use scenarios::{Scenario, ScenarioRunner};
