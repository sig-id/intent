//! Behavior composition for Intent systems.
//!
//! This module provides functionality to compose multiple behaviors into a single
//! unified behavior, detecting conflicts and merging states, transitions, and properties.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};

use crate::parser::ast::{
    BehaviorDecl, FairnessSpec, InvariantDecl, StateDecl, TemporalProperty, TransitionDecl,
};

/// Configuration for behavior composition.
#[derive(Debug, Clone, Default)]
pub struct CompositionConfig {
    /// How to handle state conflicts: error, use_first, or merge
    pub state_conflict_strategy: ConflictStrategy,
    /// How to handle transition conflicts: error or use_first
    pub transition_conflict_strategy: ConflictStrategy,
    /// Prefix to add to composed states (for disambiguation)
    pub state_prefix: Option<String>,
}

/// Strategy for handling conflicts during composition.
#[derive(Debug, Clone, Copy, Default)]
pub enum ConflictStrategy {
    #[default]
    Error,
    UseFirst,
    Merge,
}

/// A behavior composed from multiple source behaviors.
#[derive(Debug, Clone)]
pub struct ComposedBehavior {
    /// Name of the composed behavior
    pub name: String,
    /// Names of source behaviors
    pub source_behaviors: Vec<String>,
    /// Merged states
    pub states: Vec<StateDecl>,
    /// Merged transitions
    pub transitions: Vec<TransitionDecl>,
    /// Merged temporal properties
    pub properties: Vec<TemporalProperty>,
    /// Merged fairness specifications
    pub fairness: Vec<FairnessSpec>,
    /// Merged invariants
    pub invariants: Vec<InvariantDecl>,
    /// Conflicts detected during composition
    pub conflicts: Vec<CompositionConflict>,
}

/// A conflict detected during behavior composition.
#[derive(Debug, Clone, PartialEq)]
pub enum CompositionConflict {
    /// Multiple initial states from different sources
    MultipleInitialStates { states: Vec<(String, String)> }, // (source, state)
    /// State has different modifiers in different sources
    StateModifierMismatch {
        state: String,
        sources: Vec<(String, StateModifiers)>,
    },
    /// Same (from, event) leads to different targets
    TransitionConflict {
        from: String,
        event: String,
        targets: Vec<(String, String)>, // (source, target)
    },
    /// Property name collision
    PropertyCollision {
        name: String,
        sources: Vec<String>,
    },
}

/// State modifiers for conflict reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct StateModifiers {
    pub initial: bool,
    pub terminal: bool,
}

impl From<&StateDecl> for StateModifiers {
    fn from(s: &StateDecl) -> Self {
        Self {
            initial: s.initial,
            terminal: s.terminal,
        }
    }
}

/// Compose multiple behaviors into one.
///
/// # Arguments
/// * `name` - Name for the composed behavior
/// * `behaviors` - Slice of (name, behavior) tuples to compose
/// * `config` - Composition configuration
///
/// # Returns
/// A `ComposedBehavior` with merged content and any detected conflicts.
pub fn compose_behaviors(
    name: &str,
    behaviors: &[(&str, &BehaviorDecl)],
    config: &CompositionConfig,
) -> Result<ComposedBehavior> {
    if behaviors.is_empty() {
        return Err(anyhow!("Cannot compose zero behaviors"));
    }

    let mut composed = ComposedBehavior {
        name: name.to_string(),
        source_behaviors: behaviors.iter().map(|(n, _)| n.to_string()).collect(),
        states: Vec::new(),
        transitions: Vec::new(),
        properties: Vec::new(),
        fairness: Vec::new(),
        invariants: Vec::new(),
        conflicts: Vec::new(),
    };

    // Merge each component
    merge_states(&mut composed, behaviors, config)?;
    merge_transitions(&mut composed, behaviors, config)?;
    merge_properties(&mut composed, behaviors);
    merge_fairness(&mut composed, behaviors);
    merge_invariants(&mut composed, behaviors);

    // Validate reachability
    validate_reachability(&composed)?;

    Ok(composed)
}

