//! Validation pipeline for the Intent language.
//!
//! This module provides a multi-pass validation system that checks:
//! - Type correctness
//! - Entity resolution
//! - State machine well-formedness
//! - Pattern compatibility
//! - Refinement validity
//! - Deadlock detection
//! - Livelock detection
//! - Transition validation
//! - Property validation

pub mod passes;
pub mod verification;

use crate::diagnostic::{Diagnostics, Severity};
use crate::parser::ast::SystemDecl;

/// A single validation pass.
pub trait ValidationPass {
    /// The name of this pass.
    fn name(&self) -> &'static str;

    /// Run this validation pass on the system.
    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext);
}

/// Context for validation passes.
#[derive(Debug, Default)]
pub struct ValidationContext {
    /// Collected diagnostics
    pub diagnostics: Diagnostics,
}

impl ValidationContext {
    /// Create a new validation context.
    pub fn new() -> Self {
        Self {
            diagnostics: Diagnostics::new(),
        }
    }
}

/// A pipeline of validation passes.
pub struct ValidationPipeline {
    /// Ordered list of validation passes
    passes: Vec<Box<dyn ValidationPass>>,
}

impl ValidationPipeline {
    /// Create a new empty pipeline.
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    /// Create the standard validation pipeline.
    pub fn standard() -> Self {
        let mut pipeline = Self::new();
        pipeline.add_pass(passes::TypeCheckPass);
        pipeline.add_pass(passes::ExpressionTypeCheckPass);
        pipeline.add_pass(passes::EntityResolutionPass);
        pipeline.add_pass(passes::GuardEffectResolutionPass);
        pipeline.add_pass(verification::TransitionValidationPass);
        pipeline.add_pass(passes::StateReachabilityPass);
        pipeline.add_pass(verification::DeadlockDetectionPass);
        pipeline.add_pass(verification::LivelockDetectionPass);
        pipeline.add_pass(verification::PropertyValidationPass);
        pipeline.add_pass(passes::EventDeclarationPass);
        pipeline.add_pass(passes::PatternCompatibilityPass);
        pipeline.add_pass(passes::PatternConflictPass);
        pipeline.add_pass(passes::RefinementValidationPass);
        pipeline
    }

    /// Add a pass to the pipeline.
    pub fn add_pass<P: ValidationPass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }

    /// Run all passes on a system.
    pub fn run(&self, system: &SystemDecl) -> ValidationContext {
        let mut ctx = ValidationContext::new();

        for pass in &self.passes {
            pass.run(system, &mut ctx);

            // Stop on errors if configured to do so
            // For now, continue to collect all diagnostics
        }

        ctx
    }

    /// Run all passes on multiple systems.
    pub fn run_all(&self, systems: &[SystemDecl]) -> Vec<(String, ValidationContext)> {
        systems
            .iter()
            .map(|s| (s.name.clone(), self.run(s)))
            .collect()
    }
}

/// Validate a single system.
pub fn validate(system: &SystemDecl) -> Diagnostics {
    let pipeline = ValidationPipeline::standard();
    let ctx = pipeline.run(system);
    ctx.diagnostics
}

/// Validate multiple systems.
pub fn validate_all(systems: &[SystemDecl]) -> Vec<(String, Diagnostics)> {
    let pipeline = ValidationPipeline::standard();
    pipeline
        .run_all(systems)
        .into_iter()
        .map(|(name, ctx)| (name, ctx.diagnostics))
        .collect()
}

/// Check if validation results have any errors.
pub fn has_errors(results: &[(String, Diagnostics)]) -> bool {
    results.iter().any(|(_, diags)| diags.has_errors())
}

/// Print validation results.
pub fn print_results(results: &[(String, Diagnostics)]) {
    for (system_name, diagnostics) in results {
        if !diagnostics.is_empty() {
            println!("=== {} ===", system_name);
            for diag in &diagnostics.items {
                let severity_str = match diag.severity {
                    Severity::Error => "ERROR",
                    Severity::Warning => "WARN",
                    Severity::Info => "INFO",
                    Severity::Hint => "HINT",
                };
                println!(
                    "  [{}] {}: {}",
                    severity_str, diag.code, diag.message
                );
                for suggestion in &diag.suggestions {
                    println!("    Suggestion: {}", suggestion);
                }
            }
        }
    }
}
