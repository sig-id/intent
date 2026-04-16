//! Additional lint checks for the Intent language.
//!
//! This module provides specialized checks that can be run independently
//! or as part of the main linting pipeline.

use crate::diagnostic::{Diagnostic, Diagnostics, ErrorCode, Span};
use crate::parser::ast::*;
use std::collections::{HashMap, HashSet};

/// Check for cyclic dependencies in component dependencies.
pub fn check_cyclic_dependencies(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Build dependency graph
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

    for component in &system.components {
        graph.insert(&component.name, Vec::new());
        for dep in &component.depends_only {
            graph.get_mut(&component.name.as_str()).unwrap().push(dep.as_str());
        }
    }

    // Detect cycles using DFS
    fn has_cycle<'a>(
        node: &'a str,
        graph: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        rec_stack: &mut HashSet<&'a str>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<&'a str>> {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        if let Some(neighbors) = graph.get(node) {
            for &neighbor in neighbors {
                if !visited.contains(neighbor) {
                    if let Some(cycle) = has_cycle(neighbor, graph, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(neighbor) {
                    // Found cycle
                    let cycle_start = path.iter().position(|&n| n == neighbor).unwrap();
                    let cycle: Vec<&str> = path[cycle_start..].to_vec();
                    return Some(cycle);
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
        None
    }

    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut path = Vec::new();

    for component in &system.components {
        if !visited.contains(component.name.as_str()) {
            if let Some(cycle) = has_cycle(
                component.name.as_str(),
                &graph,
                &mut visited,
                &mut rec_stack,
                &mut path,
            ) {
                diagnostics.push(
                    Diagnostic::error(
                        ErrorCode::E010_CyclicDependency,
                        format!("Cyclic dependency detected: {} -> {}", cycle.join(" -> "), cycle[0]),
                        Span::synthetic(),
                    )
                    .with_suggestion("Consider breaking the cycle by introducing an abstraction layer"),
                );
            }
        }
    }

    diagnostics
}

/// Check for dead code (unused declarations).
pub fn check_dead_code(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect all declared entities
    let mut declared: HashSet<&str> = HashSet::new();
    for component in &system.components {
        declared.insert(&component.name);
        for contained in &component.contains {
            declared.insert(contained.as_str());
        }
    }

    // Add let bindings
    for (name, _) in &system.let_bindings {
        declared.insert(name.as_str());
    }

    // Collect all referenced entities
    let mut referenced: HashSet<&str> = HashSet::new();

    // From constraints
    for constraint in &system.constraints {
        for rule in &constraint.rules {
            collect_rule_references(rule, &mut referenced);
        }
    }

    // From behavior transitions
    for behavior in &system.behaviors {
        for transition in &behavior.transitions {
            for state in transition.from.states() {
                referenced.insert(state);
            }
            for state in transition.to.states() {
                referenced.insert(state);
            }
        }
        for property in &behavior.properties {
            collect_temporal_references(&property.expr, &mut referenced);
        }
    }

    // Check for unused
    for component in &system.components {
        if !referenced.contains(component.name.as_str()) {
            // Only warn if component has no internal behavior
            if component.behaviors.is_empty() && component.components.is_empty() {
                diagnostics.push(
                    Diagnostic::warning(
                        ErrorCode::E001_UnknownIdentifier,
                        format!("Component '{}' is declared but never used", component.name),
                        component.span,
                    )
                    .with_suggestion("Remove the unused component or add references to it"),
                );
            }
        }
    }

    diagnostics
}

/// Collect references from a constraint rule.
fn collect_rule_references<'a>(rule: &'a ConstraintRule, refs: &mut HashSet<&'a str>) {
    match rule {
        ConstraintRule::Not(inner) => {
            collect_rule_references(inner, refs);
        }
        ConstraintRule::And(a, b)
        | ConstraintRule::Or(a, b)
        | ConstraintRule::Implies(a, b)
        | ConstraintRule::Iff(a, b) => {
            collect_rule_references(a, refs);
            collect_rule_references(b, refs);
        }
        ConstraintRule::Forall { domain, body, .. }
        | ConstraintRule::Exists { domain, body, .. } => {
            collect_scope_refs(domain, refs);
            collect_rule_references(body, refs);
        }
        ConstraintRule::Predicate(pred) => {
            collect_predicate_refs(pred, refs);
        }
        ConstraintRule::Call { subject, args, .. } => {
            collect_scope_refs(subject, refs);
            for arg in args {
                collect_scope_refs(arg, refs);
            }
        }
        ConstraintRule::Comparison { .. } | ConstraintRule::NFConstraint { .. } => {}
        ConstraintRule::Suppressed { rule, .. } => {
            collect_rule_references(rule, refs);
        }
    }
}

/// Collect references from a scope expression.
fn collect_scope_refs<'a>(expr: &'a ScopeExpr, refs: &mut HashSet<&'a str>) {
    match expr {
        ScopeExpr::Ident(qname) => {
            if qname.is_simple() {
                refs.insert(&qname.segments[0]);
            }
        }
        ScopeExpr::EntityList(names) => {
            for name in names {
                refs.insert(name.as_str());
            }
        }
        ScopeExpr::Union(a, b) | ScopeExpr::Intersection(a, b) | ScopeExpr::Difference(a, b) => {
            collect_scope_refs(a, refs);
            collect_scope_refs(b, refs);
        }
        ScopeExpr::Glob(_) | ScopeExpr::All | ScopeExpr::Matches { .. } | ScopeExpr::Filtered { .. } => {}
    }
}

/// Collect references from a predicate call.
fn collect_predicate_refs<'a>(pred: &'a PredicateCall, refs: &mut HashSet<&'a str>) {
    match pred {
        PredicateCall::Depends { from, to }
        | PredicateCall::References { from, to }
        | PredicateCall::DependsTransitively { from, to } => {
            collect_scope_refs(from, refs);
            for target in to {
                collect_scope_refs(target, refs);
            }
        }
        PredicateCall::Implements { entity, .. } => {
            collect_scope_refs(entity, refs);
        }
        PredicateCall::Contains { container, entities } => {
            collect_scope_refs(container, refs);
            for entity in entities {
                collect_scope_refs(entity, refs);
            }
        }
    }
}

/// Collect references from a temporal expression.
fn collect_temporal_references<'a>(expr: &'a TemporalExpr, refs: &mut HashSet<&'a str>) {
    match expr {
        TemporalExpr::State(name) | TemporalExpr::Count(name) => {
            refs.insert(name.as_str());
        }
        TemporalExpr::Always(inner)
        | TemporalExpr::Eventually(inner)
        | TemporalExpr::Next(inner)
        | TemporalExpr::Not(inner) => {
            collect_temporal_references(inner, refs);
        }
        TemporalExpr::Until { lhs, rhs }
        | TemporalExpr::Release { lhs, rhs }
        | TemporalExpr::WeakUntil { lhs, rhs }
        | TemporalExpr::StrongRelease { lhs, rhs }
        | TemporalExpr::AlwaysImplies { premise: lhs, conclusion: rhs }
        | TemporalExpr::BinOp { lhs, rhs, .. } => {
            collect_temporal_references(lhs, refs);
            collect_temporal_references(rhs, refs);
        }
        TemporalExpr::Int(_) => {}
    }
}

/// Check for missing documentation.
pub fn check_documentation(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check system description
    if system.description.is_none() {
        diagnostics.push(
            Diagnostic::info(
                ErrorCode::E011_MissingRequiredField,
                format!("System '{}' lacks a description", system.name),
                system.span,
            )
            .with_suggestion("Add a description to document the system's purpose"),
        );
    }

    // Check component implementations
    for component in &system.components {
        if component.implements.is_none() && component.behaviors.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    ErrorCode::E011_MissingRequiredField,
                    format!("Component '{}' has no implementation path or behaviors", component.name),
                    component.span,
                )
                .with_suggestion("Add an `implements` path or define behaviors"),
            );
        }
    }

    // Check behavior documentation
    for behavior in &system.behaviors {
        if behavior.properties.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    ErrorCode::E011_MissingRequiredField,
                    format!("Behavior '{}' has no temporal properties defined", behavior.name),
                    behavior.span,
                )
                .with_suggestion("Define temporal properties to specify correctness requirements"),
            );
        }
    }

    diagnostics
}

