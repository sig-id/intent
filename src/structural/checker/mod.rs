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
use crate::parser::ast::ConstraintRule;

/// Dispatch a single constraint rule to the appropriate checker.
pub fn check_rule(
    rule: &ConstraintRule,
    constraint_name: &str,
    concern_name: &str,
    scopes: &HashMap<String, Vec<String>>,
    index: &CrateIndex,
) -> ConstraintResult {
    match rule {
        ConstraintRule::MustNotDependOn { from, target } => {
            let from = resolve_entity_ref(from, scopes);
            let from = expand_wildcards(&from, index);
            let entities = resolve_scope_entities(target, scopes);
            let entities = expand_wildcards(&entities, index);
            must_not_depend::check(constraint_name, concern_name, &from, &entities, index)
        }
        ConstraintRule::MustNotReference { from, targets } => {
            let from = resolve_entity_ref(from, scopes);
            let from = expand_wildcards(&from, index);
            let targets = resolve_entity_ref(targets, scopes);
            let targets = expand_wildcards(&targets, index);
            must_not_ref::check(constraint_name, concern_name, &from, &targets, index)
        }
        ConstraintRule::MustDependOn { from, target } => {
            let from = resolve_entity_ref(from, scopes);
            let from = expand_wildcards(&from, index);
            let entities = resolve_scope_entities(target, scopes);
            let entities = expand_wildcards(&entities, index);
            must_depend::check(constraint_name, concern_name, &from, &entities, index)
        }
        ConstraintRule::MustReference { from, targets } => {
            let from = resolve_entity_ref(from, scopes);
            let from = expand_wildcards(&from, index);
            let targets = resolve_entity_ref(targets, scopes);
            let targets = expand_wildcards(&targets, index);
            must_ref::check(constraint_name, concern_name, &from, &targets, index)
        }
        ConstraintRule::OccurOnlyIn { pattern, modules } => {
            let modules = resolve_entity_ref(modules, scopes);
            let modules = expand_wildcards(&modules, index);
            let pattern = expand_single_wildcard(pattern, index);
            // OccurOnlyIn with a wildcard pattern checks each expanded entity
            if pattern.len() == 1 {
                occur_only_in::check(
                    constraint_name,
                    concern_name,
                    &pattern[0],
                    &modules,
                    index,
                )
            } else {
                // Multiple expanded patterns: check each, merge violations
                let mut all_violations = Vec::new();
                for pat in &pattern {
                    let result =
                        occur_only_in::check(constraint_name, concern_name, pat, &modules, index);
                    all_violations.extend(result.violations);
                }
                ConstraintResult {
                    name: constraint_name.to_string(),
                    concern: concern_name.to_string(),
                    passed: all_violations.is_empty(),
                    violations: all_violations,
                }
            }
        }
        ConstraintRule::MustImplement {
            type_name,
            trait_name,
        } => must_implement::check(constraint_name, concern_name, type_name, trait_name, index),
        ConstraintRule::WhenPresent { .. }
        | ConstraintRule::MutuallyExclusive { .. }
        | ConstraintRule::Forall { .. }
        | ConstraintRule::Exists { .. }
        | ConstraintRule::Implies { .. }
        | ConstraintRule::Call { .. }
        | ConstraintRule::LambdaApply { .. } => {
            // WhenPresent/MutuallyExclusive: schema/data constraints (plan mode)
            // Forall/Exists/Implies/Call: v0.2 features (not yet in structural checker)
            // LambdaApply: v0.3 feature (not yet in structural checker)
            ConstraintResult {
                name: constraint_name.to_string(),
                concern: concern_name.to_string(),
                passed: true,
                violations: vec![],
            }
        }
    }
}

/// Check an `only [accessors] accesses [entities]` scope.
pub fn check_only_accesses_scope(
    scope_name: &str,
    concern_name: &str,
    accessors: &[String],
    entities: &[String],
    index: &CrateIndex,
    within: Option<&[String]>,
) -> ConstraintResult {
    only_accesses::check(scope_name, concern_name, accessors, entities, index, within)
}

/// Resolve a single-element entity ref as a scope name, otherwise pass through.
fn resolve_entity_ref(entities: &[String], scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    if entities.len() == 1 {
        resolve_scope_entities(&entities[0], scopes)
    } else {
        entities.to_vec()
    }
}

fn resolve_scope_entities(name: &str, scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    scopes
        .get(name)
        .cloned()
        .unwrap_or_else(|| vec![name.to_string()])
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

/// Expand a single entity name that may contain a wildcard.
fn expand_single_wildcard(name: &str, index: &CrateIndex) -> Vec<String> {
    if name.contains('*') {
        let re_pattern = format!("^{}$", name.replace('*', ".*"));
        if let Ok(re) = Regex::new(&re_pattern) {
            let matches: Vec<String> = index
                .entity_refs
                .keys()
                .filter(|k| re.is_match(k))
                .cloned()
                .collect();
            if !matches.is_empty() {
                return matches;
            }
        }
    }
    vec![name.to_string()]
}
