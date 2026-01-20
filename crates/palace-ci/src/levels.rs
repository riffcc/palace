//! CI level definitions.

use serde::{Deserialize, Serialize};

/// CI granularity levels - each level includes all checks from previous levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CILevel {
    /// Just check that the project compiles.
    #[default]
    Simple,

    /// Zero warnings and linter errors (clippy for Rust).
    Lint,

    /// Run tests (excluding ignored/slow tests).
    Basic,

    /// Run ALL tests including ignored ones.
    BasicLong,

    /// Basic tests + run the binary to verify it starts.
    Run,

    /// Run the binary in release mode with production checks.
    RunProd,

    /// Full end-to-end scenarios with scripted automation.
    Scenarios,
}

impl CILevel {
    /// Returns the steps that should be executed for this level.
    pub fn steps(&self) -> Vec<CIStep> {
        match self {
            CILevel::Simple => vec![CIStep::Compile],
            CILevel::Lint => vec![CIStep::Compile, CIStep::Lint],
            CILevel::Basic => vec![CIStep::Compile, CIStep::Lint, CIStep::Test],
            CILevel::BasicLong => vec![CIStep::Compile, CIStep::Lint, CIStep::TestAll],
            CILevel::Run => vec![CIStep::Compile, CIStep::Lint, CIStep::Test, CIStep::Run],
            CILevel::RunProd => vec![
                CIStep::Compile,
                CIStep::Lint,
                CIStep::Test,
                CIStep::BuildRelease,
                CIStep::RunRelease,
            ],
            CILevel::Scenarios => vec![
                CIStep::Compile,
                CIStep::Lint,
                CIStep::Test,
                CIStep::BuildRelease,
                CIStep::RunRelease,
                CIStep::Scenarios,
            ],
        }
    }

    /// Check if this level includes compilation.
    pub fn compiles(&self) -> bool {
        true // All levels compile
    }

    /// Check if this level includes linting.
    pub fn lints(&self) -> bool {
        *self >= CILevel::Lint
    }

    /// Check if this level runs tests.
    pub fn tests(&self) -> bool {
        *self >= CILevel::Basic
    }

    /// Check if this level runs all tests (including ignored).
    pub fn tests_all(&self) -> bool {
        *self >= CILevel::BasicLong
    }

    /// Check if this level runs the binary.
    pub fn runs(&self) -> bool {
        *self >= CILevel::Run
    }

    /// Check if this level builds/runs in release mode.
    pub fn release(&self) -> bool {
        *self >= CILevel::RunProd
    }

    /// Check if this level runs scenarios.
    pub fn scenarios(&self) -> bool {
        *self >= CILevel::Scenarios
    }
}

/// Individual CI steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CIStep {
    /// Compile the project (debug mode).
    Compile,
    /// Run linter/clippy.
    Lint,
    /// Run tests (excluding ignored).
    Test,
    /// Run all tests including ignored.
    TestAll,
    /// Run the binary (debug mode).
    Run,
    /// Build in release mode.
    BuildRelease,
    /// Run the binary (release mode).
    RunRelease,
    /// Run scripted scenarios.
    Scenarios,
}

impl std::fmt::Display for CIStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CIStep::Compile => write!(f, "compile"),
            CIStep::Lint => write!(f, "lint"),
            CIStep::Test => write!(f, "test"),
            CIStep::TestAll => write!(f, "test-all"),
            CIStep::Run => write!(f, "run"),
            CIStep::BuildRelease => write!(f, "build-release"),
            CIStep::RunRelease => write!(f, "run-release"),
            CIStep::Scenarios => write!(f, "scenarios"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_ordering() {
        assert!(CILevel::Simple < CILevel::Lint);
        assert!(CILevel::Lint < CILevel::Basic);
        assert!(CILevel::Basic < CILevel::BasicLong);
        assert!(CILevel::BasicLong < CILevel::Run);
        assert!(CILevel::Run < CILevel::RunProd);
        assert!(CILevel::RunProd < CILevel::Scenarios);
    }

    #[test]
    fn test_level_steps() {
        assert_eq!(CILevel::Simple.steps(), vec![CIStep::Compile]);
        assert!(CILevel::Lint.steps().contains(&CIStep::Lint));
        assert!(CILevel::Basic.steps().contains(&CIStep::Test));
        assert!(CILevel::Scenarios.steps().contains(&CIStep::Scenarios));
    }

    #[test]
    fn test_level_checks() {
        assert!(CILevel::Simple.compiles());
        assert!(!CILevel::Simple.lints());
        assert!(CILevel::Lint.lints());
        assert!(!CILevel::Lint.tests());
        assert!(CILevel::Basic.tests());
        assert!(CILevel::RunProd.release());
    }
}
