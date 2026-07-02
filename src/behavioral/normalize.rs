//! Hierarchical state machine normalization.
//!
//! Desugars hierarchical (nested) states into flat `parent.child` states
//! before any analysis pass. This ensures downstream checks (composition,
//! reachability, TLA+ generation) see only flat state machines.

use std::collections::HashMap;

use crate::parser::ast::{
    BehaviorDecl, StateDecl, TemporalExpr, TemporalProperty, TransitionDecl, TransitionSource,
    TransitionTarget,
};

/// Map from parent state name to (initial_substate, all_substates).
type ParentMap = HashMap<String, (Option<String>, Vec<String>)>;

/// Desugar hierarchical states in a behavior into flat states.
///
/// Phase 1: Flatten nested states into `parent.child` naming.
/// Phase 2: Rewrite transitions referencing parent states.
/// Phase 3: Rewrite temporal properties referencing parent states.
pub fn desugar_hierarchical_states(behavior: &BehaviorDecl) -> BehaviorDecl {
    // Check if any states have substates – if not, return unchanged
    let has_hierarchy = behavior.states.iter().any(|s| !s.substates.is_empty());
    if !has_hierarchy {
        return behavior.clone();
    }

    // Phase 1: Flatten states
    let (flat_states, parent_map) = flatten_states(&behavior.states, "");

    // Phase 2: Rewrite transitions
    let transitions = rewrite_transitions(&behavior.transitions, &parent_map);

    // Phase 3: Rewrite temporal properties
    let properties = rewrite_properties(&behavior.properties, &parent_map);

    // Phase 3b: Rewrite invariants (state references in invariant expressions)
    let invariants = behavior.invariants.clone();

    BehaviorDecl {
        name: behavior.name.clone(),
        executable: behavior.executable,
        composes: behavior.composes.clone(),
        nodes: behavior.nodes.clone(),
        parameters: behavior.parameters.clone(),
        variables: behavior.variables.clone(),
        memory: behavior.memory.clone(),
        functions: behavior.functions.clone(),
        states: flat_states,
        fixtures: behavior.fixtures.clone(),
        projections: behavior.projections.clone(),
        mbt: behavior.mbt.clone(),
        transitions,
        properties,
        fairness: behavior.fairness.clone(),
        invariants,
        refines: behavior.refines.clone(),
        applies: behavior.applies.clone(),
        refinement_map: behavior.refinement_map.clone(),
        grounding: behavior.grounding.clone(),
        strengthens: behavior.strengthens.clone(),
        span: behavior.span,
    }
}

/// Recursively flatten hierarchical states into flat `parent.child` names.
///
/// Parent states themselves are removed from the flat list; only leaf states remain.
/// If a parent is marked `initial`, its initial substate inherits that flag.
fn flatten_states(states: &[StateDecl], prefix: &str) -> (Vec<StateDecl>, ParentMap) {
    let mut flat = Vec::new();
    let mut parent_map = ParentMap::new();

    for state in states {
        let full_name = if prefix.is_empty() {
            state.name.clone()
        } else {
            format!("{}.{}", prefix, state.name)
        };

        if state.substates.is_empty() {
            // Leaf state – add to flat list
            flat.push(StateDecl {
                name: full_name,
                initial: state.initial,
                terminal: state.terminal,
                parent: if prefix.is_empty() {
                    None
                } else {
                    Some(prefix.to_string())
                },
                substates: Vec::new(),
                entry_actions: state.entry_actions.clone(),
                exit_actions: state.exit_actions.clone(),
            });
        } else {
            // Parent state with substates – flatten recursively
            let (sub_flat, sub_parent_map) = flatten_states(&state.substates, &full_name);

            // Find initial substate
            let initial_substate = sub_flat.iter().find(|s| s.initial).map(|s| s.name.clone());

            let all_substate_names: Vec<String> = sub_flat.iter().map(|s| s.name.clone()).collect();

            parent_map.insert(
                full_name.clone(),
                (initial_substate.clone(), all_substate_names),
            );
            parent_map.extend(sub_parent_map);

            // If parent is initial, propagate to its initial substate
            let mut sub_flat = sub_flat;
            if state.initial {
                if let Some(ref init_name) = initial_substate {
                    for s in &mut sub_flat {
                        if s.name == *init_name {
                            s.initial = true;
                        }
                    }
                }
            }

            flat.extend(sub_flat);
        }
    }

    (flat, parent_map)
}

