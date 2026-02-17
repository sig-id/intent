//! C3 linearization for deterministic component/layer ordering.
//!
//! C3 linearization (used by Python for MRO) provides:
//! - Deterministic topological ordering
//! - Detection of inconsistent hierarchies (cycles)
//! - Respect for local precedence order
//! - Monotonicity (if A precedes B in one linearization, A precedes B in all)
//!
//! When linearization fails, it indicates a dependency cycle or inconsistent ordering.

use std::collections::HashMap;

/// Result of C3 linearization.
#[derive(Debug, Clone)]
pub struct LinearizationResult {
    /// The linearized order (if successful).
    pub order: Option<Vec<String>>,
    /// Error message if linearization failed.
    pub error: Option<String>,
    /// Whether the linearization succeeded.
    pub success: bool,
}

/// Compute C3 linearization for a class/component with given parents.
///
/// This implements the C3 MRO algorithm used by Python:
/// L[C] = C + merge(L[P1], L[P2], ..., L[Pn], [P1, P2, ..., Pn])
///
/// The merge operation selects heads that don't appear in tails of other lists.
pub fn c3_linearize(
    class: &str,
    parents: &[String],
    linearizations: &HashMap<String, Vec<String>>,
) -> LinearizationResult {
    // Build the lists to merge: L[P1], L[P2], ..., plus [P1, P2, ...] for local precedence
    let mut merge_lists: Vec<Vec<String>> = Vec::new();

    for parent in parents {
        if let Some(parent_lin) = linearizations.get(parent) {
            merge_lists.push(parent_lin.clone());
        } else {
            // Parent not yet linearized - treat as just the parent itself
            merge_lists.push(vec![parent.clone()]);
        }
    }

    // Add the parents list for local precedence order
    if !parents.is_empty() {
        merge_lists.push(parents.to_vec());
    }

    // Start result with the class itself
    let mut result = vec![class.to_string()];

    // Merge until all lists are empty
    while !merge_lists.iter().all(Vec::is_empty) {
        // Find a good head: first element of some list that doesn't appear in tail of any list
        let mut found_head: Option<String> = None;

        for list in &merge_lists {
            if let Some(candidate) = list.first() {
                // Check if candidate appears in the tail of any list
                let in_tail = merge_lists.iter().any(|l| {
                    l.len() > 1 && l[1..].contains(candidate)
                });

                if !in_tail {
                    found_head = Some(candidate.clone());
                    break;
                }
            }
        }

        match found_head {
            Some(head) => {
                result.push(head.clone());
                // Remove head from the front of all lists where it appears
                for list in &mut merge_lists {
                    if list.first() == Some(&head) {
                        list.remove(0);
                    }
                }
            }
            None => {
                // No valid head found - inconsistent hierarchy
                let non_empty: Vec<_> = merge_lists
                    .iter()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.join(" <- "))
                    .collect();
                return LinearizationResult {
                    order: None,
                    error: Some(format!(
                        "Inconsistent hierarchy for '{}'. Cannot merge: [{}]",
                        class,
                        non_empty.join("], [")
                    )),
                    success: false,
                };
            }
        }
    }

    LinearizationResult {
        order: Some(result),
        error: None,
        success: true,
    }
}

/// Perform full C3 linearization on a hierarchy.
///
/// `parents_map` maps each class to its direct parents (in declaration order).
/// Returns the linearization for the specified root class.
pub fn linearize_hierarchy(
    root: &str,
    parents_map: &HashMap<String, Vec<String>>,
) -> LinearizationResult {
    let mut linearizations: HashMap<String, Vec<String>> = HashMap::new();

    // Compute linearizations bottom-up using memoization
    fn compute(
        class: &str,
        parents_map: &HashMap<String, Vec<String>>,
        linearizations: &mut HashMap<String, Vec<String>>,
        visiting: &mut Vec<String>,
    ) -> Result<Vec<String>, String> {
        // Check for cycles
        if visiting.contains(&class.to_string()) {
            return Err(format!(
                "Cycle detected: {} -> {}",
                visiting.join(" -> "),
                class
            ));
        }

        // Already computed
        if let Some(lin) = linearizations.get(class) {
            return Ok(lin.clone());
        }

        visiting.push(class.to_string());

        let parents = parents_map.get(class).cloned().unwrap_or_default();

        // Compute linearizations for all parents first
        for parent in &parents {
            compute(parent, parents_map, linearizations, visiting)?;
        }

        // Now compute this class's linearization
        let result = c3_linearize(class, &parents, linearizations);

        visiting.pop();

        match result.order {
            Some(order) => {
                linearizations.insert(class.to_string(), order.clone());
                Ok(order)
            }
            None => Err(result.error.unwrap_or_else(|| "Unknown error".to_string())),
        }
    }

    let mut visiting = Vec::new();
    match compute(root, parents_map, &mut linearizations, &mut visiting) {
        Ok(order) => LinearizationResult {
            order: Some(order),
            error: None,
            success: true,
        },
        Err(e) => LinearizationResult {
            order: None,
            error: Some(e),
            success: false,
        },
    }
}