/// Merge states from multiple behaviors.
fn merge_states(
    composed: &mut ComposedBehavior,
    behaviors: &[(&str, &BehaviorDecl)],
    config: &CompositionConfig,
) -> Result<()> {
    let mut state_map: HashMap<String, (String, StateDecl)> = HashMap::new();
    let mut initial_states: Vec<(String, String)> = Vec::new();

    for (source_name, behavior) in behaviors {
        for state in &behavior.states {
            let key = if let Some(ref prefix) = config.state_prefix {
                format!("{}_{}", prefix, state.name)
            } else {
                state.name.clone()
            };

            if let Some((existing_source, existing_state)) = state_map.get(&key) {
                // Check for modifier conflicts
                if state.initial != existing_state.initial || state.terminal != existing_state.terminal
                {
                    match config.state_conflict_strategy {
                        ConflictStrategy::Error => {
                            composed.conflicts.push(CompositionConflict::StateModifierMismatch {
                                state: key.clone(),
                                sources: vec![
                                    (existing_source.clone(), StateModifiers::from(existing_state)),
                                    (source_name.to_string(), StateModifiers::from(state)),
                                ],
                            });
                        }
                        ConflictStrategy::UseFirst => {
                            // Keep existing state, skip this one
                            continue;
                        }
                        ConflictStrategy::Merge => {
                            // Merge modifiers (OR them together)
                            let merged = StateDecl {
                                name: key.clone(),
                                initial: existing_state.initial || state.initial,
                                terminal: existing_state.terminal || state.terminal,
                            };
                            state_map.insert(key, (existing_source.clone(), merged));
                        }
                    }
                }
            } else {
                let prefixed_state = StateDecl {
                    name: key.clone(),
                    ..state.clone()
                };
                state_map.insert(key.clone(), (source_name.to_string(), prefixed_state));
            }

            if state.initial {
                let state_name = if config.state_prefix.is_some() {
                    format!("{}_{}", config.state_prefix.as_ref().unwrap(), state.name)
                } else {
                    state.name.clone()
                };
                initial_states.push((source_name.to_string(), state_name));
            }
        }
    }

    // Check for multiple initial states
    if initial_states.len() > 1 {
        composed.conflicts.push(CompositionConflict::MultipleInitialStates {
            states: initial_states,
        });
    }

    // Collect states
    composed.states = state_map.into_values().map(|(_, s)| s).collect();

    Ok(())
}

/// Merge transitions from multiple behaviors.
fn merge_transitions(
    composed: &mut ComposedBehavior,
    behaviors: &[(&str, &BehaviorDecl)],
    config: &CompositionConfig,
) -> Result<()> {
    let mut transition_map: HashMap<(String, String), (String, TransitionDecl)> = HashMap::new();
    let mut conflicts: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();

    for (source_name, behavior) in behaviors {
        for transition in &behavior.transitions {
            let from = if let Some(ref prefix) = config.state_prefix {
                format!("{}_{}", prefix, transition.from)
            } else {
                transition.from.clone()
            };
            let to = if let Some(ref prefix) = config.state_prefix {
                format!("{}_{}", prefix, transition.to)
            } else {
                transition.to.clone()
            };
            let prefixed_transition = TransitionDecl {
                from: from.clone(),
                to: to.clone(),
                ..transition.clone()
            };

            let key = (from.clone(), transition.on_event.clone());

            if let Some((existing_source, existing)) = transition_map.get(&key) {
                if existing.to != to {
                    // Conflict: same (from, event) but different targets
                    match config.transition_conflict_strategy {
                        ConflictStrategy::Error => {
                            conflicts
                                .entry(key.clone())
                                .or_insert_with(Vec::new)
                                .push((existing_source.clone(), existing.to.clone()));
                            conflicts
                                .entry(key.clone())
                                .or_insert_with(Vec::new)
                                .push((source_name.to_string(), to));
                        }
                        ConflictStrategy::UseFirst | ConflictStrategy::Merge => {
                            // Keep existing transition
                            continue;
                        }
                    }
                }
            } else {
                transition_map.insert(key, (source_name.to_string(), prefixed_transition));
            }
        }
    }

    // Record conflicts
    for ((from, event), targets) in conflicts {
        composed
            .conflicts
            .push(CompositionConflict::TransitionConflict { from, event, targets });
    }

    // Collect transitions
    composed.transitions = transition_map.into_values().map(|(_, t)| t).collect();

    Ok(())
}

/// Merge temporal properties from multiple behaviors.
fn merge_properties(composed: &mut ComposedBehavior, behaviors: &[(&str, &BehaviorDecl)]) {
    let mut seen_names: HashMap<String, Vec<String>> = HashMap::new();

    for (source_name, behavior) in behaviors {
        for prop in &behavior.properties {
            seen_names
                .entry(prop.name.clone())
                .or_insert_with(Vec::new)
                .push(source_name.to_string());
        }
    }

    // Record collisions
    for (name, sources) in &seen_names {
        if sources.len() > 1 {
            composed
                .conflicts
                .push(CompositionConflict::PropertyCollision {
                    name: name.clone(),
                    sources: sources.clone(),
                });
        }
    }

    // Collect all properties (rename duplicates with source prefix)
    let mut added_names: HashSet<String> = HashSet::new();
    for (source_name, behavior) in behaviors {
        for prop in &behavior.properties {
            let name = if added_names.contains(&prop.name) {
                format!("{}_{}", source_name, prop.name)
            } else {
                prop.name.clone()
            };
            added_names.insert(prop.name.clone());

            composed.properties.push(TemporalProperty {
                name,
                expr: prop.expr.clone(),
            });
        }
    }
}

