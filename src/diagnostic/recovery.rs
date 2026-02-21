//! Error recovery strategies for the Intent language.
//!
//! This module provides strategies for recovering from errors during
//! validation and type checking, allowing the compiler to continue
//! and report multiple errors at once.

use crate::diagnostic::{Diagnostic, Diagnostics};

/// Strategy for recovering from errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Stop on the first error (fail-fast).
    FailFast,
    /// Continue collecting errors up to a maximum.
    Continue { max_errors: usize },
    /// Batch errors per phase, continue to next phase.
    BatchPerPhase { max_per_phase: usize },
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        RecoveryStrategy::Continue { max_errors: 100 }
    }
}

impl RecoveryStrategy {
    /// Create a fail-fast strategy.
    pub fn fail_fast() -> Self {
        RecoveryStrategy::FailFast
    }

    /// Create a continue strategy with a default max of 100 errors.
    pub fn continue_collecting() -> Self {
        RecoveryStrategy::default()
    }

    /// Create a continue strategy with a custom max.
    pub fn continue_with_max(max_errors: usize) -> Self {
        RecoveryStrategy::Continue { max_errors }
    }

    /// Create a batch-per-phase strategy.
    pub fn batch_per_phase(max_per_phase: usize) -> Self {
        RecoveryStrategy::BatchPerPhase { max_per_phase }
    }

    /// Check if this strategy should stop after an error.
    pub fn should_stop(&self, current_error_count: usize) -> bool {
        match self {
            RecoveryStrategy::FailFast => true,
            RecoveryStrategy::Continue { max_errors } => current_error_count >= *max_errors,
            RecoveryStrategy::BatchPerPhase { .. } => false,
        }
    }

    /// Check if this strategy should stop for a phase.
    pub fn should_stop_phase(&self, phase_error_count: usize) -> bool {
        match self {
            RecoveryStrategy::FailFast => phase_error_count > 0,
            RecoveryStrategy::Continue { .. } => false,
            RecoveryStrategy::BatchPerPhase { max_per_phase } => {
                phase_error_count >= *max_per_phase
            }
        }
    }
}

/// Context for error recovery during validation.
#[derive(Debug)]
pub struct RecoveryContext {
    /// Collected diagnostics
    diagnostics: Diagnostics,
    /// Recovery strategy
    strategy: RecoveryStrategy,
    /// Errors in current phase
    phase_error_count: usize,
    /// Total error count
    total_error_count: usize,
    /// Whether we've stopped due to error limit
    stopped: bool,
}

impl RecoveryContext {
    /// Create a new recovery context with the given strategy.
    pub fn new(strategy: RecoveryStrategy) -> Self {
        RecoveryContext {
            diagnostics: Diagnostics::new(),
            strategy,
            phase_error_count: 0,
            total_error_count: 0,
            stopped: false,
        }
    }

    /// Create a recovery context with default strategy.
    pub fn default_strategy() -> Self {
        Self::new(RecoveryStrategy::default())
    }

    /// Add a diagnostic with recovery handling.
    ///
    /// Returns `Ok(())` if collection should continue,
    /// `Err(())` if the error limit has been reached.
    pub fn add(&mut self, diagnostic: Diagnostic) -> Result<(), ()> {
        if self.stopped {
            return Err(());
        }

        let is_error = diagnostic.severity == crate::diagnostic::Severity::Error;

        self.diagnostics.add(diagnostic);

        if is_error {
            self.phase_error_count += 1;
            self.total_error_count += 1;

            if self.strategy.should_stop(self.total_error_count) {
                self.stopped = true;
                return Err(());
            }
        }

        Ok(())
    }

    /// Add a warning (always succeeds).
    pub fn add_warning(&mut self, diagnostic: Diagnostic) {
        if !self.stopped {
            self.diagnostics.add(diagnostic);
        }
    }

    /// Start a new phase (reset phase counter).
    pub fn start_phase(&mut self) {
        self.phase_error_count = 0;
    }

    /// Check if the current phase should stop.
    pub fn should_stop_phase(&self) -> bool {
        self.stopped || self.strategy.should_stop_phase(self.phase_error_count)
    }

    /// Check if we've hit the error limit.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Get the collected diagnostics.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Consume this context and return the diagnostics.
    pub fn into_diagnostics(self) -> Diagnostics {
        self.diagnostics
    }

