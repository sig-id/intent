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
/// Patterns are defined in `stdlib/patterns.intent` and TLA+ generation
/// is handled by the generic `tla` module. This function is
/// reserved for future pattern-specific optimizations.
///
/// Returns `None` - pattern obligations are generated via the standard
/// behavior-to-TLA+ pipeline.
pub fn generate(
    _pattern_name: &str,
    _system_name: &str,
    _app: &PatternApplication,
    _project_root: &Path,
) -> Result<Option<PatternObligation>> {
    // Patterns are defined in stdlib/patterns.intent
    // TLA+ generation is handled by tla.rs
    Ok(None)
}
