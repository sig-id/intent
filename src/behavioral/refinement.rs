//! Refinement checking for Intent systems.
//!
//! This module provides validation that a concrete behavior correctly refines
//! an abstract specification, ensuring all abstract behaviors are preserved.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};

use crate::parser::ast::{BehaviorDecl, RefinementMap, StateDecl};

/// Result of refinement validation.
#[derive(Debug, Clone)]
pub struct RefinementResult {
    /// Name of the concrete behavior
    pub concrete_behavior: String,
    /// Name of the abstract specification
    pub abstract_spec: String,
    /// Whether the refinement is valid
    pub is_valid: bool,
    /// Violations found during validation
    pub violations: Vec<RefinementViolation>,
    /// Computed or provided refinement map
    pub map: RefinementMapInfo,
}

/// Information about the refinement mapping.
#[derive(Debug, Clone)]
pub struct RefinementMapInfo {
    /// Mapping from concrete states to abstract states
    pub concrete_to_abstract: HashMap<String, String>,
    /// Abstract states that are covered by concrete states
    pub covered_abstract_states: HashSet<String>,
    /// Abstract states that are not reachable via any concrete state
    pub unreachable_abstract_states: Vec<String>,
}

/// A violation of the refinement relation.
#[derive(Debug, Clone, PartialEq)]
pub enum RefinementViolation {
    /// A concrete state has no corresponding abstract state
    UnmappedConcreteState { state: String },
    /// An abstract state is not reachable via any concrete state path
    UnreachableAbstractState { state: String },
    /// A concrete transition doesn't correspond to a valid abstract transition
    IllegalTransition {
        from: String,
        to: String,
        event: String,
        reason: String,
    },
    /// Multiple concrete states map to the same abstract state with different transitions
    InconsistentMapping {
        abstract_state: String,
        concrete_states: Vec<String>,
    },
}

/// Validate that a concrete behavior refines an abstract specification.
///
/// # Arguments
/// * `concrete` - The concrete (implementation) behavior
/// * `abstract_spec` - The abstract specification being refined
/// * `refinement_map` - Optional explicit mapping from concrete to abstract states
///
/// # Returns
/// A `RefinementResult` indicating whether the refinement is valid and any violations.
pub fn validate_refinement(
    concrete: &BehaviorDecl,
    abstract_spec: &BehaviorDecl,
    refinement_map: &Option<RefinementMap>,
) -> Result<RefinementResult> {
    // Build the concrete-to-abstract state mapping
    let map = build_refinement_map(concrete, abstract_spec, refinement_map)?;

    let mut result = RefinementResult {
        concrete_behavior: concrete.name.clone(),
        abstract_spec: abstract_spec.name.clone(),
        is_valid: true,
        violations: Vec::new(),
        map,
    };

    // Validate all concrete states are mapped
    validate_state_coverage(&mut result, concrete, abstract_spec)?;

    // Validate concrete transitions correspond to valid abstract transitions
    validate_transitions(&mut result, concrete, abstract_spec)?;

    // Check for unreachable abstract states
    validate_abstract_reachability(&mut result, concrete, abstract_spec)?;

    Ok(result)
}

/// Build the concrete-to-abstract state mapping.
fn build_refinement_map(
    concrete: &BehaviorDecl,
    abstract_spec: &BehaviorDecl,
    explicit_map: &Option<RefinementMap>,
) -> Result<RefinementMapInfo> {
    let mut concrete_to_abstract: HashMap<String, String> = HashMap::new();
    let mut covered_abstract_states: HashSet<String> = HashSet::new();

    // Collect abstract state names for lookup
    let abstract_state_names: HashSet<&str> =
        abstract_spec.states.iter().map(|s| s.name.as_str()).collect();

    // If there's an explicit map, use it
    if let Some(ref map) = explicit_map {
        for (abstract_state, concrete_states) in &map.mappings {
            if !abstract_state_names.contains(abstract_state.as_str()) {
                return Err(anyhow!(
                    "Abstract state '{}' not found in abstract specification",
                    abstract_state
                ));
            }

            for concrete_state in concrete_states {
                concrete_to_abstract.insert(concrete_state.clone(), abstract_state.clone());
                covered_abstract_states.insert(abstract_state.clone());
            }
        }
    }

    // Infer remaining mappings by name matching
    for concrete_state in &concrete.states {
        if !concrete_to_abstract.contains_key(&concrete_state.name) {
            // Try exact name match
            if abstract_state_names.contains(concrete_state.name.as_str()) {
                concrete_to_abstract.insert(
                    concrete_state.name.clone(),
                    concrete_state.name.clone(),
                );
                covered_abstract_states.insert(concrete_state.name.clone());
            }
        }
    }

    // Find unreachable abstract states
    let unreachable_abstract_states: Vec<String> = abstract_spec
        .states
        .iter()
        .filter(|s| !covered_abstract_states.contains(&s.name))
        .map(|s| s.name.clone())
        .collect();

    Ok(RefinementMapInfo {
        concrete_to_abstract,
        covered_abstract_states,
        unreachable_abstract_states,
    })
}

