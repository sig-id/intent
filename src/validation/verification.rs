//! Semantic verification passes for the Intent language.
//!
//! This module provides advanced verification including:
//! - Deadlock detection (states with no outgoing transitions)
//! - Livelock detection (cycles that cannot reach terminal states)
//! - Transition validation (source/target states exist)
//! - Property validation (temporal expressions reference valid states)

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, ErrorCode, Span};
use crate::diagnostic::suggestions::{suggest_multiple, format_suggestions};
use crate::parser::ast::{BehaviorDecl, SystemDecl, TemporalExpr, TransitionSource, TransitionTarget};
use crate::validation::{ValidationContext, ValidationPass};

// ═══════════════════════════════════════════════════════════════════════════
// TRANSITION VALIDATION PASS
// ═══════════════════════════════════════════════════════════════════════════

/// Validates that all transition source and target states are defined.
pub struct TransitionValidationPass;

impl ValidationPass for TransitionValidationPass {
    fn name(&self) -> &'static str {
        "transition_validation"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            self.validate_behavior_transitions(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                self.validate_behavior_transitions(behavior, ctx);
            }
        }
    }
}

impl TransitionValidationPass {
    fn validate_behavior_transitions(&self, behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
        // Collect all defined states
        let defined_states: HashSet<&str> = behavior
            .states
            .iter()
            .map(|s| s.name.as_str())
            .collect();

        // Collect all referenced states for suggestions
        let defined_state_names: Vec<&str> = defined_states.iter().copied().collect();

        for transition in &behavior.transitions {
            // Validate source states
            for source_state in transition.from.states() {
                if !defined_states.contains(source_state) {
                    let suggestions = suggest_multiple(source_state, &defined_state_names, 3);
                    let mut diag = Diagnostic::error(
                        ErrorCode::E028_InvalidTransitionSource,
                        format!(
                            "Source state '{}' in transition is not defined in behavior '{}'",
                            source_state, behavior.name
                        ),
                        transition.span,
                    );

                    if !suggestions.is_empty() {
                        diag = diag.with_suggestion(format_suggestions(&suggestions));
                    }

                    ctx.diagnostics.add(diag);
                }
            }

            // Validate target states
            for target_state in transition.to.states() {
                if !defined_states.contains(target_state) {
                    let suggestions = suggest_multiple(target_state, &defined_state_names, 3);
                    let mut diag = Diagnostic::error(
                        ErrorCode::E029_InvalidTransitionTarget,
                        format!(
                            "Target state '{}' in transition is not defined in behavior '{}'",
                            target_state, behavior.name
                        ),
                        transition.span,
                    );

                    if !suggestions.is_empty() {
                        diag = diag.with_suggestion(format_suggestions(&suggestions));
                    }

                    ctx.diagnostics.add(diag);
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DEADLOCK DETECTION PASS
// ═══════════════════════════════════════════════════════════════════════════

/// Detects potential deadlocks in state machines.
///
/// A deadlock is a non-terminal state with no outgoing transitions.
pub struct DeadlockDetectionPass;

impl ValidationPass for DeadlockDetectionPass {
    fn name(&self) -> &'static str {
        "deadlock_detection"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            self.detect_deadlocks(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                self.detect_deadlocks(behavior, ctx);
            }
        }
    }
}

impl DeadlockDetectionPass {
    fn detect_deadlocks(&self, behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
        // Find states with outgoing transitions
        let mut states_with_outgoing: HashSet<&str> = HashSet::new();

        for transition in &behavior.transitions {
            for source_state in transition.from.states() {
                states_with_outgoing.insert(source_state);
            }
        }

        // Non-terminal states without outgoing transitions are potential deadlocks
        for state in &behavior.states {
            if !state.terminal && !states_with_outgoing.contains(state.name.as_str()) {
                // Check if this is an initial state with no transitions (might be intentional)
                if state.initial && behavior.transitions.is_empty() {
                    continue; // Single-state behavior, skip
                }

                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E026_DeadlockDetected,
                        format!(
                            "Potential deadlock: non-terminal state '{}' has no outgoing transitions in behavior '{}'",
                            state.name, behavior.name
                        ),
                        Span::synthetic(),
                    )
                    .with_suggestion("Add a transition from this state or mark it as terminal"),
                );
            }
        }

        // Find states that can reach deadlock states
        let deadlock_states: HashSet<&str> = behavior
            .states
            .iter()
            .filter(|s| !s.terminal && !states_with_outgoing.contains(s.name.as_str()))
            .map(|s| s.name.as_str())
            .collect();

        if !deadlock_states.is_empty() {
            let can_reach_deadlock = self.compute_states_reaching_target(behavior, &deadlock_states);

            if can_reach_deadlock.len() > deadlock_states.len() {
                let reachable: Vec<&str> = can_reach_deadlock.iter()
                    .filter(|s| !deadlock_states.contains(*s))
                    .copied()
                    .collect();

                ctx.diagnostics.add(
                    Diagnostic::info(
                        ErrorCode::E026_DeadlockDetected,
                        format!(
                            "States that can reach deadlocks in '{}': {}",
                            behavior.name,
                            reachable.join(", ")
                        ),
                        Span::synthetic(),
                    ),
                );
            }
        }
    }

    /// Compute all states that can reach any of the target states.
    fn compute_states_reaching_target<'a>(
        &self,
        behavior: &'a BehaviorDecl,
        targets: &HashSet<&'a str>,
    ) -> HashSet<&'a str> {
        // Build reverse graph
        let mut reverse_edges: HashMap<&str, HashSet<&str>> = HashMap::new();

