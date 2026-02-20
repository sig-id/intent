//! Standard validation passes for the Intent language.

use crate::diagnostic::{Diagnostic, ErrorCode, Span};
use crate::parser::ast::{
    BehaviorDecl, ComponentDecl, ConstraintDecl, PatternDecl, SystemDecl,
};
use crate::types::checker::{self, TypeContext};
use crate::validation::{ValidationContext, ValidationPass};

use std::collections::{HashMap, HashSet};

/// Type checking pass.
pub struct TypeCheckPass;

impl ValidationPass for TypeCheckPass {
    fn name(&self) -> &'static str {
        "type_check"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        let mut type_ctx = TypeContext::new();

        // Check patterns
        for pattern in &system.patterns {
            checker::check_pattern_params(&pattern.parameters, &mut type_ctx);
        }

        // Check pattern applications
        for applies in &system.applies {
            // Look up the pattern definition
            if let Some(pattern) = system.patterns.iter().find(|p| p.name == applies.pattern) {
                checker::check_pattern_application(applies, &pattern.parameters, &mut type_ctx);
            }
        }

        // Check component-level patterns and applications
        for component in &system.components {
            check_component_types(component, &mut type_ctx);
        }

        ctx.diagnostics.merge(type_ctx.diagnostics);
    }
}

fn check_component_types(component: &ComponentDecl, _ctx: &mut TypeContext) {
    // Check component-level patterns
    for pattern in &component.behaviors {
        // Check if this is actually a pattern (has type_params)
        // For now, just check the behaviors
        let _ = pattern;
    }
}

/// Entity resolution pass.
pub struct EntityResolutionPass;

impl ValidationPass for EntityResolutionPass {
    fn name(&self) -> &'static str {
        "entity_resolution"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Collect all declared entities
        let mut declared_entities: HashSet<String> = HashSet::new();

        // Add components
        for component in &system.components {
            declared_entities.insert(component.name.clone());
            declared_entities.extend(component.contains.iter().cloned());

            // Add nested components
            for nested in &component.components {
                declared_entities.insert(nested.name.clone());
            }
        }

        // Add let bindings
        for (name, _) in &system.let_bindings {
            declared_entities.insert(name.clone());
        }

        // Now check references
        for constraint in &system.constraints {
            check_constraint_references(constraint, &declared_entities, ctx);
        }

        // Check component depends_only references
        for component in &system.components {
            for dep in &component.depends_only {
                if !declared_entities.contains(dep) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E013_ComponentNotFound,
                        format!("Component '{}' in depends_only not found", dep),
                        Span::synthetic(),
                    ).with_suggestion(format!("Available components: {}", declared_entities.iter().cloned().collect::<Vec<_>>().join(", "))));
                }
            }
        }
    }
}

fn check_constraint_references(
    constraint: &ConstraintDecl,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    for rule in &constraint.rules {
        check_rule_references(rule, declared, ctx);
    }
}

fn check_rule_references(
    rule: &crate::parser::ast::ConstraintRule,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::ConstraintRule;

    match rule {
        ConstraintRule::Not(inner) => {
            check_rule_references(inner, declared, ctx);
        }
        ConstraintRule::And(a, b) | ConstraintRule::Or(a, b) | ConstraintRule::Implies(a, b) | ConstraintRule::Iff(a, b) => {
            check_rule_references(a, declared, ctx);
            check_rule_references(b, declared, ctx);
        }
        ConstraintRule::Forall { domain, body, .. } | ConstraintRule::Exists { domain, body, .. } => {
            check_scope_expr_references(domain, declared, ctx);
            check_rule_references(body, declared, ctx);
        }
        ConstraintRule::Predicate(pred) => {
            check_predicate_references(pred, declared, ctx);
        }
        ConstraintRule::Comparison { .. } | ConstraintRule::NFConstraint { .. } => {}
        ConstraintRule::Call { subject, args, .. } => {
            check_scope_expr_references(subject, declared, ctx);
            for arg in args {
                check_scope_expr_references(arg, declared, ctx);
            }
        }
        ConstraintRule::Suppressed { rule, .. } => {
            check_rule_references(rule, declared, ctx);
        }
    }
}