/// Validate that all concrete states have a corresponding abstract state.
fn validate_state_coverage(
    result: &mut RefinementResult,
    concrete: &BehaviorDecl,
    _abstract_spec: &BehaviorDecl,
) -> Result<()> {
    for state in &concrete.states {
        if !result.map.concrete_to_abstract.contains_key(&state.name) {
            result.violations.push(RefinementViolation::UnmappedConcreteState {
                state: state.name.clone(),
            });
            result.is_valid = false;
        }
    }
    Ok(())
}

/// Validate that concrete transitions correspond to valid abstract transitions.
fn validate_transitions(
    result: &mut RefinementResult,
    concrete: &BehaviorDecl,
    abstract_spec: &BehaviorDecl,
) -> Result<()> {
    // Build abstract transition lookup: (from_state, event) -> [to_states]
    let mut abstract_transitions: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
    for t in &abstract_spec.transitions {
        if let (Some(from), Some(to)) = (t.from.as_state(), t.to.as_state()) {
            abstract_transitions
                .entry((from, &t.on_event))
                .or_insert_with(Vec::new)
                .push(to);
        }
    }

    // Check each concrete transition
    for concrete_trans in &concrete.transitions {
        let concrete_from = match concrete_trans.from.as_state() {
            Some(s) => s.to_string(),
            None => continue, // Skip non-simple transitions
        };
        let concrete_to = match concrete_trans.to.as_state() {
            Some(s) => s.to_string(),
            None => continue, // Skip non-simple transitions
        };
        let event = &concrete_trans.on_event;

        // Get abstract states for concrete endpoints
        let abstract_from = match result.map.concrete_to_abstract.get(&concrete_from) {
            Some(s) => s,
            None => continue, // Already reported as unmapped
        };

        let abstract_to = match result.map.concrete_to_abstract.get(&concrete_to) {
            Some(s) => s,
            None => continue, // Already reported as unmapped
        };

        // Check if this is a stuttering step (staying in same abstract state)
        if abstract_from == abstract_to {
            // Stuttering is always valid in refinement
            continue;
        }

        // Check if abstract transition exists
        let valid_abstract_targets = abstract_transitions.get(&(&abstract_from[..], &event[..]));

        match valid_abstract_targets {
            Some(targets) => {
                if !targets.iter().any(|t| *t == abstract_to) {
                    result.violations.push(RefinementViolation::IllegalTransition {
                        from: concrete_from.clone(),
                        to: concrete_to.clone(),
                        event: event.clone(),
                        reason: format!(
                            "Abstract transition from {} on {} goes to {:?}, not to {}",
                            abstract_from, event, targets, abstract_to
                        ),
                    });
                    result.is_valid = false;
                }
            }
            None => {
                result.violations.push(RefinementViolation::IllegalTransition {
                    from: concrete_from.clone(),
                    to: concrete_to.clone(),
                    event: event.clone(),
                    reason: format!(
                        "No abstract transition from {} on event {}",
                        abstract_from, event
                    ),
                });
                result.is_valid = false;
            }
        }
    }

    Ok(())
}

/// Validate that all abstract states are reachable via concrete states.
fn validate_abstract_reachability(
    result: &mut RefinementResult,
    concrete: &BehaviorDecl,
    abstract_spec: &BehaviorDecl,
) -> Result<()> {
    // Report unreachable abstract states
    for state in &result.map.unreachable_abstract_states {
        result
            .violations
            .push(RefinementViolation::UnreachableAbstractState {
                state: state.clone(),
            });
        result.is_valid = false;
    }

    // Check that initial states map correctly
    let concrete_initial: Vec<&StateDecl> = concrete
        .states
        .iter()
        .filter(|s| s.initial)
        .collect();
    let abstract_initial: Vec<&StateDecl> = abstract_spec
        .states
        .iter()
        .filter(|s| s.initial)
        .collect();

    if !concrete_initial.is_empty() && !abstract_initial.is_empty() {
        // Each concrete initial state should map to an abstract initial state
        for c_init in &concrete_initial {
            if let Some(abstract_mapped) = result.map.concrete_to_abstract.get(&c_init.name) {
                let is_abstract_initial = abstract_initial
                    .iter()
                    .any(|a| &a.name == abstract_mapped);
                if !is_abstract_initial {
                    result.violations.push(RefinementViolation::IllegalTransition {
                        from: c_init.name.clone(),
                        to: abstract_mapped.clone(),
                        event: "init".to_string(),
                        reason: format!(
                            "Concrete initial state {} maps to non-initial abstract state {}",
                            c_init.name, abstract_mapped
                        ),
                    });
                    result.is_valid = false;
                }
            }
        }
    }

    Ok(())
}