        for transition in &behavior.transitions {
            let to_states: Vec<&str> = transition.to.states();
            for to_state in &to_states {
                for from_state in transition.from.states() {
                    reverse_edges.entry(to_state).or_default().insert(from_state);
                }
            }
        }

        // BFS from target states
        let mut reachable: HashSet<&str> = targets.clone();
        let mut worklist: Vec<&str> = targets.iter().copied().collect();

        while let Some(state) = worklist.pop() {
            if let Some(predecessors) = reverse_edges.get(state) {
                for &pred in predecessors {
                    if !reachable.contains(pred) {
                        reachable.insert(pred);
                        worklist.push(pred);
                    }
                }
            }
        }

        reachable
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LIVELOCK DETECTION PASS
// ═══════════════════════════════════════════════════════════════════════════

/// Detects potential livelocks in state machines.
///
/// A livelock is a cycle of states that cannot reach any terminal state.
/// Uses Tarjan's algorithm for Strongly Connected Components detection.
pub struct LivelockDetectionPass;

impl ValidationPass for LivelockDetectionPass {
    fn name(&self) -> &'static str {
        "livelock_detection"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            self.detect_livelocks(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                self.detect_livelocks(behavior, ctx);
            }
        }
    }
}

impl LivelockDetectionPass {
    fn detect_livelocks(&self, behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
        // Get terminal states
        let terminal_states: HashSet<&str> = behavior
            .states
            .iter()
            .filter(|s| s.terminal)
            .map(|s| s.name.as_str())
            .collect();

        if terminal_states.is_empty() {
            // No terminal states - cannot detect livelocks meaningfully
            return;
        }

        // Compute states that can reach terminal states
        let can_reach_terminal = self.compute_states_reaching_target(behavior, &terminal_states);

        // Find SCCs using Tarjan's algorithm
        let sccs = self.find_sccs(behavior);

        // Check each SCC
        for scc in sccs {
            // Skip single-state SCCs unless they have self-loops
            if scc.len() == 1 {
                continue;
            }

            // Check if any state in the SCC can reach a terminal state
            let can_escape = scc.iter().any(|s| can_reach_terminal.contains(*s));

            if !can_escape {
                // This is a potential livelock - a cycle that cannot escape
                let states: Vec<&str> = scc.iter().copied().collect();

                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E027_LivelockDetected,
                        format!(
                            "Potential livelock detected in behavior '{}': cycle of states [{}] cannot reach any terminal state",
                            behavior.name,
                            states.join(", ")
                        ),
                        Span::synthetic(),
                    )
                    .with_suggestion(
                        "Add a transition from this cycle to a terminal state or mark states as terminal"
                    ),
                );
            }
        }
    }

    /// Compute all states that can reach any of the target states.
    fn compute_states_reaching_target<'a>(
        &self,
        behavior: &'a BehaviorDecl,
        targets: &HashSet<&'a str>,
    ) -> HashSet<&'a str> {
        // Build reverse graph
        let mut reverse_edges: HashMap<&str, HashSet<&str>> = HashMap::new();

        for transition in &behavior.transitions {
            let to_states: Vec<&str> = transition.to.states();
            for to_state in &to_states {
                for from_state in transition.from.states() {
                    reverse_edges.entry(to_state).or_default().insert(from_state);
                }
            }
        }

        // BFS from target states
        let mut reachable: HashSet<&str> = targets.clone();
        let mut worklist: Vec<&str> = targets.iter().copied().collect();

        while let Some(state) = worklist.pop() {
            if let Some(predecessors) = reverse_edges.get(state) {
                for &pred in predecessors {
                    if !reachable.contains(pred) {
                        reachable.insert(pred);
                        worklist.push(pred);
                    }
                }
            }
        }

        reachable
    }

    /// Find all strongly connected components using Tarjan's algorithm.
    fn find_sccs<'a>(&self, behavior: &'a BehaviorDecl) -> Vec<HashSet<&'a str>> {
        // Build adjacency list
        let mut graph: HashMap<&'a str, HashSet<&'a str>> = HashMap::new();

        for state in &behavior.states {
            graph.entry(state.name.as_str()).or_default();
        }

        for transition in &behavior.transitions {
            for from_state in transition.from.states() {
                for to_state in transition.to.states() {
                    graph.entry(from_state).or_default().insert(to_state);
                }
            }
        }

        // Tarjan's algorithm
        let mut index_counter = 0;
        let mut stack: Vec<&'a str> = Vec::new();
        let mut on_stack: HashSet<&'a str> = HashSet::new();
        let mut indices: HashMap<&'a str, usize> = HashMap::new();
        let mut lowlinks: HashMap<&'a str, usize> = HashMap::new();
        let mut sccs: Vec<HashSet<&'a str>> = Vec::new();

        for state in behavior.states.iter().map(|s| s.name.as_str()) {
            if !indices.contains_key(state) {
                self.strongconnect(
                    state,
                    &graph,
                    &mut index_counter,
                    &mut stack,
                    &mut on_stack,
                    &mut indices,
                    &mut lowlinks,
                    &mut sccs,
                );
            }
        }

        sccs
    }

    fn strongconnect<'a>(
        &self,
        v: &'a str,
        graph: &HashMap<&'a str, HashSet<&'a str>>,
        index_counter: &mut usize,
        stack: &mut Vec<&'a str>,
        on_stack: &mut HashSet<&'a str>,
        indices: &mut HashMap<&'a str, usize>,
        lowlinks: &mut HashMap<&'a str, usize>,
        sccs: &mut Vec<HashSet<&'a str>>,
    ) {
        indices.insert(v, *index_counter);
        lowlinks.insert(v, *index_counter);
        *index_counter += 1;
        stack.push(v);
        on_stack.insert(v);

        if let Some(neighbors) = graph.get(v) {
            for &w in neighbors {
                if !indices.contains_key(w) {
                    self.strongconnect(w, graph, index_counter, stack, on_stack, indices, lowlinks, sccs);
                    let v_low = *lowlinks.get(v).unwrap();
                    let w_low = *lowlinks.get(w).unwrap();
                    lowlinks.insert(v, v_low.min(w_low));
                } else if on_stack.contains(w) {
                    let v_low = *lowlinks.get(v).unwrap();
                    let w_idx = *indices.get(w).unwrap();
                    lowlinks.insert(v, v_low.min(w_idx));
                }
            }
        }

        // If v is a root node, pop the stack and generate an SCC
        if lowlinks.get(v) == indices.get(v) {
            let mut scc: HashSet<&str> = HashSet::new();
            loop {
                let w = stack.pop().unwrap();
                on_stack.remove(w);
                scc.insert(w);
                if w == v {
                    break;
                }
            }
            sccs.push(scc);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PROPERTY VALIDATION PASS
// ═══════════════════════════════════════════════════════════════════════════

/// Validates that temporal properties reference valid states and events.
pub struct PropertyValidationPass;

impl ValidationPass for PropertyValidationPass {
    fn name(&self) -> &'static str {
        "property_validation"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            self.validate_properties(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                self.validate_properties(behavior, ctx);
            }
        }
    }
}

