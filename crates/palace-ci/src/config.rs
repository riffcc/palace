//! Project configuration for CI pipelines.

use crate::levels::CILevel;
use crate::scenarios::Scenario;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Project type for language-specific CI steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    /// Rust project (cargo build/test/clippy).
    #[default]
    Rust,
    /// Node.js project (npm/yarn/pnpm).
    Node,
    /// Python project (pip/poetry/uv).
    Python,
    /// Go project.
    Go,
    /// Generic project with custom commands.
    Generic,
}

/// Configuration for a CI pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name.
    pub name: String,

    /// Path to the project root.
    pub path: PathBuf,

    /// Project type.
    pub project_type: ProjectType,

    /// CI level to run.
    pub level: CILevel,

    /// Docker base image to use.
    pub base_image: String,

    /// Binary name (for run steps).
    pub binary_name: Option<String>,

    /// Custom compile command (overrides default).
    pub compile_cmd: Option<Vec<String>>,

    /// Custom lint command (overrides default).
    pub lint_cmd: Option<Vec<String>>,

    /// Custom test command (overrides default).
    pub test_cmd: Option<Vec<String>>,

    /// Custom run command (overrides default).
    pub run_cmd: Option<Vec<String>>,

    /// Environment variables to set.
    pub env_vars: Vec<(String, String)>,

    /// Additional packages to install in the container.
    pub packages: Vec<String>,

    /// Test scenarios for scripted automation.
    pub scenarios: Vec<Scenario>,

    /// Timeout in seconds for each step.
    pub timeout_secs: u64,

    /// Whether to cache dependencies between runs.
    pub cache_deps: bool,
}

impl ProjectConfig {
    /// Create a new Rust project configuration.
    pub fn rust(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        Self {
            name,
            path,
            project_type: ProjectType::Rust,
            level: CILevel::default(),
            base_image: "rust:1.84-slim".to_string(),
            binary_name: None,
            compile_cmd: None,
            lint_cmd: None,
            test_cmd: None,
            run_cmd: None,
            env_vars: vec![],
            packages: vec![],
            scenarios: vec![],
            timeout_secs: 600,
            cache_deps: true,
        }
    }

    /// Create a new Node.js project configuration.
    pub fn node(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        Self {
            name,
            path,
            project_type: ProjectType::Node,
            level: CILevel::default(),
            base_image: "node:22-slim".to_string(),
            binary_name: None,
            compile_cmd: None,
            lint_cmd: None,
            test_cmd: None,
            run_cmd: None,
            env_vars: vec![],
            packages: vec![],
            scenarios: vec![],
            timeout_secs: 600,
            cache_deps: true,
        }
    }

    /// Create a new Python project configuration.
    pub fn python(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        Self {
            name,
            path,
            project_type: ProjectType::Python,
            level: CILevel::default(),
            base_image: "python:3.12-slim".to_string(),
            binary_name: None,
            compile_cmd: None,
            lint_cmd: None,
            test_cmd: None,
            run_cmd: None,
            env_vars: vec![],
            packages: vec![],
            scenarios: vec![],
            timeout_secs: 600,
            cache_deps: true,
        }
    }

    /// Create a generic project configuration with custom commands.
    pub fn generic(path: impl Into<PathBuf>, base_image: impl Into<String>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        Self {
            name,
            path,
            project_type: ProjectType::Generic,
            level: CILevel::default(),
            base_image: base_image.into(),
            binary_name: None,
            compile_cmd: None,
            lint_cmd: None,
            test_cmd: None,
            run_cmd: None,
            env_vars: vec![],
            packages: vec![],
            scenarios: vec![],
            timeout_secs: 600,
            cache_deps: true,
        }
    }

    /// Set the CI level.
    pub fn with_level(mut self, level: CILevel) -> Self {
        self.level = level;
        self
    }

    /// Set the binary name.
    pub fn with_binary(mut self, name: impl Into<String>) -> Self {
        self.binary_name = Some(name.into());
        self
    }

    /// Set a custom compile command.
    pub fn with_compile_cmd(mut self, cmd: Vec<String>) -> Self {
        self.compile_cmd = Some(cmd);
        self
    }

    /// Set a custom lint command.
    pub fn with_lint_cmd(mut self, cmd: Vec<String>) -> Self {
        self.lint_cmd = Some(cmd);
        self
    }

