pub mod must_depend;
pub mod must_implement;
pub mod must_not_depend;
pub mod must_not_ref;
pub mod must_ref;
pub mod occur_only_in;
pub mod only_accesses;

use std::collections::HashMap;

use regex::Regex;

use super::index::CrateIndex;
use super::ConstraintResult;
use crate::parser::ast::{ConstraintRule, PredicateCall, ScopeExpr};

/// Dispatch a single constraint rule to the appropriate checker.
pub fn check_rule(
    rule: &ConstraintRule,
    constraint_name: &str,
    system_name: &str,
    scopes: &HashMap<String, Vec<String>>,
    index: &CrateIndex,
) -> ConstraintResult {
    match rule {
        // Negation: !depends(A, B) -> MustNotDependOn
        ConstraintRule::Not(inner) => {
            match inner.as_ref() {
                ConstraintRule::Predicate(PredicateCall::Depends { from, to }) => {
                    let from_entities = resolve_scope_expr(from, scopes);
                    let from_entities = expand_wildcards(&from_entities, index);
                    let to_entities = resolve_scope_expr(to, scopes);
                    let to_entities = expand_wildcards(&to_entities, index);
                    must_not_depend::check(constraint_name, system_name, &from_entities, &to_entities, index)
                }
                ConstraintRule::Predicate(PredicateCall::References { from, to }) => {
                    let from_entities = resolve_scope_expr(from, scopes);
                    let from_entities = expand_wildcards(&from_entities, index);
                    let to_entities = resolve_scope_expr(to, scopes);
                    let to_entities = expand_wildcards(&to_entities, index);
                    must_not_ref::check(constraint_name, system_name, &from_entities, &to_entities, index)
                }
                _ => ConstraintResult {
                    name: constraint_name.to_string(),
                    concern: system_name.to_string(),
                    passed: true,
                    violations: vec![],
                },
            }
        }

        // Direct predicates
        ConstraintRule::Predicate(pred) => {
            match pred {
                PredicateCall::Depends { from, to } => {
                    let from_entities = resolve_scope_expr(from, scopes);
                    let from_entities = expand_wildcards(&from_entities, index);
                    let to_entities = resolve_scope_expr(to, scopes);
                    let to_entities = expand_wildcards(&to_entities, index);
                    must_depend::check(constraint_name, system_name, &from_entities, &to_entities, index)
                }
                PredicateCall::References { from, to } => {
                    let from_entities = resolve_scope_expr(from, scopes);
                    let from_entities = expand_wildcards(&from_entities, index);
                    let to_entities = resolve_scope_expr(to, scopes);
                    let to_entities = expand_wildcards(&to_entities, index);
                    must_ref::check(constraint_name, system_name, &from_entities, &to_entities, index)
                }
                PredicateCall::Implements { entity, trait_name } => {
                    let entities = resolve_scope_expr(entity, scopes);
                    if entities.len() == 1 {
                        must_implement::check(constraint_name, system_name, &entities[0], trait_name, index)
                    } else {
                        ConstraintResult {
                            name: constraint_name.to_string(),
                            concern: system_name.to_string(),
                            passed: true,
                            violations: vec![],
                        }
                    }
                }
                PredicateCall::Contains { container, entity } => {
                    // Contains check - verify entity is within container
                    let _container = resolve_scope_expr(container, scopes);
                    let _entity = resolve_scope_expr(entity, scopes);
                    // For now, just pass - this would need actual implementation
                    ConstraintResult {
                        name: constraint_name.to_string(),
                        concern: system_name.to_string(),
                        passed: true,
                        violations: vec![],
                    }
                }
            }
        }

        // Compound rules: evaluate recursively
        ConstraintRule::And(left, right) => {
            let left_result = check_rule(left, constraint_name, system_name, scopes, index);
            if !left_result.passed {
                return left_result;
            }
            check_rule(right, constraint_name, system_name, scopes, index)
        }
        ConstraintRule::Or(left, right) => {
            let left_result = check_rule(left, constraint_name, system_name, scopes, index);
            if left_result.passed {
                return left_result;
            }
            check_rule(right, constraint_name, system_name, scopes, index)
        }
        ConstraintRule::Implies(premise, conclusion) => {
            let premise_result = check_rule(premise, constraint_name, system_name, scopes, index);
            if !premise_result.passed {
                // Premise is false, implication is vacuously true
                return ConstraintResult {
                    name: constraint_name.to_string(),
                    concern: system_name.to_string(),
                    passed: true,
                    violations: vec![],
                };
            }
            check_rule(conclusion, constraint_name, system_name, scopes, index)
        }

        // Quantifiers and calls: not yet implemented for structural checking
        ConstraintRule::Forall { .. }
        | ConstraintRule::Exists { .. }
        | ConstraintRule::Call { .. }
        | ConstraintRule::Comparison { .. } => {
            ConstraintResult {
                name: constraint_name.to_string(),
                concern: system_name.to_string(),
                passed: true,
                violations: vec![],
            }
        }
    }
}