impl PropertyValidationPass {
    fn validate_properties(&self, behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
        // Collect all defined states
        let defined_states: HashSet<&str> = behavior
            .states
            .iter()
            .map(|s| s.name.as_str())
            .collect();

        let defined_state_names: Vec<&str> = defined_states.iter().copied().collect();

        // Validate each property
        for property in &behavior.properties {
            self.validate_temporal_expr(
                &property.expr,
                &defined_states,
                &defined_state_names,
                &behavior.name,
                &property.name,
                ctx,
            );
        }

        // Validate invariants
        for invariant in &behavior.invariants {
            // Invariants can reference any variable/state - basic validation
            // For now, we just check that any state references are valid
            self.validate_expr_states(
                &invariant.expr,
                &defined_states,
                &defined_state_names,
                &behavior.name,
                ctx,
            );
        }
    }

    fn validate_temporal_expr(
        &self,
        expr: &TemporalExpr,
        defined_states: &HashSet<&str>,
        defined_state_names: &[&str],
        behavior_name: &str,
        property_name: &str,
        ctx: &mut ValidationContext,
    ) {
        match expr {
            TemporalExpr::State(state_name) => {
                if !defined_states.contains(state_name.as_str()) {
                    let suggestions = suggest_multiple(state_name, defined_state_names, 3);
                    let mut diag = Diagnostic::error(
                        ErrorCode::E003_UndefinedState,
                        format!(
                            "State '{}' referenced in property '{}' is not defined in behavior '{}'",
                            state_name, property_name, behavior_name
                        ),
                        Span::synthetic(),
                    );

                    if !suggestions.is_empty() {
                        diag = diag.with_suggestion(format_suggestions(&suggestions));
                    }

                    ctx.diagnostics.add(diag);
                }
            }
            TemporalExpr::Count(state_name) => {
                if !defined_states.contains(state_name.as_str()) {
                    let suggestions = suggest_multiple(state_name, defined_state_names, 3);
                    let mut diag = Diagnostic::warning(
                        ErrorCode::E003_UndefinedState,
                        format!(
                            "State '{}' in count() may not be defined in behavior '{}'",
                            state_name, behavior_name
                        ),
                        Span::synthetic(),
                    );

                    if !suggestions.is_empty() {
                        diag = diag.with_suggestion(format_suggestions(&suggestions));
                    }

                    ctx.diagnostics.add(diag);
                }
            }
            TemporalExpr::Always(inner)
            | TemporalExpr::Eventually(inner)
            | TemporalExpr::Next(inner)
            | TemporalExpr::Not(inner) => {
                self.validate_temporal_expr(
                    inner,
                    defined_states,
                    defined_state_names,
                    behavior_name,
                    property_name,
                    ctx,
                );
            }
            TemporalExpr::Until { lhs, rhs }
            | TemporalExpr::Release { lhs, rhs }
            | TemporalExpr::WeakUntil { lhs, rhs }
            | TemporalExpr::StrongRelease { lhs, rhs }
            | TemporalExpr::AlwaysImplies { premise: lhs, conclusion: rhs } => {
                self.validate_temporal_expr(
                    lhs,
                    defined_states,
                    defined_state_names,
                    behavior_name,
                    property_name,
                    ctx,
                );
                self.validate_temporal_expr(
                    rhs,
                    defined_states,
                    defined_state_names,
                    behavior_name,
                    property_name,
                    ctx,
                );
            }
            TemporalExpr::BinOp { lhs, rhs, .. } => {
                self.validate_temporal_expr(
                    lhs,
                    defined_states,
                    defined_state_names,
                    behavior_name,
                    property_name,
                    ctx,
                );
                self.validate_temporal_expr(
                    rhs,
                    defined_states,
                    defined_state_names,
                    behavior_name,
                    property_name,
                    ctx,
                );
            }
            TemporalExpr::Int(_) => {}
        }
    }

