//! Behavioral module - TLA+ generation and verification.
//!
//! This module is being updated for v0.4. Currently a stub.

pub mod composition;
pub mod patterns;
pub mod refinement;
pub mod statemachine;

// Re-export key types from composition
pub use composition::{
    compose_behaviors, ComposedBehavior, CompositionConflict, CompositionConfig, ConflictStrategy,
    ConflictType,
};

// Re-export key types from refinement
pub use refinement::{
    validate_refinement, ComputedRefinement, RefinementResult, RefinementViolation, ViolationType,
};

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::SystemDecl;

/// Result of behavioral obligation verification.
#[derive(Debug, Clone, Serialize)]
pub struct ObligationResult {
    pub pattern: String,
    pub target: String,
    pub refines: String,
    pub concern: String,
    pub status: ObligationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ObligationStatus {
    Pass,
    Fail,
    Skipped,
}

impl std::fmt::Display for ObligationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObligationStatus::Pass => write!(f, "pass"),
            ObligationStatus::Fail => write!(f, "fail"),
            ObligationStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Compile TLA+ specifications from systems.
///
/// This is a stub for v0.4 - returns empty list.
pub fn compile(
    _systems: &[SystemDecl],
    _output_dir: &Path,
    _project_root: &Path,
) -> Result<Vec<PathBuf>> {
    // TODO: Implement v0.4 behavioral compilation
    Ok(Vec::new())
}

/// Verify TLA+ obligations.
///
/// This is a stub for v0.4 - returns empty list.
pub fn verify(
    _obligation_dir: &Path,
    _project_root: &Path,
) -> Result<Vec<ObligationResult>> {
    // TODO: Implement v0.4 behavioral verification
    Ok(Vec::new())
}
