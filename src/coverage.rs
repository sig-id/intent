//! Spec coverage reporting.
//!
//! Reports which components have structural constraints, behavioral specs,
//! or are unconstrained.

use crate::parser::ast::{ConstraintRule, PredicateCall, ScopeExpr, SystemDecl};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize)]
pub struct CoverageReport {
    pub system: String,
    pub components: Vec<ComponentCoverage>,
    pub summary: CoverageSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentCoverage {
    pub name: String,
    pub has_structural_constraints: bool,
    pub has_behavioral_specs: bool,
    pub has_implementation_path: bool,
    pub constraint_count: usize,
    pub behavior_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageSummary {
    pub total_components: usize,
    pub with_structural: usize,
    pub with_behavioral: usize,
    pub fully_specified: usize,
    pub unconstrained: usize,
    pub coverage_percentage: f64,
}

pub fn analyze(systems: &[SystemDecl]) -> Vec<CoverageReport> {
    systems
        .iter()
        .map(|system| {
            let mut components = Vec::new();

            // Collect which components appear in constraints
            let mut constrained_components: HashSet<String> = HashSet::new();
            let mut constraint_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            for constraint in &system.constraints {
                for rule in &constraint.rules {
                    let mut refs = HashSet::new();
                    collect_component_refs(rule, &mut refs);
                    for r in refs {
                        *constraint_counts.entry(r.clone()).or_insert(0) += 1;
                        constrained_components.insert(r);
                    }
                }
            }

            for component in &system.components {
                let has_structural = constrained_components.contains(&component.name);
                let has_behavioral = !component.behaviors.is_empty();
                let constraint_count = constraint_counts.get(&component.name).copied().unwrap_or(0);

                components.push(ComponentCoverage {
                    name: component.name.clone(),
                    has_structural_constraints: has_structural,
                    has_behavioral_specs: has_behavioral,
                    has_implementation_path: component.implements.is_some(),
                    constraint_count,
                    behavior_count: component.behaviors.len(),
                });
            }

            let total = components.len();
            let with_structural = components
                .iter()
                .filter(|c| c.has_structural_constraints)
                .count();
            let with_behavioral = components.iter().filter(|c| c.has_behavioral_specs).count();
            let fully_specified = components
                .iter()
                .filter(|c| c.has_structural_constraints && c.has_behavioral_specs)
                .count();
            let unconstrained = components
                .iter()
                .filter(|c| !c.has_structural_constraints && !c.has_behavioral_specs)
                .count();
            let coverage_pct = if total > 0 {
                ((total - unconstrained) as f64 / total as f64) * 100.0
            } else {
                100.0
            };

            CoverageReport {
                system: system.name.clone(),
                components,
                summary: CoverageSummary {
                    total_components: total,
                    with_structural,
                    with_behavioral,
                    fully_specified,
                    unconstrained,
                    coverage_percentage: coverage_pct,
                },
            }
        })
        .collect()
}

fn collect_component_refs(rule: &ConstraintRule, refs: &mut HashSet<String>) {
    match rule {
        ConstraintRule::Not(inner) => collect_component_refs(inner, refs),
        ConstraintRule::And(a, b)
        | ConstraintRule::Or(a, b)
        | ConstraintRule::Implies(a, b)
        | ConstraintRule::Iff(a, b) => {
            collect_component_refs(a, refs);
            collect_component_refs(b, refs);
        }
        ConstraintRule::Forall { domain, body, .. }
        | ConstraintRule::Exists { domain, body, .. } => {
            collect_scope_refs(domain, refs);
            collect_component_refs(body, refs);
        }
        ConstraintRule::Predicate(pred) => match pred {
            PredicateCall::Depends { from, to }
            | PredicateCall::References { from, to }
            | PredicateCall::DependsTransitively { from, to } => {
                collect_scope_refs(from, refs);
                for t in to {
                    collect_scope_refs(t, refs);
                }
            }
            PredicateCall::Implements { entity, .. } => {
                collect_scope_refs(entity, refs);
            }
            PredicateCall::Contains {
                container,
                entities,
            } => {
                collect_scope_refs(container, refs);
                for e in entities {
                    collect_scope_refs(e, refs);
                }
            }
        },
        ConstraintRule::Call { subject, args, .. } => {
            collect_scope_refs(subject, refs);
            for a in args {
                collect_scope_refs(a, refs);
            }
        }
        ConstraintRule::Suppressed { rule, .. } => collect_component_refs(rule, refs),
        _ => {}
    }
}

fn collect_scope_refs(expr: &ScopeExpr, refs: &mut HashSet<String>) {
    match expr {
        ScopeExpr::Ident(qname) if qname.is_simple() => {
            refs.insert(qname.segments[0].clone());
        }
        ScopeExpr::EntityList(names) => {
            for n in names {
                refs.insert(n.clone());
            }
        }
        ScopeExpr::Union(a, b) | ScopeExpr::Intersection(a, b) | ScopeExpr::Difference(a, b) => {
            collect_scope_refs(a, refs);
            collect_scope_refs(b, refs);
        }
        _ => {}
    }
}
