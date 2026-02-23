//! Behavioral module - behavior composition, refinement, and verification.
//!
//! This module handles behavioral semantics including composition, refinement
//! checking, and pattern definitions. TLA+ transpilation is in the separate
//! `transpile` module.

pub mod composition;
pub mod normalize;
pub mod patterns;
pub mod refinement;
pub mod verification;

// Re-export key types from composition
pub use composition::{
    compose_behaviors, parallel_compose, ComposedBehavior, CompositionConflict, CompositionConfig,
    ConflictStrategy, ConflictType, ParallelComposition, ParallelConfig,
};

// Re-export key types from refinement
pub use refinement::{
    validate_refinement, ComputedRefinement, RefinementResult, RefinementViolation, ViolationType,
};

// Re-export key types from verification
pub use verification::{
    verify_directory, verify_module, CheckResult, InvariantResult, ModuleVerificationResult,
    TemporalResult, VerificationConfig, VerificationMode, VerificationStatus,
};

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::{BehaviorDecl, SystemDecl};

/// Result of behavioral obligation verification.
#[derive(Debug, Clone, Serialize)]
pub struct ObligationResult {
    pub pattern: String,
    pub target: String,
    pub refines: String,
    pub concern: String,
    pub status: ObligationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ObligationStatus {
    Pass,
    Fail,
    Skipped,
}

impl std::fmt::Display for ObligationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObligationStatus::Pass => write!(f, "pass"),
            ObligationStatus::Fail => write!(f, "fail"),
            ObligationStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Options for TLA+ compilation.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Generate TLC .cfg files alongside .tla modules
    pub generate_cfg: bool,
    /// Generate Apalache-compatible output
    pub apalache: bool,
}