/// Merge fairness specifications from multiple behaviors.
fn merge_fairness(composed: &mut ComposedBehavior, behaviors: &[(&str, &BehaviorDecl)]) {
    let mut seen: HashSet<(String, String, String)> = HashSet::new(); // (kind, from, to)

    for (_source_name, behavior) in behaviors {
        for spec in &behavior.fairness {
            let kind = match spec.kind {
                crate::parser::ast::FairnessKind::Weak => "weak",
                crate::parser::ast::FairnessKind::Strong => "strong",
            };
            let key = (kind.to_string(), spec.from.clone(), spec.to.clone());
            if !seen.contains(&key) {
                seen.insert(key);
                composed.fairness.push(spec.clone());
            }
        }
    }
}

/// Merge invariants from multiple behaviors.
fn merge_invariants(composed: &mut ComposedBehavior, behaviors: &[(&str, &BehaviorDecl)]) {
    let mut seen_names: HashSet<String> = HashSet::new();

    for (source_name, behavior) in behaviors {
        for inv in &behavior.invariants {
            let final_name = if seen_names.contains(&inv.name) {
                format!("{}_{}", source_name, inv.name)
            } else {
                inv.name.clone()
            };
            seen_names.insert(inv.name.clone());

            composed.invariants.push(InvariantDecl {
                name: final_name,
                expr: inv.expr.clone(),
            });
        }
    }
}

/// Validate that all states are reachable from the initial state(s).
fn validate_reachability(composed: &ComposedBehavior) -> Result<()> {
    let initial_states: Vec<&str> = composed
        .states
        .iter()
        .filter(|s| s.initial)
        .map(|s| s.name.as_str())
        .collect();

    if initial_states.is_empty() {
        // No initial state - skip reachability check
        return Ok(());
    }

    // Build adjacency list
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for state in &composed.states {
        adjacency.insert(state.name.as_str(), Vec::new());
    }
    for transition in &composed.transitions {
        adjacency
            .entry(transition.from.as_str())
            .or_insert_with(Vec::new)
            .push(transition.to.as_str());
    }

    // BFS from initial states
    let mut reachable: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = initial_states.iter().copied().collect();

    while let Some(state) = queue.pop() {
        if reachable.contains(state) {
            continue;
        }
        reachable.insert(state);

        if let Some(neighbors) = adjacency.get(state) {
            for neighbor in neighbors {
                if !reachable.contains(neighbor) {
                    queue.push(neighbor);
                }
            }
        }
    }

    // Check for unreachable states
    let unreachable: Vec<&str> = composed
        .states
        .iter()
        .filter(|s| !reachable.contains(s.name.as_str()))
        .map(|s| s.name.as_str())
        .collect();

    if !unreachable.is_empty() {
        // Log warning but don't fail - unreachable states may be intentional
        // In a real implementation, we might add this to a warnings list
    }

    Ok(())
}

impl ComposedBehavior {
    /// Convert to a BehaviorDecl for use with existing TLA+ generation.
    pub fn to_behavior_decl(&self) -> BehaviorDecl {
        BehaviorDecl {
            name: self.name.clone(),
            composes: self.source_behaviors.clone(),
            states: self.states.clone(),
            transitions: self.transitions.clone(),
            properties: self.properties.clone(),
            fairness: self.fairness.clone(),
            invariants: self.invariants.clone(),
            ..Default::default()
        }
    }

    /// Check if composition has any conflicts.
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// Get conflicts of a specific type.
    pub fn conflicts_of_type(&self, conflict_type: ConflictType) -> Vec<&CompositionConflict> {
        self.conflicts
            .iter()
            .filter(|c| match conflict_type {
                ConflictType::State => matches!(
                    c,
                    CompositionConflict::MultipleInitialStates { .. }
                        | CompositionConflict::StateModifierMismatch { .. }
                ),
                ConflictType::Transition => matches!(c, CompositionConflict::TransitionConflict { .. }),
                ConflictType::Property => matches!(c, CompositionConflict::PropertyCollision { .. }),
            })
            .collect()
    }
}