    /// Get the total error count.
    pub fn error_count(&self) -> usize {
        self.total_error_count
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.total_error_count > 0
    }

    /// Merge diagnostics from another source.
    pub fn merge(&mut self, other: Diagnostics) {
        for diag in other.items {
            let _ = self.add(diag);
        }
    }
}

impl Default for RecoveryContext {
    fn default() -> Self {
        Self::default_strategy()
    }
}

/// Result of a validation operation with recovery.
pub type RecoveryResult<T> = Result<T, RecoveryError>;

/// Error indicating validation stopped due to error limit.
#[derive(Debug, Clone)]
pub struct RecoveryError {
    /// Message explaining why validation stopped
    pub message: String,
    /// Collected diagnostics so far
    pub diagnostics: Diagnostics,
}

impl RecoveryError {
    /// Create a new recovery error.
    pub fn new(message: impl Into<String>, diagnostics: Diagnostics) -> Self {
        RecoveryError {
            message: message.into(),
            diagnostics,
        }
    }

    /// Create a "too many errors" recovery error.
    pub fn too_many_errors(count: usize) -> Self {
        RecoveryError {
            message: format!("Stopped after {} errors", count),
            diagnostics: Diagnostics::new(),
        }
    }
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RecoveryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{ErrorCode, Severity, Span};

    fn make_error(msg: &str) -> Diagnostic {
        Diagnostic::error(ErrorCode::E001_UnknownIdentifier, msg, Span::synthetic())
    }

    fn make_warning(msg: &str) -> Diagnostic {
        Diagnostic::warning(ErrorCode::E006_UnreachableState, msg, Span::synthetic())
    }

    #[test]
    fn test_recovery_strategy_fail_fast() {
        let strategy = RecoveryStrategy::fail_fast();
        assert!(strategy.should_stop(0));
    }

    #[test]
    fn test_recovery_strategy_continue() {
        let strategy = RecoveryStrategy::continue_with_max(3);
        assert!(!strategy.should_stop(0));
        assert!(!strategy.should_stop(2));
        assert!(strategy.should_stop(3));
    }

    #[test]
    fn test_recovery_context_add_error() {
        let mut ctx = RecoveryContext::new(RecoveryStrategy::continue_with_max(2));

        let result = ctx.add(make_error("error 1"));
        assert!(result.is_ok());
        assert_eq!(ctx.error_count(), 1);

        let result = ctx.add(make_error("error 2"));
        assert!(result.is_err()); // Hit limit
        assert!(ctx.is_stopped());
    }

    #[test]
    fn test_recovery_context_add_warning() {
        let mut ctx = RecoveryContext::new(RecoveryStrategy::fail_fast());

        // Warnings don't count toward limit
        ctx.add_warning(make_warning("warning 1"));
        ctx.add_warning(make_warning("warning 2"));

        assert!(!ctx.has_errors());
        assert!(!ctx.is_stopped());
    }

    #[test]
    fn test_recovery_context_phase() {
        let mut ctx = RecoveryContext::new(RecoveryStrategy::batch_per_phase(2));

        // Phase 1
        ctx.start_phase();
        let _ = ctx.add(make_error("error 1"));
        assert!(!ctx.should_stop_phase());

        let _ = ctx.add(make_error("error 2"));
        assert!(ctx.should_stop_phase()); // Phase limit reached

        // Phase 2 - reset
        ctx.start_phase();
        assert!(!ctx.should_stop_phase()); // Reset for new phase
    }

    #[test]
    fn test_recovery_context_merge() {
        let mut ctx = RecoveryContext::new(RecoveryStrategy::continue_with_max(10));
        let mut other = Diagnostics::new();
        other.add(make_error("merged error"));

        ctx.merge(other);

        assert_eq!(ctx.error_count(), 1);
    }

    #[test]
    fn test_recovery_result() {
        fn might_fail(ctx: &mut RecoveryContext, should_fail: bool) -> RecoveryResult<()> {
            if should_fail {
                ctx.add(make_error("failed")).map_err(|_| RecoveryError::too_many_errors(1))?;
            }
            Ok(())
        }

        let mut ctx = RecoveryContext::new(RecoveryStrategy::fail_fast());
        let result = might_fail(&mut ctx, true);
        assert!(result.is_err());
    }
}
