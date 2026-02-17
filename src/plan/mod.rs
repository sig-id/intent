//! Plan mode validation.
//!
//! This module is being updated for v0.4. Currently a stub.

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::SystemDecl;

/// Result of plan-mode validation for a single system.
#[derive(Debug, Clone, Serialize)]
pub struct PlanResult {
    pub system: String,
    pub checks: Vec<PlanCheck>,
}

/// A single plan-mode check.
#[derive(Debug, Clone, Serialize)]
pub struct PlanCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// Validate systems in plan mode (no codebase required).
///
/// This is a stub for v0.4.
pub fn validate(_systems: &[SystemDecl]) -> Result<Vec<PlanResult>> {
    // TODO: Implement v0.4 plan mode validation
    Ok(Vec::new())
}
