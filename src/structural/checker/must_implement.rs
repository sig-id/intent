use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `TypeName must_implement TraitName`.
///
/// Looks up the trait_impls index for (trait_name, type_name).
/// If no impl is found, reports a violation.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    type_name: &str,
    trait_name: &str,
    index: &CrateIndex,
) -> ConstraintResult {
    let key = (trait_name.to_string(), type_name.to_string());
    let found = index.trait_impls.contains_key(&key);

    let violations = if found {
        vec![]
    } else {
        vec![Violation {
            file: index.codebase_root.clone(),
            line: 0,
            content: format!("no `impl {trait_name} for {type_name}` found"),
            entity: type_name.to_string(),
        }]
    };

    ConstraintResult::structural(
        constraint_name.to_string(),
        concern_name.to_string(),
        found,
        violations,
    )
}