/// Check an `only [accessors] accesses [entities]` scope.
pub fn check_only_accesses_scope(
    scope_name: &str,
    system_name: &str,
    accessors: &[String],
    entities: &[String],
    index: &CrateIndex,
    within: Option<&[String]>,
) -> ConstraintResult {
    only_accesses::check(scope_name, system_name, accessors, entities, index, within)
}

/// Resolve a scope expression to a list of entity names.
fn resolve_scope_expr(expr: &ScopeExpr, scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    match expr {
        ScopeExpr::Ident(name) => {
            scopes.get(name).cloned().unwrap_or_else(|| vec![name.clone()])
        }
        ScopeExpr::EntityList(entities) => {
            let mut result = Vec::new();
            for entity in entities {
                let resolved = scopes.get(entity).cloned().unwrap_or_else(|| vec![entity.clone()]);
                for e in resolved {
                    if !result.contains(&e) {
                        result.push(e);
                    }
                }
            }
            result
        }
        ScopeExpr::Glob(pattern) => vec![pattern.clone()],
        ScopeExpr::All => vec!["*".to_string()],
        ScopeExpr::Union(left, right) => {
            let mut result = resolve_scope_expr(left, scopes);
            for e in resolve_scope_expr(right, scopes) {
                if !result.contains(&e) {
                    result.push(e);
                }
            }
            result
        }
        ScopeExpr::Intersection(left, right) => {
            let left_set: std::collections::HashSet<_> =
                resolve_scope_expr(left, scopes).into_iter().collect();
            let right_set: std::collections::HashSet<_> =
                resolve_scope_expr(right, scopes).into_iter().collect();
            left_set.intersection(&right_set).cloned().collect()
        }
        ScopeExpr::Difference(left, right) => {
            let left_set: std::collections::HashSet<_> =
                resolve_scope_expr(left, scopes).into_iter().collect();
            let right_set: std::collections::HashSet<_> =
                resolve_scope_expr(right, scopes).into_iter().collect();
            left_set.difference(&right_set).cloned().collect()
        }
        ScopeExpr::Comprehension { pattern, .. } => vec![pattern.clone()],
    }
}

/// Expand wildcard patterns (`*Client`, `Dgraph*`) against the entity_refs index.
fn expand_wildcards(entities: &[String], index: &CrateIndex) -> Vec<String> {
    let mut result = Vec::new();
    for entity in entities {
        if entity.contains('*') {
            let re_pattern = format!("^{}$", entity.replace('*', ".*"));
            if let Ok(re) = Regex::new(&re_pattern) {
                for key in index.entity_refs.keys() {
                    if re.is_match(key) && !result.contains(key) {
                        result.push(key.clone());
                    }
                }
            }
        } else if !result.contains(entity) {
            result.push(entity.clone());
        }
    }
    result
}

/// Test-only exposure of `expand_wildcards`.
#[cfg(test)]
pub fn expand_wildcards_for_test(entities: &[String], index: &CrateIndex) -> Vec<String> {
    expand_wildcards(entities, index)
}
