//! Refinement checking for Intent systems.
//!
//! This module handles:
//! - Auto-inference of refinement maps from naming conventions
//! - Explicit refinement map overrides
//! - TLA+ simulation theorem generation

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::parser::ast::{Maturity, RefinementMapping, SystemDecl};

/// A computed refinement map with auto-inferred and explicit mappings.
#[derive(Debug, Clone)]
pub struct ComputedRefinement {
    /// The system being refined.
    pub system_name: String,
    /// Path to the abstract TLA+ spec.
    pub abstract_spec: String,
    /// Mapping from abstract states to concrete states.
    pub mappings: HashMap<String, Vec<String>>,
    /// Auto-inferred mappings (for reporting).
    pub inferred: Vec<String>,
    /// Explicit mappings (for reporting).
    pub explicit: Vec<String>,
}

/// Compute the refinement map for a system, combining auto-inference with explicit overrides.
pub fn compute_refinement(system: &SystemDecl) -> Option<ComputedRefinement> {
    let abstract_spec = system.refines.as_ref()?;
    let mut mappings: HashMap<String, Vec<String>> = HashMap::new();
    let inferred = Vec::new();
    let mut explicit = Vec::new();

    // Auto-inference from naming convention: ConcreteState -> abstract.concretestate
    // This would require access to both concrete and abstract state definitions.
    // For now, we just record the explicit mappings.

    // Apply explicit refinement map
    if let Some(ref_map) = &system.refinement_map {
        for mapping in &ref_map.mappings {
            mappings.insert(
                mapping.abstract_state.clone(),
                mapping.concrete_states.clone(),
            );
            explicit.push(format!(
                "{} -> [{}]",
                mapping.abstract_state,
                mapping.concrete_states.join(", ")
            ));
        }
    }

    Some(ComputedRefinement {
        system_name: system.name.clone(),
        abstract_spec: abstract_spec.clone(),
        mappings,
        inferred,
        explicit,
    })
}

/// Generate a TLA+ refinement proof obligation.
#[allow(clippy::too_many_arguments)]
pub fn generate_refinement_tla(
    refinement: &ComputedRefinement,
    concrete_states: &[String],
    _concrete_initial: &str,
    _concrete_transitions: &[(String, String)],
    _output_dir: &Path,
) -> Result<String> {
    let module_name = format!("{}_Refinement", refinement.system_name);

    let mut tla = String::new();

    // Module header
    tla.push_str(&format!("---- MODULE {} ----\n", module_name));
    tla.push_str("EXTENDS TLC, Sequences, Integers\n\n");

    // Import abstract and concrete specs
    tla.push_str(&format!("INSTANCE {}\n", refinement.abstract_spec));
    tla.push_str(&format!("INSTANCE {}_Concrete\n", refinement.system_name));
    tla.push_str("\n");

    // Abstraction function
    tla.push_str("\\* Abstraction function: maps concrete states to abstract states\n");
    tla.push_str("Abs(concrete_state) ==\n");
    tla.push_str("  CASE concrete_state = \"dummy\" -> \"dummy\"\n");

    for (abstract_state, concrete_states) in &refinement.mappings {
        for concrete_state in concrete_states {
            tla.push_str(&format!(
                "    [] concrete_state = \"{}\" -> \"{}\"\n",
                concrete_state, abstract_state
            ));
        }
    }
    // Default: identity mapping for states not in refinement map
    for state in concrete_states {
        if !refinement.mappings.values().any(|v| v.contains(state)) {
            tla.push_str(&format!(
                "    [] concrete_state = \"{}\" -> \"{}\"\n",
                state, state
            ));
        }
    }
    tla.push_str("\n");

    // Simulation theorem
    tla.push_str("\\* Simulation theorem: every concrete step simulates an abstract step\n");
    tla.push_str("THEOREM Simulation ==\n");
    tla.push_str("  ASSUME Init_concrete\n");
    tla.push_str("  PROVE  Init_abstract\n");
    tla.push_str("/\\ \\A s, s' \\in States_concrete:\n");
    tla.push_str("     (state_concrete = s /\\ Next_concrete /\\ state_concrete' = s')\n");
    tla.push_str("     => (state_abstract = Abs(s) /\\ Next_abstract /\\ state_abstract' = Abs(s'))\n");
    tla.push_str("\n");

    // Module footer
    tla.push_str("====\n");

    Ok(tla)
}