impl RefinementResult {
    /// Get a summary of the validation result.
    pub fn summary(&self) -> String {
        if self.is_valid {
            format!(
                "✓ {} correctly refines {}",
                self.concrete_behavior, self.abstract_spec
            )
        } else {
            format!(
                "✗ {} does NOT correctly refine {} ({} violations)",
                self.concrete_behavior,
                self.abstract_spec,
                self.violations.len()
            )
        }
    }

    /// Get violations of a specific type.
    pub fn violations_of_type(&self, violation_type: ViolationType) -> Vec<&RefinementViolation> {
        self.violations
            .iter()
            .filter(|v| match violation_type {
                ViolationType::UnmappedState => {
                    matches!(v, RefinementViolation::UnmappedConcreteState { .. })
                }
                ViolationType::UnreachableAbstract => {
                    matches!(v, RefinementViolation::UnreachableAbstractState { .. })
                }
                ViolationType::IllegalTransition => {
                    matches!(v, RefinementViolation::IllegalTransition { .. })
                }
                ViolationType::InconsistentMapping => {
                    matches!(v, RefinementViolation::InconsistentMapping { .. })
                }
            })
            .collect()
    }
}

/// Types of refinement violations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViolationType {
    UnmappedState,
    UnreachableAbstract,
    IllegalTransition,
    InconsistentMapping,
}

/// Compute a refinement map from a system (for integration with existing code).
pub fn compute_refinement(
    system: &crate::parser::ast::SystemDecl,
) -> Option<ComputedRefinement> {
    // Find behaviors with refinement specifications
    let mut refinements = Vec::new();

    for behavior in &system.behaviors {
        if let Some(ref abstract_spec) = behavior.refines {
            refinements.push((behavior.name.clone(), abstract_spec.clone()));
        }
    }

    // Also check component behaviors
    for component in &system.components {
        for behavior in &component.behaviors {
            if let Some(ref abstract_spec) = behavior.refines {
                refinements.push((behavior.name.clone(), abstract_spec.clone()));
            }
        }
    }

    if refinements.is_empty() {
        return None;
    }

    // For now, return the first refinement found
    // A more complete implementation would handle multiple refinements
    let (_concrete_name, abstract_name) = refinements.into_iter().next()?;

    Some(ComputedRefinement {
        system_name: system.name.clone(),
        abstract_spec: abstract_name,
        mappings: HashMap::new(),
        inferred: vec![],
        explicit: vec![],
    })
}

/// A computed refinement map (for compatibility with existing code).
#[derive(Debug, Clone)]
pub struct ComputedRefinement {
    pub system_name: String,
    pub abstract_spec: String,
    pub mappings: HashMap<String, Vec<String>>,
    pub inferred: Vec<String>,
    pub explicit: Vec<String>,
}