/// Check for security-related issues.
pub fn check_security(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check for sensitive component names without proper isolation
    let sensitive_names = ["auth", "security", "credential", "secret", "key", "password", "token"];
    let sensitive_components: Vec<_> = system
        .components
        .iter()
        .filter(|c| {
            sensitive_names
                .iter()
                .any(|&s| c.name.to_lowercase().contains(s))
        })
        .collect();

    // Check if sensitive components have isolation constraints
    let isolated_components: HashSet<&str> = system
        .constraints
        .iter()
        .flat_map(|c| {
            c.rules.iter().flat_map(|rule| {
                let mut isolated = Vec::new();
                if let ConstraintRule::Not(inner) = rule {
                    if let ConstraintRule::Predicate(PredicateCall::Depends { from, .. }) =
                        inner.as_ref()
                    {
                        if let ScopeExpr::Ident(qname) = from {
                            if qname.is_simple() {
                                isolated.push(qname.segments[0].as_str());
                            }
                        }
                    }
                }
                isolated
            })
        })
        .collect();

    for component in sensitive_components {
        if !isolated_components.contains(component.name.as_str()) {
            diagnostics.push(
                Diagnostic::warning(
                    ErrorCode::E016_ConstraintViolation,
                    format!(
                        "Sensitive component '{}' may not have proper isolation constraints",
                        component.name
                    ),
                    component.span,
                )
                .with_suggestion(
                    "Add constraint to limit what components can depend on this sensitive component",
                ),
            );
        }
    }

    diagnostics
}

