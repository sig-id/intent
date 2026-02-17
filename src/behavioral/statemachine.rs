//! State machine to TLA+ transpiler.
//!
//! This module is being updated for v0.4. Currently a stub.

use std::path::Path;

use anyhow::Result;

use crate::parser::ast::BehaviorDecl;

/// A generated TLA+ module for a state machine.
pub struct StateMachineTla {
    /// TLA+ module content.
    pub content: String,
    /// The module name.
    pub module_name: String,
    /// Invariants to check.
    pub invariants: Vec<String>,
}

/// Generate a TLA+ specification from an Intent behavior.
pub fn generate(_behavior: &BehaviorDecl, _system_name: &str, _project_root: &Path) -> Result<StateMachineTla> {
    // TODO: Implement v0.4 state machine generation
    Ok(StateMachineTla {
        content: String::new(),
        module_name: String::new(),
        invariants: Vec::new(),
    })
}
