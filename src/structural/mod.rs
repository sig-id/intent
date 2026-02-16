pub mod checker;
pub mod index;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::{Concern, ConcernItem, ConstraintRule, ConstraintStatus, ScopeKind};

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

/// Check all structural constraints in the given concerns against the codebase.
///
/// Builds a `CrateIndex` once (parsing all `.rs` files with `syn`), then dispatches
/// each constraint to the appropriate checker.
pub fn check(concerns: &[Concern], codebase: &Path) -> Result<Vec<ConstraintResult>> {
    let idx = index::CrateIndex::build(codebase)?;

    let global_scopes = build_global_scope_map(concerns);
    let mut results = Vec::new();

    for concern in concerns {
        let mut scopes = collect_scopes_with_imports(concern, &global_scopes);

        // Register layers as scopes and collect in declaration order
        let mut layer_names = Vec::new();
        for item in &concern.items {
            if let ConcernItem::Layer(layer) = item {
                scopes.insert(layer.name.clone(), layer.entities.clone());
                layer_names.push(layer.name.clone());
            }
        }

        // Generate implicit layer constraints: lower layers must not depend on higher layers
        let layer_rules = generate_layer_constraints(&layer_names);

        for item in &concern.items {
            match item {
                ConcernItem::Scope(scope) => {
                    if let ScopeKind::OnlyAccesses { accessors, target } = &scope.kind {
                        let entities = resolve_scope_entities(target, &scopes);
                        let result = checker::check_only_accesses_scope(
                            &scope.name,
                            &concern.name,
                            accessors,
                            &entities,
                            &idx,
                            scope.within.as_deref(),
                        );
                        results.push(result);
                    }
                }
                ConcernItem::Constraint(constraint) => {
                    // Skip deferred constraints entirely
                    if matches!(constraint.status, Some(ConstraintStatus::Deferred)) {
                        continue;
                    }

                    // For planned constraints, only validate internal consistency
                    // (in skeleton mode we would generate stubs, but in check mode we skip codebase verification)
                    if matches!(constraint.status, Some(ConstraintStatus::Planned)) {
                        // Skip codebase verification for planned constraints
                        continue;
                    }

                    for rule in &constraint.rules {
                        let result = checker::check_rule(
                            rule,
                            &constraint.name,
                            &concern.name,
                            &scopes,
                            &idx,
                        );
                        results.push(result);
                    }
                }
                _ => {}
            }
        }

        // Check layer constraints
        for (name, rule) in &layer_rules {
            let result = checker::check_rule(rule, name, &concern.name, &scopes, &idx);
            results.push(result);
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Global scope map (cross-concern resolution)
// ---------------------------------------------------------------------------

fn build_global_scope_map(concerns: &[Concern]) -> HashMap<String, HashMap<String, Vec<String>>> {
    let mut map: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for concern in concerns {
        let mut scopes = HashMap::new();
        for item in &concern.items {
            if let ConcernItem::Scope(scope) = item {
                if let ScopeKind::EntityList(entities) = &scope.kind {
                    scopes.insert(scope.name.clone(), entities.clone());
                }
            }
        }
        map.insert(concern.name.clone(), scopes);
    }
    map
}

fn collect_scopes_with_imports(
    concern: &Concern,
    global: &HashMap<String, HashMap<String, Vec<String>>>,
) -> HashMap<String, Vec<String>> {
    let mut scopes = HashMap::new();

    // Local scopes
    for item in &concern.items {
        if let ConcernItem::Scope(scope) = item {
            if let ScopeKind::EntityList(entities) = &scope.kind {
                scopes.insert(scope.name.clone(), entities.clone());
            }
        }
    }

    // Imported scopes via `use Concern.scope`
    for item in &concern.items {
        if let ConcernItem::UseScope {
            concern: ref src_concern,
            scope: ref src_scope,
        } = item
        {
            if let Some(concern_scopes) = global.get(src_concern) {
                if let Some(entities) = concern_scopes.get(src_scope) {
                    scopes.insert(src_scope.clone(), entities.clone());
                }
            }
        }
    }

    scopes
}

fn resolve_scope_entities(name: &str, scopes: &HashMap<String, Vec<String>>) -> Vec<String> {
    scopes
        .get(name)
        .cloned()
        .unwrap_or_else(|| vec![name.to_string()])
}

/// Generate MustNotDependOn constraints for layers.
///
/// For layers declared in order [presentation, application, processing, infrastructure],
/// each lower layer must not depend on any higher layer. The `from` field uses the scope
/// name so that scope resolution in the checker expands it to the actual entities.
fn generate_layer_constraints(layer_names: &[String]) -> Vec<(String, ConstraintRule)> {
    let mut rules = Vec::new();
    for i in 1..layer_names.len() {
        for j in 0..i {
            // layer_names[i] (lower) must not depend on layer_names[j] (higher)
            let name = format!("layer_{}__not_depend_on_{}", layer_names[i], layer_names[j]);
            let rule = ConstraintRule::MustNotDependOn {
                from: vec![layer_names[i].clone()],
                target: layer_names[j].clone(),
            };
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
    fn test_check_occur_only_in() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a crate with lib.rs
        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod routes;\nmod services;\n",
        )
        .unwrap();

        let routes = tmp.path().join("routes");
        let services = tmp.path().join("services");
        std::fs::create_dir(&routes).unwrap();
        std::fs::create_dir(&services).unwrap();

        std::fs::write(
            routes.join("mod.rs"),
            "pub struct AuthMiddleware;\n",
        )
        .unwrap();
        std::fs::write(
            services.join("mod.rs"),
            "pub fn init() -> AuthMiddleware { todo!() }\npub struct AuthMiddleware;\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();
        let result = checker::occur_only_in::check(
            "auth_loc",
            "TestConcern",
            "AuthMiddleware",
            &["routes".into()],
            &idx,
        );

        assert!(!result.passed);
        assert!(
            !result.violations.is_empty(),
            "should find violations in services"
        );
        assert!(result.violations.iter().any(|v| {
            v.file.to_string_lossy().contains("services")
        }));
    }

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
            "TestConcern",
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
            "TestConcern",
            "DgraphClient",
            "GraphStore",
            &idx,
        );
        assert!(result.passed);

        // Should fail: no impl
        let result = checker::must_implement::check(
            "trait_check",
            "TestConcern",
            "DgraphClient",
            "VectorStore",
            &idx,
        );
        assert!(!result.passed);
    }

    #[test]
    fn test_scope_ref_resolution_in_from() {
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
            "use crate::storage::DgraphClient;\npub struct ServiceManager;\n",
        )
        .unwrap();
        std::fs::write(
            storage.join("mod.rs"),
            "pub struct DgraphClient;\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();

        // Use scope ref: "processing" expands to ["services"]
        let mut scopes = HashMap::new();
        scopes.insert("processing".into(), vec!["services".into()]);

        let rule = crate::parser::ast::ConstraintRule::MustNotDependOn {
            from: vec!["processing".into()],
            target: "DgraphClient".into(),
        };
        let result = checker::check_rule(&rule, "test", "TestConcern", &scopes, &idx);
        assert!(!result.passed, "scope ref 'processing' should resolve to 'services' and find violation");
    }

    #[test]
    fn test_wildcard_expansion() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod storage;\nmod services;\n",
        )
        .unwrap();

        let storage = tmp.path().join("storage");
        let services = tmp.path().join("services");
        std::fs::create_dir(&storage).unwrap();
        std::fs::create_dir(&services).unwrap();

        std::fs::write(
            storage.join("mod.rs"),
            "pub struct DgraphClient;\npub struct MilvusClient;\npub struct AppConfig;\n",
        )
        .unwrap();

        // services references DgraphClient, MilvusClient, and AppConfig
        // so they appear in entity_refs
        std::fs::write(
            services.join("mod.rs"),
            "pub fn init() -> (DgraphClient, MilvusClient, AppConfig) { todo!() }\nuse crate::storage::{DgraphClient, MilvusClient, AppConfig};\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();

        // Verify *Client expands to DgraphClient and MilvusClient
        let expanded = checker::expand_wildcards_for_test(&["*Client".into()], &idx);
        assert!(expanded.contains(&"DgraphClient".into()), "should expand *Client to DgraphClient");
        assert!(expanded.contains(&"MilvusClient".into()), "should expand *Client to MilvusClient");
        assert!(!expanded.contains(&"AppConfig".into()), "*Client should not match AppConfig");
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
            "TestConcern",
            &["services".into()],
            &["DgraphClient".into()],
            &idx,
        );
        assert!(result.passed, "services imports DgraphClient, should pass");
    }

    #[test]
    fn test_must_depend_fail() {
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
            "pub fn init() {}\n",
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
            "TestConcern",
            &["services".into()],
            &["DgraphClient".into()],
            &idx,
        );
        assert!(!result.passed, "services does not import DgraphClient, should fail");
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
            "TestConcern",
            &["services".into()],
            &["AppError".into()],
            &idx,
        );
        assert!(result.passed, "services references AppError, should pass");
    }

    #[test]
    fn test_must_reference_fail() {
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
            "pub fn init() {}\n",
        )
        .unwrap();

        let idx = index::CrateIndex::build(tmp.path()).unwrap();
        let result = checker::must_ref::check(
            "ref_check",
            "TestConcern",
            &["services".into()],
            &["AppError".into()],
            &idx,
        );
        assert!(!result.passed, "services does not reference AppError, should fail");
    }

    #[test]
    fn test_layer_generates_constraints() {
        // 3 layers: presentation > application > infrastructure
        // Should generate: application !-> presentation, infrastructure !-> presentation, infrastructure !-> application
        let rules = generate_layer_constraints(&[
            "presentation".into(),
            "application".into(),
            "infrastructure".into(),
        ]);
        assert_eq!(rules.len(), 3, "3 layers should generate 3 constraint pairs");

        // application must not depend on presentation
        assert!(rules.iter().any(|(name, _)| name.contains("application") && name.contains("presentation")));
        // infrastructure must not depend on presentation
        assert!(rules.iter().any(|(name, _)| name.contains("infrastructure") && name.contains("presentation")));
        // infrastructure must not depend on application
        assert!(rules.iter().any(|(name, _)| name.contains("infrastructure") && name.contains("application")));
    }

    #[test]
    fn test_layer_constraint_detects_violation() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod routes;\nmod storage;\n",
        )
        .unwrap();

        let routes = tmp.path().join("routes");
        let storage = tmp.path().join("storage");
        std::fs::create_dir(&routes).unwrap();
        std::fs::create_dir(&storage).unwrap();

        // storage depends on routes (violation: lower layer depends on upper)
        std::fs::write(
            storage.join("mod.rs"),
            "use crate::routes::Router;\npub struct StorageClient;\n",
        )
        .unwrap();
        std::fs::write(
            routes.join("mod.rs"),
            "pub struct Router;\n",
        )
        .unwrap();

        let concerns = crate::parser::parse_concerns(
            r#"concern X {
                layer presentation { [routes] }
                layer infrastructure { [storage] }
            }"#,
        )
        .unwrap();

        let results = check(&concerns, tmp.path()).unwrap();
        // Layer constraint: infrastructure must not depend on presentation
        let layer_result = results.iter().find(|r| r.name.contains("layer_"));
        assert!(layer_result.is_some(), "should have a layer constraint result");
        assert!(!layer_result.unwrap().passed, "storage depending on routes should be a violation");
    }
}
