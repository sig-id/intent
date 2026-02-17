pub mod circuit_breaker;

use std::path::Path;

use anyhow::Result;

use crate::parser::ast::PatternApplication;

/// A generated TLA+ obligation for a single pattern application.
pub struct PatternObligation {
    /// TLA+ module content.
    pub tla_content: String,
    /// The TLA+ module that this obligation INSTANCEs (must be copied alongside).
    pub instance_module: Option<String>,
    /// The invariant name to check with Apalache.
    pub invariant_name: String,
}

/// Generate a TLA+ obligation module for a pattern application.
///
/// Returns `None` if the pattern is not recognized (will be skipped).
pub fn generate(
    pattern_name: &str,
    _system_name: &str,
    _app: &PatternApplication,
    _project_root: &Path,
) -> Result<Option<PatternObligation>> {
    // TODO: Implement v0.4 pattern obligation generation
    match pattern_name {
        "CircuitBreaker" => Ok(None), // circuit_breaker::generate(system_name, app, project_root).map(Some),
        _ => Ok(None),
    }
}