/// Infer refinement mappings from concrete and abstract state names.
///
/// Convention: `ConcreteState` -> `abstract.concretestate` (case-insensitive)
pub fn infer_refinement_mappings(
    concrete_states: &[String],
    abstract_states: &[String],
) -> Vec<RefinementMapping> {
    let mut mappings = Vec::new();

    for concrete in concrete_states {
        let concrete_lower = concrete.to_lowercase();

        // Look for matching abstract state
        for abstract_state in abstract_states {
            // Try exact match
            if abstract_state.to_lowercase() == concrete_lower {
                // This is a direct match, no mapping needed
                continue;
            }

            // Try matching against dotted abstract state (e.g., "abstract.pending")
            if let Some(suffix) = abstract_state.split('.').last() {
                if suffix.to_lowercase() == concrete_lower {
                    mappings.push(RefinementMapping {
                        abstract_state: abstract_state.clone(),
                        concrete_states: vec![concrete.clone()],
                    });
                }
            }
        }
    }

    mappings
}

/// Report refinement gaps - missing mappings or states.
pub fn report_refinement_gaps(
    refinement: &ComputedRefinement,
    concrete_states: &[String],
    abstract_states: &[String],
) -> Vec<String> {
    let mut gaps = Vec::new();

    // Check for unmapped concrete states
    let mapped_concrete: Vec<String> = refinement
        .mappings
        .values()
        .flatten()
        .cloned()
        .collect();

    for state in concrete_states {
        if !mapped_concrete.contains(state) {
            // Check if there's an implicit mapping (same name)
            if !abstract_states.contains(state) {
                gaps.push(format!("Unmapped concrete state: {}", state));
            }
        }
    }

    // Check for unused abstract states
    for state in abstract_states {
        if !refinement.mappings.contains_key(state) {
            // Simple name might be implicitly mapped
            let simple_name = state.split('.').last().unwrap_or(state);
            if !concrete_states.iter().any(|c| c.to_lowercase() == simple_name.to_lowercase()) {
                gaps.push(format!("Abstract state not mapped: {}", state));
            }
        }
    }

    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_refinement() {
        let concrete = vec!["PENDING".to_string(), "PROCESSING".to_string(), "DONE".to_string()];
        let abstract_states = vec!["abstract.pending".to_string(), "abstract.completed".to_string()];

        let mappings = infer_refinement_mappings(&concrete, &abstract_states);

        // PENDING should map to abstract.pending
        assert!(mappings.iter().any(|m|
            m.abstract_state == "abstract.pending" &&
            m.concrete_states.contains(&"PENDING".to_string())
        ));
    }

    #[test]
    fn test_report_gaps() {
        let system = SystemDecl {
            name: "Test".to_string(),
            maturity: Maturity::default(),
            description: None,
            parent: None,
            subsystems: vec![],
            implements: None,
            scopes: vec![],
            constraints: vec![],
            models: vec![],
            interfaces: vec![],
            adapters: vec![],
            behaviors: vec![],
            patterns: vec![],
            let_bindings: vec![],
            predicates: vec![],
            applies: vec![],
            refines: Some("abstract/Test.tla".to_string()),
            refinement_map: None,
            progression: None,
            current_stage: None,
            decided_because: vec![],
            rejected_alternatives: vec![],
            revisit_when: vec![],
            span: None,
        };

        let refinement = compute_refinement(&system).unwrap();
        let concrete = vec!["A".to_string(), "B".to_string()];
        let abstract_states = vec!["X".to_string(), "Y".to_string()];

        let gaps = report_refinement_gaps(&refinement, &concrete, &abstract_states);

        assert!(!gaps.is_empty());
    }
}