/// Simple topological sort for layer ordering (simpler than full C3).
///
/// For layers, we just need to ensure dependencies come before dependents.
pub fn linearize(
    nodes: &[String],
    dependencies: &HashMap<String, Vec<String>>,
) -> LinearizationResult {
    use std::collections::{HashSet, VecDeque};

    let node_set: HashSet<_> = nodes.iter().cloned().collect();

    // Build in-degree map and adjacency list
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    for node in nodes {
        in_degree.entry(node.clone()).or_insert(0);
        dependents.entry(node.clone()).or_insert_with(Vec::new);
    }

    for (node, deps) in dependencies {
        if !node_set.contains(node) {
            continue;
        }
        for dep in deps {
            if node_set.contains(dep) {
                dependents.entry(dep.clone()).or_default().push(node.clone());
                *in_degree.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut queue: VecDeque<String> = nodes
        .iter()
        .filter(|n| in_degree.get(*n).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();

    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.clone());

        if let Some(deps) = dependents.get(&node) {
            for dependent in deps {
                if let Some(deg) = in_degree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
    }

    if result.len() != nodes.len() {
        let remaining: Vec<_> = nodes
            .iter()
            .filter(|n| !result.contains(n))
            .cloned()
            .collect();
        return LinearizationResult {
            order: None,
            error: Some(format!(
                "Dependency cycle detected involving: [{}]",
                remaining.join(", ")
            )),
            success: false,
        };
    }

    LinearizationResult {
        order: Some(result),
        error: None,
        success: true,
    }
}

/// Build a dependency map from the crate index for the given entities.
pub fn build_dependency_map(
    entities: &[String],
    depends_fn: impl Fn(&str) -> Vec<String>,
) -> HashMap<String, Vec<String>> {
    use std::collections::HashSet;
    let entity_set: HashSet<_> = entities.iter().cloned().collect();
    let mut deps = HashMap::new();

    for entity in entities {
        let entity_deps: Vec<String> = depends_fn(entity)
            .into_iter()
            .filter(|d| entity_set.contains(d))
            .collect();
        deps.insert(entity.clone(), entity_deps);
    }

    deps
}

/// Validate that a set of components can be linearized (no cycles/inconsistencies).
pub fn validate_ordering(
    entities: &[String],
    dependencies: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>, String> {
    let result = linearize(entities, dependencies);
    match result.order {
        Some(order) => Ok(order),
        None => Err(result.error.unwrap_or_else(|| "Unknown linearization error".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_linearization() {
        let nodes = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let mut deps = HashMap::new();
        deps.insert("B".to_string(), vec!["A".to_string()]);
        deps.insert("C".to_string(), vec!["B".to_string()]);

        let result = linearize(&nodes, &deps);
        assert!(result.success);
        let order = result.order.unwrap();
        
        // C depends on B, B depends on A, so order should be A, B, C (or similar valid order)
        let a_pos = order.iter().position(|x| x == "A").unwrap();
        let b_pos = order.iter().position(|x| x == "B").unwrap();
        let c_pos = order.iter().position(|x| x == "C").unwrap();
        
        assert!(a_pos < b_pos, "A should come before B");
        assert!(b_pos < c_pos, "B should come before C");
    }

    #[test]
    fn test_diamond_linearization() {
        // Diamond: D depends on B and C, B and C both depend on A
        let nodes = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let mut deps = HashMap::new();
        deps.insert("B".to_string(), vec!["A".to_string()]);
        deps.insert("C".to_string(), vec!["A".to_string()]);
        deps.insert("D".to_string(), vec!["B".to_string(), "C".to_string()]);

        let result = linearize(&nodes, &deps);
        assert!(result.success, "Diamond should linearize: {:?}", result.error);
        
        let order = result.order.unwrap();
        let a_pos = order.iter().position(|x| x == "A").unwrap();
        let b_pos = order.iter().position(|x| x == "B").unwrap();
        let c_pos = order.iter().position(|x| x == "C").unwrap();
        let d_pos = order.iter().position(|x| x == "D").unwrap();
        
        assert!(a_pos < b_pos);
        assert!(a_pos < c_pos);
        assert!(b_pos < d_pos);
        assert!(c_pos < d_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let nodes = vec!["A".to_string(), "B".to_string()];
        let mut deps = HashMap::new();
        deps.insert("A".to_string(), vec!["B".to_string()]);
        deps.insert("B".to_string(), vec!["A".to_string()]);

        let result = linearize(&nodes, &deps);
        assert!(!result.success, "Cycle should fail linearization");
        assert!(result.error.is_some());
    }

    #[test]
    fn test_independent_nodes() {
        let nodes = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let deps = HashMap::new();

        let result = linearize(&nodes, &deps);
        assert!(result.success);
        assert_eq!(result.order.unwrap().len(), 3);
    }

    #[test]
    fn test_layer_ordering() {
        // Typical layer setup: presentation -> application -> infrastructure
        let nodes = vec![
            "presentation".to_string(),
            "application".to_string(),
            "infrastructure".to_string(),
        ];
        let mut deps = HashMap::new();
        // Lower layers depend on higher layers (presentation uses application)
        deps.insert("presentation".to_string(), vec!["application".to_string()]);
        deps.insert("application".to_string(), vec!["infrastructure".to_string()]);

        let result = linearize(&nodes, &deps);
        assert!(result.success);
        
        let order = result.order.unwrap();
        // Infrastructure should come first (no deps), then application, then presentation
        assert_eq!(order[0], "infrastructure");
        assert_eq!(order[1], "application");
        assert_eq!(order[2], "presentation");
    }

    /// Complex C3 linearization test based on Python MRO example.
    ///
    /// Hierarchy:
    /// - A, B, C, D, E are base classes
    /// - F(A, B, C) inherits from A, B, C
    /// - G(D, B, E) inherits from D, B, E  
    /// - H(D, A) inherits from D, A
    /// - Z(F, G, H) inherits from F, G, H
    ///
    /// Expected MRO for Z: Z -> F -> G -> H -> D -> A -> B -> C -> E
    #[test]
    fn test_complex_c3_mro() {
        let mut parents: HashMap<String, Vec<String>> = HashMap::new();

        // Base classes have no parents
        parents.insert("A".to_string(), vec![]);
        parents.insert("B".to_string(), vec![]);
        parents.insert("C".to_string(), vec![]);
        parents.insert("D".to_string(), vec![]);
        parents.insert("E".to_string(), vec![]);

        // F inherits from A, B, C
        parents.insert("F".to_string(), vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
        ]);

        // G inherits from D, B, E
        parents.insert("G".to_string(), vec![
            "D".to_string(),
            "B".to_string(),
            "E".to_string(),
        ]);

        // H inherits from D, A
        parents.insert("H".to_string(), vec![
            "D".to_string(),
            "A".to_string(),
        ]);

        // Z inherits from F, G, H
        parents.insert("Z".to_string(), vec![
            "F".to_string(),
            "G".to_string(),
            "H".to_string(),
        ]);

        let result = linearize_hierarchy("Z", &parents);
        assert!(result.success, "C3 should succeed: {:?}", result.error);

        let mro = result.order.unwrap();
        
        // Expected: Z -> F -> G -> H -> D -> A -> B -> C -> E
        let expected = vec!["Z", "F", "G", "H", "D", "A", "B", "C", "E"];
        
        assert_eq!(
            mro, expected,
            "MRO mismatch.\nGot:      {:?}\nExpected: {:?}",
            mro, expected
        );
    }

    /// Test that C3 correctly rejects inconsistent hierarchies.
    ///
    /// Example: X(A, B) and Y(B, A) cannot both be parents of Z
    /// because the order of A and B is inconsistent.
    #[test]
    fn test_c3_inconsistent_hierarchy() {
        let mut parents: HashMap<String, Vec<String>> = HashMap::new();

        parents.insert("A".to_string(), vec![]);
        parents.insert("B".to_string(), vec![]);

        // X says A before B
        parents.insert("X".to_string(), vec![
            "A".to_string(),
            "B".to_string(),
        ]);

        // Y says B before A (inconsistent with X)
        parents.insert("Y".to_string(), vec![
            "B".to_string(),
            "A".to_string(),
        ]);

        // Z inherits from both X and Y - should fail
        parents.insert("Z".to_string(), vec![
            "X".to_string(),
            "Y".to_string(),
        ]);

        let result = linearize_hierarchy("Z", &parents);
        assert!(
            !result.success,
            "C3 should fail for inconsistent hierarchy"
        );
        assert!(
            result.error.is_some(),
            "Should have error message"
        );
    }

    /// Test linearization of intermediate classes.
    #[test]
    fn test_c3_intermediate_linearizations() {
        let mut parents: HashMap<String, Vec<String>> = HashMap::new();

        parents.insert("A".to_string(), vec![]);
        parents.insert("B".to_string(), vec![]);
        parents.insert("C".to_string(), vec![]);

        // F(A, B, C)
        parents.insert("F".to_string(), vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
        ]);

        let result = linearize_hierarchy("F", &parents);
        assert!(result.success);
        
        // L(F) = F + merge(L(A), L(B), L(C), [A, B, C])
        // = F + merge([A], [B], [C], [A, B, C])
        // = F, A, B, C
        let expected = vec!["F", "A", "B", "C"];
        assert_eq!(result.order.unwrap(), expected);
    }
}
