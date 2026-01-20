//! Rust-specific CI pipeline implementation.

use crate::config::ProjectConfig;
use crate::error::{CIError, CIResult};
use crate::levels::CIStep;
use dagger_sdk::Query;

/// Rust-specific pipeline implementation using Dagger.
pub struct RustPipeline<'a> {
    config: &'a ProjectConfig,
}

impl<'a> RustPipeline<'a> {
    /// Create a new Rust pipeline.
    pub fn new(config: &'a ProjectConfig) -> Self {
        Self { config }
    }

    /// Run a single CI step.
    pub async fn run_step(
        &self,
        client: &Query,
        step: CIStep,
    ) -> CIResult<(String, String)> {
        match step {
            CIStep::Compile => self.compile(client, false).await,
            CIStep::Lint => self.lint(client).await,
            CIStep::Test => self.test(client, false).await,
            CIStep::TestAll => self.test(client, true).await,
            CIStep::Run => self.run(client, false).await,
            CIStep::BuildRelease => self.compile(client, true).await,
            CIStep::RunRelease => self.run(client, true).await,
            CIStep::Scenarios => {
                // Scenarios are handled by the pipeline orchestrator
                Ok((String::new(), String::new()))
            }
        }
    }

    /// Create a base container with Rust toolchain.
    async fn base_container(&self, client: &Query) -> CIResult<dagger_sdk::Container> {
        let src_dir = client
            .host()
            .directory(self.config.path.to_string_lossy().to_string());

        let mut container = client
            .container()
            .from(&self.config.base_image);

        // Add environment variables
        for (key, value) in &self.config.env_vars {
            container = container.with_env_variable(key, value);
        }

        // Install additional packages if specified
        if !self.config.packages.is_empty() {
            let packages = self.config.packages.join(" ");
            container = container
                .with_exec(vec!["apt-get", "update"])
                .with_exec(vec!["apt-get", "install", "-y", &packages]);
        }

        // Mount source directory
        container = container
            .with_directory("/app", src_dir)
            .with_workdir("/app");

        // Set up cargo cache if caching is enabled
        if self.config.cache_deps {
            let cache = client.cache_volume("rust-cargo-cache");
            container = container
                .with_mounted_cache("/root/.cargo/registry", cache.clone())
                .with_mounted_cache("/root/.cargo/git", cache);
        }

        Ok(container)
    }

    /// Compile the Rust project.
    async fn compile(&self, client: &Query, release: bool) -> CIResult<(String, String)> {
        let container = self.base_container(client).await?;

        let cmd = if release {
            vec!["cargo", "build", "--release"]
        } else {
            self.config
                .compile_cmd
                .as_ref()
                .map(|c| c.iter().map(|s| s.as_str()).collect())
                .unwrap_or_else(|| vec!["cargo", "build"])
        };

        let result = container
            .with_exec(cmd)
            .stdout()
            .await
            .map_err(|e| CIError::BuildFailed(e.to_string()))?;

        Ok((result, String::new()))
    }

    /// Run clippy linter.
    async fn lint(&self, client: &Query) -> CIResult<(String, String)> {
        let container = self.base_container(client).await?;

        // Install clippy if not present
        let container = container.with_exec(vec!["rustup", "component", "add", "clippy"]);

        let cmd = self
            .config
            .lint_cmd
            .as_ref()
            .map(|c| c.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| vec!["cargo", "clippy", "--", "-D", "warnings"]);

        let result = container
            .with_exec(cmd)
            .stdout()
            .await
            .map_err(|e| CIError::LintFailed(1))?;

        Ok((result, String::new()))
    }

    /// Run tests.
    async fn test(&self, client: &Query, all: bool) -> CIResult<(String, String)> {
        let container = self.base_container(client).await?;

        let cmd = self.config.default_test_cmd(all);
        let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();

        let result = container
            .with_exec(cmd_refs)
            .stdout()
            .await
            .map_err(|e| CIError::TestsFailed(1))?;

        Ok((result, String::new()))
    }

    /// Run the binary.
    async fn run(&self, client: &Query, release: bool) -> CIResult<(String, String)> {
        let container = self.base_container(client).await?;

        // First build
        let build_cmd = if release {
            vec!["cargo", "build", "--release"]
        } else {
            vec!["cargo", "build"]
        };

        let container = container.with_exec(build_cmd);

        // Then run
        let run_cmd = self.config.default_run_cmd(release);
        let run_refs: Vec<&str> = run_cmd.iter().map(|s| s.as_str()).collect();

        // For run checks, we typically want to verify the binary starts and exits cleanly
        // or runs for a short time and responds to signals
        let result = container
            .with_exec(run_refs)
            .stdout()
            .await
            .map_err(|e| CIError::ExecutionFailed(e.to_string()))?;

        Ok((result, String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use crate::levels::CILevel;

    #[test]
    fn test_rust_pipeline_creation() {
        let config = ProjectConfig::rust("./test-project").with_level(CILevel::Basic);

        let pipeline = RustPipeline::new(&config);
        assert_eq!(pipeline.config.project_type, crate::config::ProjectType::Rust);
    }
}