    fn validate_expr_states(
        &self,
        expr: &crate::parser::ast::Expr,
        defined_states: &HashSet<&str>,
        defined_state_names: &[&str],
        behavior_name: &str,
        ctx: &mut ValidationContext,
    ) {
        // Recursively check for state references in expressions
        match expr {
            crate::parser::ast::Expr::Ident(name) => {
                // If it looks like a state name (capitalized), check it
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    && !defined_states.contains(name.as_str())
                {
                    let suggestions = suggest_multiple(name, defined_state_names, 3);
                    if !suggestions.is_empty() {
                        ctx.diagnostics.add(
                            Diagnostic::hint(
                                ErrorCode::E003_UndefinedState,
                                format!(
                                    "'{}' might be an undefined state reference in behavior '{}'",
                                    name, behavior_name
                                ),
                                Span::synthetic(),
                            )
                            .with_suggestion(format_suggestions(&suggestions)),
                        );
                    }
                }
            }
            crate::parser::ast::Expr::Call { args, .. } => {
                for arg in args {
                    self.validate_expr_states(arg, defined_states, defined_state_names, behavior_name, ctx);
                }
            }
            crate::parser::ast::Expr::BinOp { lhs, rhs, .. }
            | crate::parser::ast::Expr::CompOp { lhs, rhs, .. }
            | crate::parser::ast::Expr::LogicalOp { lhs, rhs, .. } => {
                self.validate_expr_states(lhs, defined_states, defined_state_names, behavior_name, ctx);
                self.validate_expr_states(rhs, defined_states, defined_state_names, behavior_name, ctx);
            }
            crate::parser::ast::Expr::UnaryOp { expr, .. } => {
                self.validate_expr_states(expr, defined_states, defined_state_names, behavior_name, ctx);
            }
            // Add more cases as needed
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::*;

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

    fn make_transition(from: &str, to: &str, event: &str) -> TransitionDecl {
        TransitionDecl {
            from: TransitionSource::State(from.to_string()),
            to: TransitionTarget::State(to.to_string()),
            on_event: event.to_string(),
            guard: None,
            effects: Vec::new(),
            timing: None,
            span: Span::synthetic(),
        }
    }

    #[test]
    fn test_deadlock_detection_finds_deadlock() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("active", false, false), // No outgoing transitions - deadlock
                make_state("done", false, true),
            ],
            transitions: vec![
                make_transition("idle", "active", "start"),
                // No transition from "active" to anywhere
            ],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        DeadlockDetectionPass.detect_deadlocks(&behavior, &mut ctx);

        assert!(ctx.diagnostics.has_warnings());
    }

