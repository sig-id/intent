pub mod must_contain;
pub mod must_depend;
pub mod must_depend_transitively;
pub mod must_implement;
pub mod must_not_contain;
pub mod must_not_depend;
pub mod must_not_depend_transitively;
pub mod must_not_implement;
pub mod must_not_ref;
pub mod must_ref;
pub mod occur_only_in;
pub mod only_accesses;

use std::collections::HashMap;

use indexmap::IndexSet;
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
        // Negation: !A.depends(B) -> MustNotDepend
        ConstraintRule::Not(inner) => match inner.as_ref() {
            ConstraintRule::Predicate(PredicateCall::Depends { from, to }) => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_not_depend::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
            ConstraintRule::Predicate(PredicateCall::References { from, to }) => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_not_ref::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
            ConstraintRule::Predicate(PredicateCall::Contains {
                container,
                entities,
            }) => {
                let container_entities = resolve_scope_expr(container, scopes);
                let container_entities = expand_wildcards(&container_entities, index);
                let target_entities = resolve_scope_exprs(entities, scopes);
                let target_entities = expand_wildcards(&target_entities, index);
                must_not_contain::check(
                    constraint_name,
                    system_name,
                    &container_entities,
                    &target_entities,
                    index,
                )
            }
            ConstraintRule::Predicate(PredicateCall::Implements { entity, trait_name }) => {
                let entities = resolve_scope_expr(entity, scopes);
                let entities = expand_wildcards(&entities, index);
                must_not_implement::check(
                    constraint_name,
                    system_name,
                    &entities,
                    trait_name,
                    index,
                )
            }
            ConstraintRule::Predicate(PredicateCall::DependsTransitively { from, to }) => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_not_depend_transitively::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
            _ => ConstraintResult::structural(
                constraint_name.to_string(),
                system_name.to_string(),
                true,
                vec![],
            ),
        },

        // Direct predicates
        ConstraintRule::Predicate(pred) => match pred {
            PredicateCall::Depends { from, to } => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_depend::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
            PredicateCall::References { from, to } => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_ref::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
            PredicateCall::Implements { entity, trait_name } => {
                let entities = resolve_scope_expr(entity, scopes);
                let entities = expand_wildcards(&entities, index);
                let mut all_violations = vec![];
                let mut all_hold = true;
                for e in &entities {
                    let result =
                        must_implement::check(constraint_name, system_name, e, trait_name, index);
                    if !result.holds {
                        all_hold = false;
                        all_violations.extend(result.violations);
                    }
                }
                ConstraintResult::structural(
                    constraint_name.to_string(),
                    system_name.to_string(),
                    all_hold,
                    all_violations,
                )
            }
            PredicateCall::Contains {
                container,
                entities,
            } => {
                let container_entities = resolve_scope_expr(container, scopes);
                let container_entities = expand_wildcards(&container_entities, index);
                let target_entities = resolve_scope_exprs(entities, scopes);
                let target_entities = expand_wildcards(&target_entities, index);
                must_contain::check(
                    constraint_name,
                    system_name,
                    &container_entities,
                    &target_entities,
                    index,
                )
            }
            PredicateCall::DependsTransitively { from, to } => {
                let from_entities = resolve_scope_expr(from, scopes);
                let from_entities = expand_wildcards(&from_entities, index);
                let to_entities = resolve_scope_exprs(to, scopes);
                let to_entities = expand_wildcards(&to_entities, index);
                must_depend_transitively::check(
                    constraint_name,
                    system_name,
                    &from_entities,
                    &to_entities,
                    index,
                )
            }
        },

        // Compound rules: evaluate recursively
        ConstraintRule::And(left, right) => {
            let left_result = check_rule(left, constraint_name, system_name, scopes, index);
            if !left_result.holds {
                return left_result;
            }
            check_rule(right, constraint_name, system_name, scopes, index)
        }
        ConstraintRule::Or(left, right) => {
            let left_result = check_rule(left, constraint_name, system_name, scopes, index);
            if left_result.holds {
                return left_result;
            }
            check_rule(right, constraint_name, system_name, scopes, index)
        }
        ConstraintRule::Implies(premise, conclusion) => {
            let premise_result = check_rule(premise, constraint_name, system_name, scopes, index);
            if !premise_result.holds {
                // Premise is false, implication is vacuously true
                return ConstraintResult::structural(
                    constraint_name.to_string(),
                    system_name.to_string(),
                    true,
                    vec![],
                );
            }
            check_rule(conclusion, constraint_name, system_name, scopes, index)
        }
        ConstraintRule::Iff(left, right) => {
            // Iff is (a => b) && (b => a)
            let left_to_right = check_rule(
                &ConstraintRule::Implies(left.clone(), right.clone()),
                constraint_name,
                system_name,
                scopes,
                index,
            );
            if !left_to_right.holds {
                return left_to_right;
            }
            check_rule(
                &ConstraintRule::Implies(right.clone(), left.clone()),
                constraint_name,
                system_name,
                scopes,
                index,
            )
        }

        // Quantifiers: iterate over the resolved domain, binding the variable for each element
        ConstraintRule::Forall {
            var, domain, body, ..
        } => {
            let entities = resolve_scope_expr(domain, scopes);
            let entities = expand_wildcards(&entities, index);
            if entities.is_empty() {
                // Vacuously true: forall over an empty domain
                return ConstraintResult::structural(
                    constraint_name.to_string(),
                    system_name.to_string(),
                    true,
                    vec![],
                );
            }
            for entity in &entities {
                let mut scoped = scopes.clone();
                scoped.insert(var.clone(), vec![entity.clone()]);
                let result = check_rule(body, constraint_name, system_name, &scoped, index);
                if !result.holds {
                    return result;
                }
            }
            ConstraintResult::structural(
                constraint_name.to_string(),
                system_name.to_string(),
                true,
                vec![],
            )
        }
        ConstraintRule::Exists {
            var, domain, body, ..
        } => {
            let entities = resolve_scope_expr(domain, scopes);
            let entities = expand_wildcards(&entities, index);
            for entity in &entities {
                let mut scoped = scopes.clone();
                scoped.insert(var.clone(), vec![entity.clone()]);
                let result = check_rule(body, constraint_name, system_name, &scoped, index);
                if result.holds {
                    return result;
                }
            }
            // No entity satisfied the body
            ConstraintResult::structural(
                constraint_name.to_string(),
                system_name.to_string(),
                false,
                vec![],
            )
        }

        // NFConstraint: benchmark-level verification, not structurally checkable
        ConstraintRule::NFConstraint { .. } => ConstraintResult::skipped(
            constraint_name.to_string(),
            system_name.to_string(),
            super::VerificationLevel::Benchmark,
            "non-functional constraints require runtime benchmarking".to_string(),
        ),

        // Call / Comparison: not yet implemented for structural checking
        ConstraintRule::Call { .. } | ConstraintRule::Comparison { .. } => {
            ConstraintResult::skipped(
                constraint_name.to_string(),
                system_name.to_string(),
                super::VerificationLevel::Unchecked,
                format!(
                    "not yet implemented for structural checking: constraint '{}'",
                    constraint_name
                ),
            )
        }
        // Suppressed rules: pass unconditionally without evaluating the inner rule
        ConstraintRule::Suppressed { .. } => ConstraintResult::structural(
            constraint_name.to_string(),
            system_name.to_string(),
            true,
            vec![],
        ),
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