fn check_scope_expr_references(
    expr: &crate::parser::ast::ScopeExpr,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::ScopeExpr;

    match expr {
        ScopeExpr::Ident(qname) => {
            if !qname.is_simple() || !declared.contains(&qname.to_dotted()) {
                // For now, only check simple names; qualified names may reference external items
                if qname.is_simple() && !declared.contains(&qname.to_dotted()) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E001_UnknownIdentifier,
                        format!("Unknown identifier '{}' in scope expression", qname.to_dotted()),
                        Span::synthetic(),
                    ));
                }
            }
        }
        ScopeExpr::EntityList(names) => {
            for name in names {
                if !declared.contains(name) {
                    ctx.diagnostics.add(Diagnostic::warning(
                        ErrorCode::E001_UnknownIdentifier,
                        format!("Entity '{}' may not be defined", name),
                        Span::synthetic(),
                    ));
                }
            }
        }
        ScopeExpr::Union(a, b) | ScopeExpr::Intersection(a, b) | ScopeExpr::Difference(a, b) => {
            check_scope_expr_references(a, declared, ctx);
            check_scope_expr_references(b, declared, ctx);
        }
        ScopeExpr::Glob(_) | ScopeExpr::All => {}
        ScopeExpr::Matches { .. } => {}
        ScopeExpr::Filtered { condition, .. } => {
            let _ = condition; // Would check expression references
        }
    }
}

fn check_predicate_references(
    pred: &crate::parser::ast::PredicateCall,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::PredicateCall;

    match pred {
        PredicateCall::Depends { from, to } => {
            check_scope_expr_references(from, declared, ctx);
            for target in to {
                check_scope_expr_references(target, declared, ctx);
            }
        }
        PredicateCall::References { from, to } => {
            check_scope_expr_references(from, declared, ctx);
            for target in to {
                check_scope_expr_references(target, declared, ctx);
            }
        }
        PredicateCall::Implements { entity, .. } => {
            check_scope_expr_references(entity, declared, ctx);
        }
        PredicateCall::Contains { container, entities } => {
            check_scope_expr_references(container, declared, ctx);
            for entity in entities {
                check_scope_expr_references(entity, declared, ctx);
            }
        }
    }
}

/// State reachability pass.
pub struct StateReachabilityPass;

impl ValidationPass for StateReachabilityPass {
    fn name(&self) -> &'static str {
        "state_reachability"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            check_behavior_reachability(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_reachability(behavior, ctx);
            }
        }
    }
}

fn check_behavior_reachability(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    // Check for exactly one initial state
    let initial_states: Vec<_> = behavior.states.iter().filter(|s| s.initial).collect();

    match initial_states.len() {
        0 => {
            ctx.diagnostics.add(Diagnostic::error(
                ErrorCode::E021_NoInitialState,
                format!("Behavior '{}' has no initial state", behavior.name),
                Span::synthetic(),
            ).with_suggestion("Add `initial: true` to one state"));
        }
        1 => {}
        _ => {
            let names: Vec<_> = initial_states.iter().map(|s| s.name.as_str()).collect();
            ctx.diagnostics.add(Diagnostic::error(
                ErrorCode::E020_MultipleInitialStates,
                format!("Behavior '{}' has multiple initial states: {}", behavior.name, names.join(", ")),
                Span::synthetic(),
            ).with_suggestion("Only one state should have `initial: true`"));
        }
    }

    // Check for unreachable states
    let reachable = compute_reachable_states(behavior);
    for state in &behavior.states {
        if !reachable.contains(&state.name) && !state.initial {
            ctx.diagnostics.add(Diagnostic::warning(
                ErrorCode::E006_UnreachableState,
                format!("State '{}' in behavior '{}' is unreachable", state.name, behavior.name),
                Span::synthetic(),
            ).with_suggestion("Add a transition to this state or remove it"));
        }
    }

    // Check for terminal states with outgoing transitions
    let terminal_states: HashSet<_> = behavior
        .states
        .iter()
        .filter(|s| s.terminal)
        .map(|s| s.name.as_str())
        .collect();

    for transition in &behavior.transitions {
        if let Some(from) = transition.from.as_state() {
            if terminal_states.contains(from) {
                ctx.diagnostics.add(Diagnostic::warning(
                    ErrorCode::E022_TerminalStateTransitions,
                    format!(
                        "Terminal state '{}' in behavior '{}' has outgoing transition to '{}'",
                        from, behavior.name, transition.to
                    ),
                    Span::synthetic(),
                ));
            }
        }
    }
}

