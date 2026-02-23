pub mod c3;
pub mod cache;
pub mod checker;
pub mod connector;
pub mod index;

pub use connector::{ConnectorRegistry, FileAnalysisResult, LanguageConnector};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::SystemDecl;

/// Supported programming languages for structural analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Unsupported,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Language::Rust,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" => Language::JavaScript,
            _ => Language::Unsupported,
        }
    }
}

/// The level of verification applied to a constraint.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum VerificationLevel {
    /// Formally verified (model checking / proof)
    Formal,
    /// Structurally checked against codebase
    Structural,
    /// Non-functional benchmark (requires runtime measurement)
    Benchmark,
    /// Not yet implemented; skipped without evaluation
    Unchecked,
}

/// Rich status for a constraint check.
#[derive(Debug, Clone, Serialize)]
pub enum CheckStatus {
    /// Constraint was evaluated and passed
    Passed,
    /// Constraint was evaluated and failed
    Failed,
    /// Constraint was not evaluated
    Skipped { reason: String },
}

/// Result of checking one structural constraint.
///
/// The `holds` field indicates whether the constraint *assertion* is satisfied:
/// - For `must_depend(A, B)`: `holds: true` means "A does depend on B" (assertion met).
/// - For `must_not_depend(A, B)`: `holds: true` means "A does NOT depend on B" (assertion met).
/// - For compound rules (and/or/implies/forall/exists): `holds` reflects the logical result.
#[derive(Debug, Clone, Serialize)]
pub struct ConstraintResult {
    pub name: String,
    pub concern: String,
    /// Whether the constraint assertion holds (i.e. the declared invariant is satisfied).
    pub holds: bool,
    pub violations: Vec<Violation>,
    /// What level of verification was applied
    pub verification_level: VerificationLevel,
    /// Rich status (mirrors `holds` but with skip reasons)
    pub status: CheckStatus,
}

impl ConstraintResult {
    /// Create a structural result (the common case for predicate checkers).
    pub fn structural(name: String, concern: String, holds: bool, violations: Vec<Violation>) -> Self {
        let status = if holds {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        };
        Self {
            name,
            concern,
            holds,
            violations,
            verification_level: VerificationLevel::Structural,
            status,
        }
    }

    /// Create a skipped result for unimplemented or benchmark constraints.
    pub fn skipped(name: String, concern: String, level: VerificationLevel, reason: String) -> Self {
        Self {
            name,
            concern,
            holds: true,
            violations: vec![],
            verification_level: level,
            status: CheckStatus::Skipped { reason },
        }
    }
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

        // Validate dependency ordering using C3 linearization
        validate_dependency_ordering(system);

        // Emit diagnostics for unsupported languages in component directories
        for component in &system.components {
            if let Some(impl_path) = &component.implements {
                let full_path = codebase.join(impl_path);
                if full_path.is_dir() {
                    check_directory_languages(&full_path, &component.name);
                }
            }
        }

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
    }

    Ok(results)
}

/// Validate that component dependencies form a valid DAG using C3 linearization.
///
/// Components declare dependencies via `depends_only`. This validates that
/// the dependency graph is acyclic and can be linearized.
fn validate_dependency_ordering(system: &SystemDecl) {
    if system.components.is_empty() {
        return;
    }

    // Build dependency map from depends_only declarations
    let component_names: Vec<String> = system.components.iter().map(|c| c.name.clone()).collect();
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();

    for component in &system.components {
        deps.insert(component.name.clone(), component.depends_only.clone());
    }

    // Validate with C3 linearization
    let result = c3::linearize(&component_names, &deps);
    if !result.success {
        tracing::warn!(
            "Component dependency cycle in system '{}': {}",
            system.name,
            result.error.as_deref().unwrap_or("unknown")
        );
    }
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

/// Check a directory for unsupported language files and emit info diagnostics.
fn check_directory_languages(path: &Path, component_name: &str) {
    use walkdir::WalkDir;

    // List of common file extensions to skip (not code files)
    let skip_extensions = ["json", "md", "txt", "yml", "yaml", "toml", "lock", "gitignore", "env"];

    for entry in WalkDir::new(path)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Some(ext) = entry.path().extension().and_then(|s| s.to_str()) {
            let lang = Language::from_extension(ext);
            if lang == Language::Unsupported && !skip_extensions.contains(&ext) {
                tracing::info!(
                    "Component '{}': Unsupported language '.{}' in '{}'. Analysis skipped.",
                    component_name,
                    ext,
                    entry.path().display()
                );
            }
        }
    }
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

        assert!(!result.holds);
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
        assert!(result.holds);

        // Should fail: no impl
        let result = checker::must_implement::check(
            "trait_check",
            "TestSystem",
            "DgraphClient",
            "VectorStore",
            &idx,
        );
        assert!(!result.holds);
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
        assert!(result.holds, "services imports DgraphClient, should pass");
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
        assert!(result.holds, "services references AppError, should pass");
    }

}