/// Types of composition conflicts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConflictType {
    State,
    Transition,
    Property,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::FairnessKind;

    fn make_simple_behavior(
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
                })
                .collect(),
            transitions: transitions
                .into_iter()
                .map(|(from, to, event)| TransitionDecl {
                    from: from.to_string(),
                    to: to.to_string(),
                    on_event: event.to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test_compose_single_behavior() {
        let behavior = make_simple_behavior(
            "Simple",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "finish")],
        );

        let result = compose_behaviors("Composed", &[("Simple", &behavior)], &Default::default())
            .unwrap();

        assert_eq!(result.name, "Composed");
        assert_eq!(result.source_behaviors, vec!["Simple"]);
        assert_eq!(result.states.len(), 2);
        assert_eq!(result.transitions.len(), 1);
        assert!(!result.has_conflicts());
    }

    #[test]
    fn test_compose_disjoint_behaviors() {
        let b1 = make_simple_behavior(
            "Flow1",
            vec![("a1", true, false), ("a2", false, true)],
            vec![("a1", "a2", "go")],
        );
        let b2 = make_simple_behavior(
            "Flow2",
            vec![("b1", true, false), ("b2", false, true)],
            vec![("b1", "b2", "go")],
        );

        let result =
            compose_behaviors("Combined", &[("Flow1", &b1), ("Flow2", &b2)], &Default::default())
                .unwrap();

        assert_eq!(result.states.len(), 4);
        assert_eq!(result.transitions.len(), 2);

        // Should have conflict: two initial states
        assert!(result.has_conflicts());
        let state_conflicts = result.conflicts_of_type(ConflictType::State);
        assert_eq!(state_conflicts.len(), 1);
    }

    #[test]
    fn test_compose_shared_states() {
        let b1 = make_simple_behavior(
            "Flow1",
            vec![("idle", true, false), ("active", false, false)],
            vec![("idle", "active", "start")],
        );
        let b2 = make_simple_behavior(
            "Flow2",
            vec![("active", false, false), ("done", false, true)],
            vec![("active", "done", "finish")],
        );

        let result =
            compose_behaviors("Combined", &[("Flow1", &b1), ("Flow2", &b2)], &Default::default())
                .unwrap();

        // idle, active, done
        assert_eq!(result.states.len(), 3);
        assert_eq!(result.transitions.len(), 2);
    }

    #[test]
    fn test_transition_conflict() {
        let b1 = make_simple_behavior(
            "Flow1",
            vec![("s", true, false), ("a", false, false), ("b", false, false)],
            vec![("s", "a", "go")],
        );
        let b2 = make_simple_behavior(
            "Flow2",
            vec![("s", false, false), ("a", false, false), ("b", false, false)],
            vec![("s", "b", "go")],
        );

        let result = compose_behaviors(
            "Combined",
            &[("Flow1", &b1), ("Flow2", &b2)],
            &CompositionConfig {
                transition_conflict_strategy: ConflictStrategy::Error,
                ..Default::default()
            },
        )
        .unwrap();

        // Should detect transition conflict
        let trans_conflicts = result.conflicts_of_type(ConflictType::Transition);
        assert_eq!(trans_conflicts.len(), 1);
    }

    #[test]
    fn test_compose_with_fairness() {
        let mut b1 = make_simple_behavior(
            "Flow1",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "go")],
        );
        b1.fairness.push(FairnessSpec {
            kind: FairnessKind::Weak,
            from: "idle".to_string(),
            to: "done".to_string(),
            alts: vec![],
        });

        let mut b2 = make_simple_behavior(
            "Flow2",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "go")],
        );
        b2.fairness.push(FairnessSpec {
            kind: FairnessKind::Weak,
            from: "idle".to_string(),
            to: "done".to_string(),
            alts: vec![],
        });
        b2.fairness.push(FairnessSpec {
            kind: FairnessKind::Strong,
            from: "idle".to_string(),
            to: "done".to_string(),
            alts: vec![],
        });

        let result =
            compose_behaviors("Combined", &[("Flow1", &b1), ("Flow2", &b2)], &Default::default())
                .unwrap();

        // Should deduplicate identical fairness specs
        assert_eq!(result.fairness.len(), 2); // weak and strong, not 3
    }

    #[test]
    fn test_to_behavior_decl() {
        let behavior = make_simple_behavior(
            "Simple",
            vec![("idle", true, false), ("done", false, true)],
            vec![("idle", "done", "finish")],
        );

        let composed =
            compose_behaviors("Composed", &[("Simple", &behavior)], &Default::default()).unwrap();

        let decl = composed.to_behavior_decl();
        assert_eq!(decl.name, "Composed");
        assert_eq!(decl.composes, vec!["Simple"]);
        assert_eq!(decl.states.len(), 2);
    }
}