/// Generate a TLA+ refinement proof obligation.
pub fn generate_refinement_tla(
    refinement: &ComputedRefinement,
    concrete_states: &[String],
    _concrete_initial: &str,
    _concrete_transitions: &[(String, String)],
    _output_dir: &std::path::Path,
) -> Result<String> {
    let module_name = format!("{}_Refines_{}", refinement.system_name, refinement.abstract_spec);

    let mut tla = String::new();
    tla.push_str(&format!("---- MODULE {} ----\n", module_name));
    tla.push_str("EXTENDS Integers, Sequences, TLC\n\n");

    // Concrete state abstraction function
    tla.push_str("\\* Abstraction function: concrete -> abstract\n");
    tla.push_str("Abs(concrete_state) ==\n");
    tla.push_str("  CASE concrete_state = \"initial\" -> \"Init\"\n");
    for (i, state) in concrete_states.iter().enumerate() {
        tla.push_str(&format!(
            "    [] concrete_state = \"{}\" -> \"State{}\"\n",
            state, i
        ));
    }
    tla.push_str("\n");

    // Refinement theorem
    tla.push_str("\\* Refinement theorem\n");
    tla.push_str("THEOREM RefinementCorrect ==\n");
    tla.push_str("  Init /\\ [][Next]_vars => Spec_Abs\n\n");

    tla.push_str("====\n");

    Ok(tla)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Span, TransitionDecl, TransitionSource, TransitionTarget};

    fn make_behavior(
        name: &str,
        states: Vec<(&str, bool, bool)>,
        transitions: Vec<(&str, &str, &str)>,
    ) -> BehaviorDecl {
        BehaviorDecl {
            name: name.to_string(),
            states: states
                .into_iter()
                .map(|(n, init, term)| StateDecl {
                    name: n.to_string(),
                    initial: init,
                    terminal: term,
                    parent: None,
                    substates: Vec::new(),
                    entry_actions: Vec::new(),
                    exit_actions: Vec::new(),
                })
                .collect(),
            transitions: transitions
                .into_iter()
                .map(|(from, to, event)| TransitionDecl {
                    from: TransitionSource::State(from.to_string()),
                    to: TransitionTarget::State(to.to_string()),
                    on_event: event.to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: Span::synthetic(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_refinement() {
        // Abstract: idle -> active -> done
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("idle", true, false), ("active", false, false), ("done", false, true)],
            vec![("idle", "active", "start"), ("active", "done", "finish")],
        );

        // Concrete: idle -> processing -> verifying -> done
        // Maps: idle->idle, processing->active, verifying->active, done->done
        let concrete = make_behavior(
            "Concrete",
            vec![
                ("idle", true, false),
                ("processing", false, false),
                ("verifying", false, false),
                ("done", false, true),
            ],
            vec![
                ("idle", "processing", "start"),
                ("processing", "verifying", "verify"),
                ("verifying", "done", "finish"),
            ],
        );

        let explicit_map = RefinementMap {
            mappings: vec![
                ("idle".to_string(), vec!["idle".to_string()]),
                ("active".to_string(), vec!["processing".to_string(), "verifying".to_string()]),
                ("done".to_string(), vec!["done".to_string()]),
            ],
        };

        let result =
            validate_refinement(&concrete, &abstract_spec, &Some(explicit_map)).unwrap();

        assert!(result.is_valid, "Violations: {:?}", result.violations);
        assert_eq!(result.violations.len(), 0);
    }

    #[test]
    fn test_stuttering_step() {
        // Abstract: just has idle and done
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "finish")],
        );

        // Concrete: has internal steps that stay in same abstract state
        let concrete = make_behavior(
            "Concrete",
            vec![("idle", true, false), ("preparing", false, false), ("done", false, true)],
            vec![
                ("idle", "preparing", "internal"), // stuttering (both map to idle)
                ("preparing", "done", "finish"),   // valid abstract transition
            ],
        );

        let explicit_map = RefinementMap {
            mappings: vec![
                ("idle".to_string(), vec!["idle".to_string(), "preparing".to_string()]),
                ("done".to_string(), vec!["done".to_string()]),
            ],
        };

        let result =
            validate_refinement(&concrete, &abstract_spec, &Some(explicit_map)).unwrap();

        assert!(result.is_valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_unmapped_concrete_state() {
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("a", true, false)],
            vec![],
        );

        let concrete = make_behavior(
            "Concrete",
            vec![("a", true, false), ("b", false, false)], // b is unmapped
            vec![],
        );

        let result = validate_refinement(&concrete, &abstract_spec, &None).unwrap();

        assert!(!result.is_valid);
        let unmapped = result.violations_of_type(ViolationType::UnmappedState);
        assert_eq!(unmapped.len(), 1);
    }

    #[test]
    fn test_illegal_transition() {
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("a", true, false), ("b", false, false)],
            vec![("a", "b", "go")],
        );

        // Concrete tries to do a transition that doesn't exist in abstract
        let concrete = make_behavior(
            "Concrete",
            vec![("a", true, false), ("b", false, false)],
            vec![("a", "b", "different_event")], // event name doesn't match
        );

        let result = validate_refinement(&concrete, &abstract_spec, &None).unwrap();

        assert!(!result.is_valid);
        let illegal = result.violations_of_type(ViolationType::IllegalTransition);
        assert!(illegal.len() > 0);
    }

    #[test]
    fn test_unreachable_abstract_state() {
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("a", true, false), ("b", false, false), ("unreachable", false, false)],
            vec![("a", "b", "go")],
        );

        let concrete = make_behavior(
            "Concrete",
            vec![("a", true, false), ("b", false, false)],
            vec![("a", "b", "go")],
        );

        let result = validate_refinement(&concrete, &abstract_spec, &None).unwrap();

        assert!(!result.is_valid);
        let unreachable = result.violations_of_type(ViolationType::UnreachableAbstract);
        assert_eq!(unreachable.len(), 1);
    }

    #[test]
    fn test_inferred_mapping() {
        // Same state names - should infer mapping automatically
        let abstract_spec = make_behavior(
            "Abstract",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "go")],
        );

        let concrete = make_behavior(
            "Concrete",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "go")],
        );

        let result = validate_refinement(&concrete, &abstract_spec, &None).unwrap();

        assert!(result.is_valid, "Violations: {:?}", result.violations);
        assert_eq!(result.map.concrete_to_abstract.len(), 2);
    }
}