/// Check for consistency issues.
pub fn check_consistency(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check if components_decl matches actual components
    let declared_set: HashSet<&str> = system.components_decl.iter().map(|s| s.as_str()).collect();
    let actual_set: HashSet<&str> = system.components.iter().map(|c| c.name.as_str()).collect();

    // Components declared but not defined
    for name in declared_set.difference(&actual_set) {
        diagnostics.push(
            Diagnostic::warning(
                ErrorCode::E013_ComponentNotFound,
                format!(
                    "Component '{}' is in components list but not defined",
                    name
                ),
                Span::synthetic(),
            )
            .with_suggestion("Define the component or remove it from the list"),
        );
    }

    // Components defined but not in components_decl
    for name in actual_set.difference(&declared_set) {
        if !system.components_decl.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Component '{}' is defined but not in the components list",
                        name
                    ),
                    Span::synthetic(),
                )
                .with_suggestion("Consider adding to the components list for explicit documentation"),
            );
        }
    }

    // Check behavior consistency across system and component levels
    let system_behavior_names: HashSet<&str> =
        system.behaviors.iter().map(|b| b.name.as_str()).collect();

    for component in &system.components {
        for behavior in &component.behaviors {
            if system_behavior_names.contains(behavior.name.as_str()) {
                diagnostics.push(
                    Diagnostic::warning(
                        ErrorCode::E005_DuplicateDeclaration,
                        format!(
                            "Behavior '{}' is defined both at system level and in component '{}'",
                            behavior.name, component.name
                        ),
                        behavior.span,
                    )
                    .with_suggestion(
                        "Consider renaming or consolidating the behaviors",
                    ),
                );
            }
        }
    }

    diagnostics
}