    /// Set a custom test command.
    pub fn with_test_cmd(mut self, cmd: Vec<String>) -> Self {
        self.test_cmd = Some(cmd);
        self
    }

    /// Set a custom run command.
    pub fn with_run_cmd(mut self, cmd: Vec<String>) -> Self {
        self.run_cmd = Some(cmd);
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Add packages to install.
    pub fn with_packages(mut self, packages: Vec<String>) -> Self {
        self.packages = packages;
        self
    }

    /// Add test scenarios.
    pub fn with_scenarios(mut self, scenarios: Vec<Scenario>) -> Self {
        self.scenarios = scenarios;
        self
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Disable dependency caching.
    pub fn without_cache(mut self) -> Self {
        self.cache_deps = false;
        self
    }

    /// Get default compile command for the project type.
    pub fn default_compile_cmd(&self) -> Vec<String> {
        if let Some(ref cmd) = self.compile_cmd {
            return cmd.clone();
        }

        match self.project_type {
            ProjectType::Rust => vec!["cargo".into(), "build".into()],
            ProjectType::Node => vec!["npm".into(), "run".into(), "build".into()],
            ProjectType::Python => vec!["python".into(), "-m".into(), "py_compile".into(), ".".into()],
            ProjectType::Go => vec!["go".into(), "build".into(), "./...".into()],
            ProjectType::Generic => vec!["echo".into(), "No compile command configured".into()],
        }
    }

    /// Get default lint command for the project type.
    pub fn default_lint_cmd(&self) -> Vec<String> {
        if let Some(ref cmd) = self.lint_cmd {
            return cmd.clone();
        }

        match self.project_type {
            ProjectType::Rust => vec![
                "cargo".into(),
                "clippy".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ],
            ProjectType::Node => vec!["npm".into(), "run".into(), "lint".into()],
            ProjectType::Python => vec!["ruff".into(), "check".into(), ".".into()],
            ProjectType::Go => vec!["golangci-lint".into(), "run".into()],
            ProjectType::Generic => vec!["echo".into(), "No lint command configured".into()],
        }
    }

    /// Get default test command for the project type.
    pub fn default_test_cmd(&self, all: bool) -> Vec<String> {
        if let Some(ref cmd) = self.test_cmd {
            return cmd.clone();
        }

        match self.project_type {
            ProjectType::Rust => {
                if all {
                    vec![
                        "cargo".into(),
                        "test".into(),
                        "--".into(),
                        "--include-ignored".into(),
                    ]
                } else {
                    vec!["cargo".into(), "test".into()]
                }
            }
            ProjectType::Node => vec!["npm".into(), "test".into()],
            ProjectType::Python => {
                if all {
                    vec!["pytest".into(), "--run-slow".into()]
                } else {
                    vec!["pytest".into()]
                }
            }
            ProjectType::Go => vec!["go".into(), "test".into(), "./...".into()],
            ProjectType::Generic => vec!["echo".into(), "No test command configured".into()],
        }
    }

    /// Get default run command for the project type.
    pub fn default_run_cmd(&self, release: bool) -> Vec<String> {
        if let Some(ref cmd) = self.run_cmd {
            return cmd.clone();
        }

        let binary = self
            .binary_name
            .clone()
            .unwrap_or_else(|| self.name.clone());

        match self.project_type {
            ProjectType::Rust => {
                if release {
                    vec!["cargo".into(), "run".into(), "--release".into()]
                } else {
                    vec!["cargo".into(), "run".into()]
                }
            }
            ProjectType::Node => vec!["npm".into(), "start".into()],
            ProjectType::Python => vec!["python".into(), "-m".into(), binary],
            ProjectType::Go => vec!["go".into(), "run".into(), ".".into()],
            ProjectType::Generic => vec!["echo".into(), "No run command configured".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_config() {
        let config = ProjectConfig::rust("./my-project").with_level(CILevel::Basic);

        assert_eq!(config.name, "my-project");
        assert_eq!(config.project_type, ProjectType::Rust);
        assert_eq!(config.level, CILevel::Basic);
        assert!(config.base_image.contains("rust"));
    }

    #[test]
    fn test_default_commands() {
        let config = ProjectConfig::rust("./test");

        assert_eq!(config.default_compile_cmd(), vec!["cargo", "build"]);
        assert!(config.default_lint_cmd().contains(&"clippy".to_string()));
        assert_eq!(config.default_test_cmd(false), vec!["cargo", "test"]);
    }
}