/// Resolve a scope expression to a deduplicated list of entity names (insertion-order preserving).
fn resolve_scope_expr(expr: &ScopeExpr, scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    resolve_scope_expr_set(expr, scopes).into_iter().collect()
}

/// Internal: resolve into an IndexSet for O(1) dedup during construction.
fn resolve_scope_expr_set(
    expr: &ScopeExpr,
    scopes: &HashMap<String, Vec<String>>,
) -> IndexSet<String> {
    match expr {
        ScopeExpr::Ident(qname) => {
            let name = qname.to_dotted();
            match scopes.get(&name) {
                Some(names) => names.iter().cloned().collect(),
                None => IndexSet::from([name]),
            }
        }
        ScopeExpr::EntityList(entities) => {
            let mut result = IndexSet::new();
            for entity in entities {
                let resolved = scopes
                    .get(entity)
                    .cloned()
                    .unwrap_or_else(|| vec![entity.clone()]);
                result.extend(resolved);
            }
            result
        }
        ScopeExpr::Glob(pattern) => IndexSet::from([pattern.clone()]),
        ScopeExpr::All => IndexSet::from(["*".to_string()]),
        ScopeExpr::Union(left, right) => {
            let mut result = resolve_scope_expr_set(left, scopes);
            result.extend(resolve_scope_expr_set(right, scopes));
            result
        }
        ScopeExpr::Intersection(left, right) => {
            let left_set = resolve_scope_expr_set(left, scopes);
            let right_set = resolve_scope_expr_set(right, scopes);
            &left_set & &right_set
        }
        ScopeExpr::Difference(left, right) => {
            let left_set = resolve_scope_expr_set(left, scopes);
            let right_set = resolve_scope_expr_set(right, scopes);
            &left_set - &right_set
        }
        ScopeExpr::Matches { pattern, .. } => IndexSet::from([pattern.clone()]),
        ScopeExpr::Filtered { .. } => {
            tracing::warn!(
                "filtered scope expressions are not supported in structural constraints; \
                 use explicit entity lists or forall with predicates instead"
            );
            IndexSet::new()
        }
    }
}

/// Resolve multiple scope expressions to a combined deduplicated list of entity names.
fn resolve_scope_exprs(exprs: &[ScopeExpr], scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut result = IndexSet::new();
    for expr in exprs {
        result.extend(resolve_scope_expr_set(expr, scopes));
    }
    result.into_iter().collect()
}

/// Expand wildcard patterns (`*Client`, `Dgraph*`) against all known entities in the index.
///
/// Matches against the union of entity_refs, trait_impls, imports, module tree,
/// and TS class/interface declarations, so entities that exist but have no
/// type references are still matchable by glob patterns.
fn expand_wildcards(entities: &[String], index: &CrateIndex) -> Vec<String> {
    let mut result = IndexSet::new();
    for entity in entities {
        if entity.contains('*') {
            let re_pattern = format!("^{}$", entity.replace('*', ".*"));
            if let Ok(re) = Regex::new(&re_pattern) {
                for key in &index.known_entities {
                    if re.is_match(key) {
                        result.insert(key.clone());
                    }
                }
            }
        } else {
            result.insert(entity.clone());
        }
    }
    result.into_iter().collect()
}

/// Test-only exposure of `expand_wildcards`.
#[cfg(test)]
pub fn expand_wildcards_for_test(entities: &[String], index: &CrateIndex) -> Vec<String> {
    expand_wildcards(entities, index)
}