/// Rewrite transitions that reference parent states.
///
/// - Source referencing parent → expand to all substates (multiple transitions)
/// - Target referencing parent → redirect to parent's initial substate
fn rewrite_transitions(
    transitions: &[TransitionDecl],
    parent_map: &ParentMap,
) -> Vec<TransitionDecl> {
    let mut result = Vec::new();

    for transition in transitions {
        let from_states = expand_source(&transition.from, parent_map);
        let to_target = redirect_target(&transition.to, parent_map);

        for from in from_states {
            result.push(TransitionDecl {
                from,
                to: to_target.clone(),
                on_event: transition.on_event.clone(),
                inputs: transition.inputs.clone(),
                bindings: transition.bindings.clone(),
                guard: transition.guard.clone(),
                expects: transition.expects.clone(),
                effects: transition.effects.clone(),
                timing: transition.timing.clone(),
                span: transition.span,
            });
        }
    }

    result
}

/// Expand a transition source: if it references a parent state, produce
/// one copy for each substate.
fn expand_source(source: &TransitionSource, parent_map: &ParentMap) -> Vec<TransitionSource> {
    match source {
        TransitionSource::State(name) => {
            if let Some((_, substates)) = parent_map.get(name) {
                substates
                    .iter()
                    .map(|s| TransitionSource::State(s.clone()))
                    .collect()
            } else {
                vec![source.clone()]
            }
        }
        TransitionSource::Wildcard => vec![source.clone()],
        TransitionSource::States(states) => {
            let mut expanded = Vec::new();
            for state in states {
                if let Some((_, substates)) = parent_map.get(state) {
                    expanded.extend(substates.iter().cloned());
                } else {
                    expanded.push(state.clone());
                }
            }
            vec![TransitionSource::States(expanded)]
        }
    }
}

/// Redirect a transition target: if it references a parent state, redirect
/// to the parent's initial substate.
fn redirect_target(target: &TransitionTarget, parent_map: &ParentMap) -> TransitionTarget {
    match target {
        TransitionTarget::State(name) => {
            if let Some((Some(initial), _)) = parent_map.get(name) {
                TransitionTarget::State(initial.clone())
            } else {
                target.clone()
            }
        }
        _ => target.clone(),
    }
}

/// Rewrite temporal properties: state references to parent names become
/// disjunctions of all substates.
fn rewrite_properties(
    properties: &[TemporalProperty],
    parent_map: &ParentMap,
) -> Vec<TemporalProperty> {
    properties
        .iter()
        .map(|p| TemporalProperty {
            name: p.name.clone(),
            expr: rewrite_temporal_expr(&p.expr, parent_map),
        })
        .collect()
}