/// Load stdlib patterns into a PatternRegistry.
///
/// Parses `stdlib/patterns.intent` (embedded at compile time) and extracts
/// all top-level pattern declarations.
fn load_stdlib_patterns(registry: &mut patterns::PatternRegistry) {
    let source = include_str!("../../stdlib/patterns.intent");
    if let Ok(top_levels) = crate::parser::parse(source) {
        let stdlib_patterns: Vec<crate::parser::ast::PatternDecl> = top_levels
            .into_iter()
            .filter_map(|t| {
                if let crate::parser::ast::TopLevel::Pattern(p) = t {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();
        registry.load(stdlib_patterns);
    }
}

/// Expand applied patterns into a behavior, merging their states, transitions,
/// properties, and fairness specifications.
///
/// If the behavior has no `applies` entries, returns a clone as-is.
/// Otherwise, expands each PatternApplication via the registry and merges
/// the resulting elements into the behavior.
fn expand_applied_patterns(
    behavior: &BehaviorDecl,
    pattern_registry: &patterns::PatternRegistry,
) -> Result<BehaviorDecl> {
    if behavior.applies.is_empty() {
        return Ok(behavior.clone());
    }

    let mut expanded = behavior.clone();

    for app in &behavior.applies {
        match pattern_registry.expand(app) {
            Ok(expansion) => {
                // Merge expanded states
                expanded.states.extend(expansion.states);
                // Merge expanded transitions
                expanded.transitions.extend(expansion.transitions);
                // Merge expanded properties
                expanded.properties.extend(expansion.properties);
                // Merge expanded fairness specs
                expanded.fairness.extend(expansion.fairness);
            }
            Err(e) => {
                // Pattern expansion failed — log the error for diagnostics.
                // This can happen for patterns that don't have behavior blocks
                // or are not yet loaded. The linter handles unknown-pattern warnings.
                eprintln!(
                    "Warning: pattern '{}' expansion failed for behavior '{}': {}",
                    app.pattern, behavior.name, e
                );
            }
        }
    }

    Ok(expanded)
}

/// Compile TLA+ specifications from systems.
///
/// Generates TLA+ modules for each behavior in the system, including
/// full LTL temporal property transpilation and behavior composition resolution.
pub fn compile(
    systems: &[SystemDecl],
    output_dir: &Path,
    project_root: &Path,
) -> Result<Vec<PathBuf>> {
    compile_with_options(systems, output_dir, project_root, &CompileOptions::default())
}

/// Compile TLA+ specifications with options.
pub fn compile_with_options(
    systems: &[SystemDecl],
    output_dir: &Path,
    project_root: &Path,
    options: &CompileOptions,
) -> Result<Vec<PathBuf>> {
    use std::collections::HashMap;
    use std::fs;

    let mut generated = Vec::new();

    // Create output directory
    fs::create_dir_all(output_dir)?;

    for system in systems {
        // Build a registry of all behaviors in this system for composition resolution
        let mut behavior_registry: HashMap<String, &crate::parser::ast::BehaviorDecl> =
            HashMap::new();

        // Register system-level behaviors
        for behavior in &system.behaviors {
            behavior_registry.insert(behavior.name.clone(), behavior);
        }

        // Register component-level behaviors with qualified names
        for component in &system.components {
            for behavior in &component.behaviors {
                let qualified = format!("{}.{}", component.name, behavior.name);
                behavior_registry.insert(qualified, behavior);
                // Also register with just the behavior name for simple lookups
                behavior_registry.insert(behavior.name.clone(), behavior);
            }
        }

        // Build pattern registry from system-level patterns and stdlib
        let mut pattern_registry = patterns::PatternRegistry::new();
        // Load system-level patterns
        pattern_registry.load(system.patterns.clone());
        // Load stdlib patterns
        load_stdlib_patterns(&mut pattern_registry);

        // Process system-level behaviors
        for behavior in &system.behaviors {
            let result = compile_behavior_with_options(
                behavior,
                &system.name,
                &behavior_registry,
                &pattern_registry,
                project_root,
                options,
            )?;

            if result.content.is_empty() {
                continue;
            }

            // Surface TLA generation diagnostics
            for diag in &result.diagnostics {
                eprintln!(
                    "  [{}] {}: {}",
                    diag.severity, diag.code, diag.message
                );
                for suggestion in &diag.suggestions {
                    eprintln!("    help: {}", suggestion);
                }
            }

            let filename = format!("{}.tla", result.module_name);
            let path = output_dir.join(&filename);
            fs::write(&path, &result.content)?;
            generated.push(path.clone());

            // Write .cfg file if generated
            if let Some(ref cfg) = result.tlc_cfg {
                let cfg_path = output_dir.join(&cfg.filename);
                fs::write(&cfg_path, &cfg.content)?;
                generated.push(cfg_path);
            }
        }

        // Process component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                let qualified_name = format!("{}_{}", system.name, component.name);
                let result = compile_behavior_with_options(
                    behavior,
                    &qualified_name,
                    &behavior_registry,
                    &pattern_registry,
                    project_root,
                    options,
                )?;

                if result.content.is_empty() {
                    continue;
                }

                // Surface TLA generation diagnostics
                for diag in &result.diagnostics {
                    eprintln!(
                        "  [{}] {}: {}",
                        diag.severity, diag.code, diag.message
                    );
                    for suggestion in &diag.suggestions {
                        eprintln!("    help: {}", suggestion);
                    }
                }

                let filename = format!("{}.tla", result.module_name);
                let path = output_dir.join(&filename);
                fs::write(&path, &result.content)?;
                generated.push(path.clone());

                // Write .cfg file if generated
                if let Some(ref cfg) = result.tlc_cfg {
                    let cfg_path = output_dir.join(&cfg.filename);
                    fs::write(&cfg_path, &cfg.content)?;
                    generated.push(cfg_path);
                }
            }
        }
    }

    Ok(generated)
}

/// Compile a single behavior with options, resolving composition if needed.
///
/// Before generating TLA+, any applied patterns are expanded via the
/// `pattern_registry` and merged into the behavior.
fn compile_behavior_with_options(
    behavior: &crate::parser::ast::BehaviorDecl,
    system_name: &str,
    registry: &std::collections::HashMap<String, &crate::parser::ast::BehaviorDecl>,
    pattern_registry: &patterns::PatternRegistry,
    project_root: &Path,
    options: &CompileOptions,
) -> Result<crate::transpile::StateMachineTla> {
    use crate::transpile::tla;

    // Expand applied patterns into the behavior before TLA+ generation
    let behavior = expand_applied_patterns(behavior, pattern_registry)?;

    // Desugar hierarchical states into flat states before TLA+ generation
    let behavior = normalize::desugar_hierarchical_states(&behavior);

    if behavior.composes.is_empty() {
        // No composition, generate directly with config
        if options.apalache {
            return tla::generate_for_apalache(&behavior, system_name, project_root);
        } else if options.generate_cfg {
            return tla::generate_with_tlc_config(&behavior, system_name, project_root);
        } else {
            return tla::generate(&behavior, system_name, project_root, None);
        }
    }

    // Resolve composed behaviors from registry
    let mut source_behaviors: Vec<(&str, &crate::parser::ast::BehaviorDecl)> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();

    for composed_name in &behavior.composes {
        if let Some(source) = registry.get(composed_name) {
            source_behaviors.push((composed_name.as_str(), *source));
        } else {
            missing.push(composed_name.as_str());
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "behavior '{}' composes unknown behaviors: [{}]",
            behavior.name,
            missing.join(", ")
        );
    }

    // Full composition resolution
    tla::generate_composed(&behavior, &source_behaviors, system_name, None)
}



/// Verify TLA+ obligations with Apalache (fast mode).
///
/// This runs bounded model checking with Apalache on all generated TLA+ modules.
/// For exhaustive verification with TLC, use verify_exhaustive.
pub fn verify(
    obligation_dir: &Path,
    _project_root: &Path,
) -> Result<Vec<ObligationResult>> {
    let config = verification::VerificationConfig {
        mode: verification::VerificationMode::Fast,
        ..Default::default()
    };

    let results = verification::verify_directory(obligation_dir, &config)?;

    // Convert to ObligationResult format for compatibility
    let obligation_results: Vec<ObligationResult> = results
        .into_iter()
        .map(|r| {
            let status = match r.status {
                verification::VerificationStatus::Pass => ObligationStatus::Pass,
                verification::VerificationStatus::Fail => ObligationStatus::Fail,
                verification::VerificationStatus::Error => ObligationStatus::Fail,
                verification::VerificationStatus::Timeout => ObligationStatus::Skipped,
            };

            let detail = if let Some(type_check) = &r.type_check {
                if !type_check.passed {
                    format!("Type check failed: {}", type_check.detail)
                } else {
                    format!("Verified {} invariants", r.invariants.len())
                }
            } else {
                "No checks performed".to_string()
            };

            ObligationResult {
                pattern: "TLA+".to_string(),
                target: r.module.clone(),
                refines: "Specification".to_string(),
                concern: "Behavioral correctness".to_string(),
                status,
                detail,
            }
        })
        .collect();

    Ok(obligation_results)
}

/// Verify TLA+ modules with exhaustive model checking (TLC).
pub fn verify_exhaustive(
    obligation_dir: &Path,
    _project_root: &Path,
) -> Result<Vec<verification::ModuleVerificationResult>> {
    let config = verification::VerificationConfig {
        mode: verification::VerificationMode::Exhaustive,
        check_temporal: true,
        ..Default::default()
    };

    verification::verify_directory(obligation_dir, &config)
}

/// Verify TLA+ modules with both Apalache and TLC.
pub fn verify_comprehensive(
    obligation_dir: &Path,
    _project_root: &Path,
) -> Result<Vec<verification::ModuleVerificationResult>> {
    let config = verification::VerificationConfig {
        mode: verification::VerificationMode::Both,
        check_temporal: true,
        ..Default::default()
    };

    verification::verify_directory(obligation_dir, &config)
}
