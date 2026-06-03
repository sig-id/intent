mod registry;

pub use registry::{PatternExpansion, PatternRegistry};

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
/// This function expands the pattern and generates TLA+ for verification.
pub fn generate(
    pattern_name: &str,
    system_name: &str,
    app: &PatternApplication,
    _project_root: &Path,
    registry: &PatternRegistry,
) -> Result<Option<PatternObligation>> {
    // Expand the pattern
    let expansion = registry.expand(app)?;

    // Generate TLA+ for pattern's behavior
    let mut tla_content = String::new();

    // Module header
    tla_content.push_str(&format!(
        "---- MODULE {}_{} ----\n",
        system_name, pattern_name
    ));
    tla_content.push_str("EXTENDS Naturals, Sequences\n\n");

    // Pattern constants (parameters)
    if !expansion.params.is_empty() {
        tla_content.push_str("CONSTANTS\n");
        for (name, value) in &expansion.generate_constants() {
            tla_content.push_str(&format!("    {} \\* = {}\n", name, value));
        }
        tla_content.push('\n');
    }

    // States
    let state_names: Vec<&str> = expansion.states.iter().map(|s| s.name.as_str()).collect();
    tla_content.push_str(&format!("States == {{{}}}\n\n", state_names.join(", ")));

    // Variables
    tla_content.push_str("VARIABLES state\n\n");

    // Init
    let initial: Vec<&str> = expansion
        .states
        .iter()
        .filter(|s| s.initial)
        .map(|s| s.name.as_str())
        .collect();
    if initial.len() == 1 {
        tla_content.push_str(&format!("Init == state = {}\n\n", initial[0]));
    } else {
        tla_content.push_str(&format!(
            "Init == state \\in {{{}}}\n\n",
            initial.join(", ")
        ));
    }

    // Transitions as actions
    for t in &expansion.transitions {
        if let (Some(from), Some(to)) = (t.from.as_state(), t.to.as_state()) {
            tla_content.push_str(&format!(
                "{}_{} == state = {} /\\ state' = {}\n",
                from, t.on_event, from, to
            ));
        }
    }
    tla_content.push('\n');

    // Next
    tla_content.push_str("Next ==\n");
    for t in &expansion.transitions {
        if let Some(from) = t.from.as_state() {
            tla_content.push_str(&format!("    \\/ {}_{}\n", from, t.on_event));
        }
    }
    tla_content.push('\n');

    // Spec
    tla_content.push_str("Spec == Init /\\ [][Next]_state\n\n");

    // Properties
    for prop in &expansion.properties {
        tla_content.push_str(&format!("\\* Property: {}\n", prop.name));
    }

    // Footer
    tla_content.push_str(&format!("======================================\n"));

    Ok(Some(PatternObligation {
        tla_content,
        instance_module: None,
        invariant_name: format!("{}_{}_TypeOK", system_name, pattern_name),
    }))
}