/// Check for potential simultaneous read-write confusion in effect blocks.
///
/// Warns when a variable is both written (assigned) and read (in send/emit/other assignments)
/// in the same effect block, since Intent uses declarative semantics where reads see the
/// CURRENT state and writes define the NEXT state.
pub fn check_effect_semantics(system: &SystemDecl) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Helper: collect all identifier names read in an expression
    fn collect_reads(expr: &Expr, reads: &mut HashSet<String>) {
        match expr {
            Expr::Ident(name) => {
                reads.insert(name.clone());
            }
            Expr::DottedName(path) => {
                if let Some(var) = path.strip_prefix("memory.") {
                    reads.insert(var.to_string());
                } else if let Some(first) = path.split('.').next() {
                    reads.insert(first.to_string());
                }
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    collect_reads(arg, reads);
                }
            }
            Expr::BinOp { lhs, rhs, .. }
            | Expr::CompOp { lhs, rhs, .. }
            | Expr::LogicalOp { lhs, rhs, .. }
            | Expr::SetDiff { lhs, rhs }
            | Expr::SetUnion { lhs, rhs }
            | Expr::SetIntersect { lhs, rhs }
            | Expr::In { element: lhs, set: rhs } => {
                collect_reads(lhs, reads);
                collect_reads(rhs, reads);
            }
            Expr::UnaryOp { expr, .. } => {
                collect_reads(expr, reads);
            }
            Expr::IfThenElse { cond, then_expr, else_expr } => {
                collect_reads(cond, reads);
                collect_reads(then_expr, reads);
                collect_reads(else_expr, reads);
            }
            Expr::Index { base, index } => {
                collect_reads(base, reads);
                collect_reads(index, reads);
            }
            Expr::FieldAccess { record, .. } => {
                collect_reads(record, reads);
            }
            Expr::Tuple(elems) | Expr::SetLiteral(elems) => {
                for elem in elems {
                    collect_reads(elem, reads);
                }
            }
            Expr::Record(fields) => {
                for (_, value) in fields {
                    collect_reads(value, reads);
                }
            }
            Expr::Count(name) => {
                reads.insert(name.clone());
            }
            Expr::Subset(inner) | Expr::BigUnion(inner) | Expr::Domain(inner) | Expr::Assume(inner) => {
                collect_reads(inner, reads);
            }
            Expr::Except { base, updates } => {
                collect_reads(base, reads);
                for (indices, value) in updates {
                    for idx in indices {
                        collect_reads(idx, reads);
                    }
                    collect_reads(value, reads);
                }
            }
            Expr::FunctionLiteral { domain, body, .. } => {
                collect_reads(domain, reads);
                collect_reads(body, reads);
            }
            Expr::Choose { domain, predicate, .. } => {
                collect_reads(domain, reads);
                collect_reads(predicate, reads);
            }
            Expr::Let { bindings, body } => {
                for (_, expr) in bindings {
                    collect_reads(expr, reads);
                }
                collect_reads(body, reads);
            }
            Expr::Case { arms, default } => {
                for (cond, body) in arms {
                    collect_reads(cond, reads);
                    collect_reads(body, reads);
                }
                if let Some(d) = default {
                    collect_reads(d, reads);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                collect_reads(domain, reads);
                collect_reads(body, reads);
            }
            Expr::Int(_) | Expr::Float(_) | Expr::Duration(_)
            | Expr::String(_) | Expr::Bool(_) | Expr::TlaInline { .. } => {}
        }
    }

    // Helper: collect reads from send/emit arguments in an effect list,
    // and collect all assigned (written) variables.
    fn analyze_effects(
        effects: &[EffectStmt],
        written: &mut HashSet<String>,
        send_emit_reads: &mut HashSet<String>,
    ) {
        for effect in effects {
            match &effect.kind {
                EffectKind::Assign { var, .. } => {
                    written.insert(var.clone());
                }
                EffectKind::Send { args, .. } => {
                    for arg in args {
                        collect_reads(arg, send_emit_reads);
                    }
                }
                EffectKind::Emit { args, .. } => {
                    for arg in args {
                        collect_reads(arg, send_emit_reads);
                    }
                }
                EffectKind::If { then_effects, else_effects, .. } => {
                    analyze_effects(then_effects, written, send_emit_reads);
                    if let Some(else_effs) = else_effects {
                        analyze_effects(else_effs, written, send_emit_reads);
                    }
                }
                EffectKind::Expr(_) | EffectKind::Receive { .. } => {}
            }
        }
    }

    // Check a single list of behaviors
    let check_behaviors = |behaviors: &[BehaviorDecl], diagnostics: &mut Vec<Diagnostic>| {
        for behavior in behaviors {
            for transition in &behavior.transitions {
                if transition.effects.is_empty() {
                    continue;
                }

                let mut written: HashSet<String> = HashSet::new();
                let mut send_emit_reads: HashSet<String> = HashSet::new();
                analyze_effects(&transition.effects, &mut written, &mut send_emit_reads);

                // Find variables that are both written and read in send/emit
                let mut confused: Vec<&String> = written.intersection(&send_emit_reads).collect();
                confused.sort(); // deterministic output

                for var in confused {
                    diagnostics.push(
                        Diagnostic::warning(
                            ErrorCode::E054_EffectReadWriteConfusion,
                            format!(
                                "Variable '{}' is both assigned and read in send/emit in the same effect block. \
                                 Due to declarative semantics, the send/emit will use the CURRENT value of '{}', \
                                 not the new value.",
                                var, var
                            ),
                            transition.span,
                        )
                        .with_suggestion(
                            "This is correct if intentional. If you need the new value, split into separate transitions."
                        ),
                    );
                }
            }
        }
    };

    // Check system-level behaviors
    check_behaviors(&system.behaviors, &mut diagnostics);

    // Check component-level behaviors
    for component in &system.components {
        check_behaviors(&component.behaviors, &mut diagnostics);
    }

    diagnostics
}