    #[test]
    fn test_deadlock_detection_no_deadlock() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("done", false, true),
            ],
            transitions: vec![
                make_transition("idle", "done", "start"),
            ],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        DeadlockDetectionPass.detect_deadlocks(&behavior, &mut ctx);

        assert!(!ctx.diagnostics.has_warnings());
    }

    #[test]
    fn test_livelock_detection_finds_livelock() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("loop1", false, false),
                make_state("loop2", false, false),
                make_state("done", false, true),
            ],
            transitions: vec![
                make_transition("idle", "loop1", "start"),
                make_transition("loop1", "loop2", "next"),
                make_transition("loop2", "loop1", "back"),
                // No way to reach "done" from the loop
            ],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        LivelockDetectionPass.detect_livelocks(&behavior, &mut ctx);

        assert!(ctx.diagnostics.has_warnings());
    }

    #[test]
    fn test_livelock_detection_no_livelock() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("loop1", false, false),
                make_state("loop2", false, false),
                make_state("done", false, true),
            ],
            transitions: vec![
                make_transition("idle", "loop1", "start"),
                make_transition("loop1", "loop2", "next"),
                make_transition("loop2", "loop1", "back"),
                make_transition("loop1", "done", "finish"), // Can escape to terminal
            ],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        LivelockDetectionPass.detect_livelocks(&behavior, &mut ctx);

        assert!(!ctx.diagnostics.has_warnings());
    }

    #[test]
    fn test_transition_validation_finds_invalid_state() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("done", false, true),
            ],
            transitions: vec![
                make_transition("idle", "unknown_state", "start"), // Invalid target
            ],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        TransitionValidationPass.validate_behavior_transitions(&behavior, &mut ctx);

        assert!(ctx.diagnostics.has_errors());
    }

    #[test]
    fn test_property_validation_finds_invalid_state() {
        let behavior = BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_state("idle", true, false),
                make_state("done", false, true),
            ],
            properties: vec![TemporalProperty {
                name: "test_prop".to_string(),
                expr: TemporalExpr::Eventually(Box::new(TemporalExpr::State("unknown".to_string()))),
            }],
            ..Default::default()
        };

        let mut ctx = ValidationContext::new();
        PropertyValidationPass.validate_properties(&behavior, &mut ctx);

        assert!(ctx.diagnostics.has_errors());
    }
}
