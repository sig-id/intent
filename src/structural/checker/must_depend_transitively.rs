use std::collections::{HashSet, VecDeque};

use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `[from_modules] must_depend_transitively_on [target_entities]`.
///
/// For each from-module, verify that a transitive dependency path exists
/// (via BFS over imports, type refs, and call refs) to at least one target entity.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for module in from_modules {
        if !is_transitively_reachable(module, target_entities, index) {
            violations.push(Violation {
                file: index.codebase_root.clone(),
                line: 0,
                content: format!(
                    "module '{}' does not transitively depend on any of [{}]",
                    module,
                    target_entities.join(", ")
                ),
                entity: module.clone(),
            });
        }
    }

    ConstraintResult::structural(
        constraint_name.to_string(),
        concern_name.to_string(),
        violations.is_empty(),
        violations,
    )
}

/// Collect all entities that a module directly depends on (imports, type_refs, call_refs).
pub fn get_direct_dependencies(module: &str, index: &CrateIndex) -> HashSet<String> {
    let mut deps = HashSet::new();
    let module_slice = &[module.to_string()];

    for (path, analysis) in &index.rust_files {
        if !index.file_is_in_modules(path, module_slice) {
            continue;
        }

        // Collect from imports
        for import in &analysis.imports {
            // Extract entity names from import segments
            if let Some(last) = import.segments.last() {
                deps.insert(last.clone());
            }
        }

        // Collect from type references
        for type_ref in &analysis.type_refs {
            deps.insert(type_ref.name.clone());
        }

        // Collect from call references
        for call_ref in &analysis.call_refs {
            deps.insert(call_ref.receiver.clone());
        }
    }

    deps
}

/// BFS reachability check: can we reach any target from start via transitive dependencies?
pub fn is_transitively_reachable(start: &str, targets: &[String], index: &CrateIndex) -> bool {
    let target_set: HashSet<&str> = targets.iter().map(|s| s.as_str()).collect();

    // Check direct dependency first
    if super::must_depend::check("", "", &[start.to_string()], targets, index).holds {
        return true;
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    visited.insert(start.to_string());
    // Seed BFS with direct dependencies of start
    let direct_deps = get_direct_dependencies(start, index);
    for dep in &direct_deps {
        if target_set.contains(dep.as_str()) {
            return true;
        }
        if !visited.contains(dep) {
            visited.insert(dep.clone());
            queue.push_back(dep.clone());
        }
    }

    // BFS over dependency graph
    while let Some(current) = queue.pop_front() {
        let deps = get_direct_dependencies(&current, index);
        for dep in deps {
            if target_set.contains(dep.as_str()) {
                return true;
            }
            if !visited.contains(&dep) {
                visited.insert(dep.clone());
                queue.push_back(dep);
            }
        }
    }

    false
}