fn compute_reachable_states(behavior: &BehaviorDecl) -> HashSet<String> {
    let mut reachable = HashSet::new();

    // Start from initial states
    for state in &behavior.states {
        if state.initial {
            reachable.insert(state.name.clone());
        }
    }

    // BFS to find all reachable states
    let mut changed = true;
    while changed {
        changed = false;
        for transition in &behavior.transitions {
            if let (Some(from), Some(to)) = (transition.from.as_state(), transition.to.as_state()) {
                if reachable.contains(from) && !reachable.contains(to) {
                    reachable.insert(to.to_string());
                    changed = true;
                }
            }
        }
    }

    reachable
}

/// Event declaration pass.
pub struct EventDeclarationPass;

impl ValidationPass for EventDeclarationPass {
    fn name(&self) -> &'static str {
        "event_declaration"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Collect all events used in transitions
        let mut used_events: HashSet<String> = HashSet::new();

        for behavior in &system.behaviors {
            for transition in &behavior.transitions {
                used_events.insert(transition.on_event.clone());
            }
        }

        for component in &system.components {
            for behavior in &component.behaviors {
                for transition in &behavior.transitions {
                    used_events.insert(transition.on_event.clone());
                }
            }
        }

        // For now, we don't require event declarations
        // This pass is here for future use when event declarations are added
        let _ = (used_events, ctx);
    }
}

/// Pattern compatibility pass.
pub struct PatternCompatibilityPass;

impl ValidationPass for PatternCompatibilityPass {
    fn name(&self) -> &'static str {
        "pattern_compatibility"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Build a map of available patterns
        let patterns: HashMap<&str, &PatternDecl> = system
            .patterns
            .iter()
            .map(|p| (p.name.as_str(), p))
            .collect();

        // Check all pattern applications
        for applies in &system.applies {
            if !patterns.contains_key(applies.pattern.as_str()) {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E015_PatternNotFound,
                    format!("Pattern '{}' not found", applies.pattern),
                    Span::synthetic(),
                ).with_suggestion(format!(
                    "Available patterns: {}",
                    patterns.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }

        // Check component-level applications
        for component in &system.components {
            for behavior in &component.behaviors {
                for applies in &behavior.applies {
                    if !patterns.contains_key(applies.pattern.as_str()) {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E015_PatternNotFound,
                            format!("Pattern '{}' not found in component '{}'", applies.pattern, component.name),
                            Span::synthetic(),
                        ));
                    }
                }
            }
        }
    }
}

/// Refinement validation pass.
pub struct RefinementValidationPass;

impl ValidationPass for RefinementValidationPass {
    fn name(&self) -> &'static str {
        "refinement_validation"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system refinement
        if let Some(refines) = &system.refines {
            // Would need access to the refined system to validate
            // For now, just note that refinement is declared
            let _ = refines;
        }

        // Check behavior refinements
        for behavior in &system.behaviors {
            check_behavior_refinement(behavior, ctx);
        }

        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_refinement(behavior, ctx);
            }
        }
    }
}

fn check_behavior_refinement(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    if let Some(refines) = &behavior.refines {
        // Check that refinement map covers all abstract states
        if let Some(ref map) = &behavior.refinement_map {
            // For each mapping, verify the concrete states exist
            let concrete_states: HashSet<_> = behavior
                .states
                .iter()
                .map(|s| s.name.as_str())
                .collect();

            for (_, concrete_list) in &map.mappings {
                for concrete in concrete_list {
                    if !concrete_states.contains(concrete.as_str()) {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E012_InvalidRefinementMapping,
                            format!(
                                "Concrete state '{}' in refinement map not found in behavior '{}'",
                                concrete, behavior.name
                            ),
                            Span::synthetic(),
                        ));
                    }
                }
            }
        }
        let _ = refines;
    }
}