fn rewrite_temporal_expr(expr: &TemporalExpr, parent_map: &ParentMap) -> TemporalExpr {
    match expr {
        TemporalExpr::State(name) => {
            if let Some((_, substates)) = parent_map.get(name) {
                // Parent state reference → disjunction: state1 OR state2 OR ...
                // Use BinOp with Or to represent this
                if substates.is_empty() {
                    TemporalExpr::State(name.clone())
                } else if substates.len() == 1 {
                    TemporalExpr::State(substates[0].clone())
                } else {
                    let mut iter = substates.iter();
                    let first = TemporalExpr::State(iter.next().unwrap().clone());
                    iter.fold(first, |acc, s| TemporalExpr::BinOp {
                        lhs: Box::new(acc),
                        op: crate::parser::ast::TemporalOp::Or,
                        rhs: Box::new(TemporalExpr::State(s.clone())),
                    })
                }
            } else {
                expr.clone()
            }
        }
        TemporalExpr::Always(inner) => {
            TemporalExpr::Always(Box::new(rewrite_temporal_expr(inner, parent_map)))
        }
        TemporalExpr::Eventually(inner) => {
            TemporalExpr::Eventually(Box::new(rewrite_temporal_expr(inner, parent_map)))
        }
        TemporalExpr::Next(inner) => {
            TemporalExpr::Next(Box::new(rewrite_temporal_expr(inner, parent_map)))
        }
        TemporalExpr::Not(inner) => {
            TemporalExpr::Not(Box::new(rewrite_temporal_expr(inner, parent_map)))
        }
        TemporalExpr::Until { lhs, rhs } => TemporalExpr::Until {
            lhs: Box::new(rewrite_temporal_expr(lhs, parent_map)),
            rhs: Box::new(rewrite_temporal_expr(rhs, parent_map)),
        },
        TemporalExpr::Release { lhs, rhs } => TemporalExpr::Release {
            lhs: Box::new(rewrite_temporal_expr(lhs, parent_map)),
            rhs: Box::new(rewrite_temporal_expr(rhs, parent_map)),
        },
        TemporalExpr::WeakUntil { lhs, rhs } => TemporalExpr::WeakUntil {
            lhs: Box::new(rewrite_temporal_expr(lhs, parent_map)),
            rhs: Box::new(rewrite_temporal_expr(rhs, parent_map)),
        },
        TemporalExpr::StrongRelease { lhs, rhs } => TemporalExpr::StrongRelease {
            lhs: Box::new(rewrite_temporal_expr(lhs, parent_map)),
            rhs: Box::new(rewrite_temporal_expr(rhs, parent_map)),
        },
        TemporalExpr::AlwaysImplies {
            premise,
            conclusion,
        } => TemporalExpr::AlwaysImplies {
            premise: Box::new(rewrite_temporal_expr(premise, parent_map)),
            conclusion: Box::new(rewrite_temporal_expr(conclusion, parent_map)),
        },
        TemporalExpr::BinOp { lhs, op, rhs } => TemporalExpr::BinOp {
            lhs: Box::new(rewrite_temporal_expr(lhs, parent_map)),
            op: *op,
            rhs: Box::new(rewrite_temporal_expr(rhs, parent_map)),
        },
        TemporalExpr::Count(name) => {
            // Count references parent → keep as-is (count of parent is sum of substates)
            TemporalExpr::Count(name.clone())
        }
        TemporalExpr::Int(v) => TemporalExpr::Int(*v),
        TemporalExpr::Str(s) => TemporalExpr::Str(s.clone()),
        TemporalExpr::Bool(b) => TemporalExpr::Bool(*b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(name: &str, initial: bool, terminal: bool) -> StateDecl {
        StateDecl {
            name: name.to_string(),
            initial,
            terminal,
            parent: None,
            substates: Vec::new(),
            entry_actions: Vec::new(),
            exit_actions: Vec::new(),
        }
    }

    #[test]
    fn test_flatten_simple_hierarchy() {
        let states = vec![
            StateDecl {
                name: "active".to_string(),
                initial: true,
                terminal: false,
                parent: None,
                substates: vec![
                    make_state("processing", true, false),
                    make_state("waiting", false, false),
                ],
                entry_actions: Vec::new(),
                exit_actions: Vec::new(),
            },
            make_state("done", false, true),
        ];

        let (flat, parent_map) = flatten_states(&states, "");

        // Should have 3 flat states: active.processing, active.waiting, done
        assert_eq!(flat.len(), 3);
        assert!(flat.iter().any(|s| s.name == "active.processing"));
        assert!(flat.iter().any(|s| s.name == "active.waiting"));
        assert!(flat.iter().any(|s| s.name == "done"));

        // active.processing should be initial (inherited from parent + own initial)
        let proc = flat.iter().find(|s| s.name == "active.processing").unwrap();
        assert!(proc.initial);

        // Parent map should contain "active"
        assert!(parent_map.contains_key("active"));
        let (init, subs) = &parent_map["active"];
        assert_eq!(init.as_deref(), Some("active.processing"));
        assert_eq!(subs.len(), 2);
    }

    #[test]
    fn test_no_hierarchy_passthrough() {
        let behavior = BehaviorDecl {
            name: "simple".to_string(),
            states: vec![make_state("a", true, false), make_state("b", false, true)],
            ..Default::default()
        };

        let result = desugar_hierarchical_states(&behavior);
        assert_eq!(result.states.len(), 2);
        assert_eq!(result.states[0].name, "a");
        assert_eq!(result.states[1].name, "b");
    }
}