/// Run all additional checks and return combined diagnostics.
pub fn run_all_checks(system: &SystemDecl) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    for diag in check_cyclic_dependencies(system) {
        diagnostics.add(diag);
    }

    for diag in check_dead_code(system) {
        diagnostics.add(diag);
    }

    for diag in check_documentation(system) {
        diagnostics.add(diag);
    }

    for diag in check_security(system) {
        diagnostics.add(diag);
    }

    for diag in check_consistency(system) {
        diagnostics.add(diag);
    }

    for diag in check_effect_semantics(system) {
        diagnostics.add(diag);
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_cyclic_dependencies() {
        let source = r#"
            system Test {
                component A { depends_only [B] }
                component B { depends_only [C] }
                component C { depends_only [A] }
            }
        "#;
        let top = crate::parser::parse(source).unwrap();
        if let TopLevel::System(system) = &top[0] {
            let diags = check_cyclic_dependencies(system);
            assert!(!diags.is_empty(), "Expected cyclic dependency warning");
        }
    }

    #[test]
    fn test_check_no_cycles() {
        let source = r#"
            system Test {
                component A { depends_only [B] }
                component B { depends_only [C] }
                component C { }
            }
        "#;
        let top = crate::parser::parse(source).unwrap();
        if let TopLevel::System(system) = &top[0] {
            let diags = check_cyclic_dependencies(system);
            assert!(diags.is_empty(), "Expected no cycles: {:?}", diags);
        }
    }

    #[test]
    fn test_check_documentation() {
        let source = r#"
            system Test {
                component API { }
            }
        "#;
        let top = crate::parser::parse(source).unwrap();
        if let TopLevel::System(system) = &top[0] {
            let diags = check_documentation(system);
            assert!(!diags.is_empty(), "Expected documentation warnings");
        }
    }

    #[test]
    fn test_check_consistency() {
        let source = r#"
            system Test {
                components [A, B, C]
                component A { }
                component B { }
            }
        "#;
        let top = crate::parser::parse(source).unwrap();
        if let TopLevel::System(system) = &top[0] {
            let diags = check_consistency(system);
            assert!(!diags.is_empty(), "Expected consistency warnings about missing component C");
        }
    }
}
