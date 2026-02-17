pub mod c3;
pub mod checker;
pub mod index;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::{ConstraintRule, PredicateCall, ScopeExpr, SystemDecl};

/// Result of checking one structural constraint.
#[derive(Debug, Clone, Serialize)]
pub struct ConstraintResult {
    pub name: String,
    pub concern: String,
    pub passed: bool,
    pub violations: Vec<Violation>,
}

/// A single violation: a file + line where a forbidden reference was found.
#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub content: String,
    pub entity: String,
}

/// Check all structural constraints in the given systems against the codebase.
///
/// Builds a `CrateIndex` once (parsing all `.rs` files with `syn`), then dispatches
/// each constraint to the appropriate checker.
pub fn check(systems: &[SystemDecl], codebase: &Path) -> Result<Vec<ConstraintResult>> {
    let idx = index::CrateIndex::build(codebase)?;

    let mut results = Vec::new();

    for system in systems {
        let scopes = build_scope_map(system);

        // Check constraint rules
        for constraint in &system.constraints {
            for rule in &constraint.rules {
                let result = checker::check_rule(
                    rule,
                    &constraint.name,
                    &system.name,
                    &scopes,
                    &idx,
                );
                results.push(result);
            }
        }

        // Generate and check layer constraints from components
        let layer_rules = generate_layer_constraints(&system.components);
        for (name, rule) in &layer_rules {
            let result = checker::check_rule(rule, name, &system.name, &scopes, &idx);
            results.push(result);
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Scope map building
// ---------------------------------------------------------------------------

fn build_scope_map(system: &SystemDecl) -> HashMap<String, Vec<String>> {
    let mut scopes = HashMap::new();

    // Add component scopes based on contains
    for component in &system.components {
        if !component.contains.is_empty() {
            scopes.insert(component.name.clone(), component.contains.clone());
        }
    }

    scopes
}

/// Generate MustNotDependOn constraints for layers using C3 linearization.
///
/// C3 linearization provides deterministic ordering and detects inconsistent
/// hierarchies automatically. For components with `kind: layer`, we derive
/// the valid dependency direction from the linearized order.
fn generate_layer_constraints(components: &[crate::parser::ast::ComponentDecl]) -> Vec<(String, ConstraintRule)> {
    let mut rules = Vec::new();

    // Collect layers with their order
    let mut layers: Vec<(i64, &str)> = components
        .iter()
        .filter(|c| c.kind == crate::parser::ast::ComponentKind::Layer)
        .filter_map(|c| c.order.map(|o| (o, c.name.as_str())))
        .collect();

    // Sort by order (lowest number = highest in architecture = depended upon)
    layers.sort_by_key(|(order, _)| *order);

    // Build layer names in declaration order for C3
    let layer_names: Vec<String> = layers.iter().map(|(_, name)| name.to_string()).collect();

    // Build dependency map based on layer ordering
    // Higher-numbered layers depend on lower-numbered layers
    let mut deps = std::collections::HashMap::new();
    for i in 1..layers.len() {
        // layers[i] can depend on layers[0..i]
        let allowed_deps: Vec<String> = layers[0..i]
            .iter()
            .map(|(_, name)| name.to_string())
            .collect();
        deps.insert(layers[i].1.to_string(), allowed_deps);
    }

    // Validate with C3 linearization (will catch any inconsistencies)
    let linearization = c3::linearize(&layer_names, &deps);
    if !linearization.success {
        // If C3 fails, it means the layer structure is inconsistent
        // We still generate the constraints but they may all fail
        tracing::warn!(
            "Layer hierarchy inconsistent: {}",
            linearization.error.as_deref().unwrap_or("unknown")
        );
    }

    // Generate constraints: lower layers (higher order) must not depend on higher layers (lower order)
    for i in 1..layers.len() {
        for j in 0..i {
            // layers[i] (lower in architecture) must not depend on layers[j] (higher in architecture)
            let name = format!("layer_{}__not_depend_on_{}", layers[i].1, layers[j].1);
            let rule = ConstraintRule::Predicate(PredicateCall::Depends {
                from: ScopeExpr::Ident(layers[i].1.to_string()),
                to: vec![ScopeExpr::Ident(layers[j].1.to_string())],
            });
            // Wrap in Not to get !A.depends(B)
            let rule = ConstraintRule::Not(Box::new(rule));
            rules.push((name, rule));
        }
    }

    rules
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_check_must_not_reference() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod services;\n",
        )
        .unwrap();

        let services = tmp.path().join("services");
        std::fs::create_dir(&services).unwrap();

        std::fs::write(
            services.join("mod.rs"),
            "pub fn check() -> AuthMiddleware { todo!() }\npub struct AuthMiddleware;\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();
        let result = checker::must_not_ref::check(
            "auth_boundary",
            "TestSystem",
            &["services".into()],
            &["AuthMiddleware".into()],
            &idx,
        );

        assert!(!result.passed);
        assert!(!result.violations.is_empty());
    }

    #[test]
    fn test_check_must_implement() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            r#"
trait GraphStore { fn query(&self); }
struct DgraphClient;
impl GraphStore for DgraphClient {
    fn query(&self) {}
}
"#,
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();

        // Should pass: impl exists
        let result = checker::must_implement::check(
            "trait_check",
            "TestSystem",
            "DgraphClient",
            "GraphStore",
            &idx,
        );
        assert!(result.passed);

        // Should fail: no impl
        let result = checker::must_implement::check(
            "trait_check",
            "TestSystem",
            "DgraphClient",
            "VectorStore",
            &idx,
        );
        assert!(!result.passed);
    }

    #[test]
    fn test_must_depend_pass() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod services;\nmod storage;\n",
        )
        .unwrap();

        let services = tmp.path().join("services");
        let storage = tmp.path().join("storage");
        std::fs::create_dir(&services).unwrap();
        std::fs::create_dir(&storage).unwrap();

        std::fs::write(
            services.join("mod.rs"),
            "use crate::storage::DgraphClient;\npub fn init() {}\n",
        )
        .unwrap();
        std::fs::write(
            storage.join("mod.rs"),
            "pub struct DgraphClient;\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();
        let result = checker::must_depend::check(
            "dep_check",
            "TestSystem",
            &["services".into()],
            &["DgraphClient".into()],
            &idx,
        );
        assert!(result.passed, "services imports DgraphClient, should pass");
    }

    #[test]
    fn test_must_reference_pass() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod services;\n",
        )
        .unwrap();

        let services = tmp.path().join("services");
        std::fs::create_dir(&services).unwrap();

        std::fs::write(
            services.join("mod.rs"),
            "pub fn check() -> AppError { todo!() }\npub struct AppError;\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();
        let result = checker::must_ref::check(
            "ref_check",
            "TestSystem",
            &["services".into()],
            &["AppError".into()],
            &idx,
        );
        assert!(result.passed, "services references AppError, should pass");
    }

    #[test]
    fn test_layer_constraint_rules() {
        use crate::parser::ast::{ComponentDecl, ComponentKind};

        let components = vec![
            ComponentDecl {
                name: "presentation".into(),
                kind: ComponentKind::Layer,
                order: Some(1),
                ..Default::default()
            },
            ComponentDecl {
                name: "application".into(),
                kind: ComponentKind::Layer,
                order: Some(2),
                ..Default::default()
            },
            ComponentDecl {
                name: "infrastructure".into(),
                kind: ComponentKind::Layer,
                order: Some(3),
                ..Default::default()
            },
        ];

        let rules = generate_layer_constraints(&components);
        // 3 layers should generate 3 constraint pairs
        assert_eq!(rules.len(), 3, "3 layers should generate 3 constraint pairs");
    }

}
