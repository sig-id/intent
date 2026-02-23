//! Structured diagnostics for the Intent language.
//!
//! This module provides a comprehensive error reporting system with:
//! - Unique error codes for each type of issue
//! - Source code spans for precise location reporting
//! - Severity levels (error, warning, info, hint)
//! - Actionable suggestions for fixing issues
//! - Error recovery strategies
//! - Levenshtein-distance-based typo suggestions

pub mod recovery;
pub mod suggestions;

use std::fmt;

/// Source code span representing a range of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Start byte offset (inclusive)
    pub start: usize,
    /// End byte offset (exclusive)
    pub end: usize,
}

impl Span {
    /// Create a new span from start to end byte offsets.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Create a synthetic span for generated nodes.
    pub fn synthetic() -> Self {
        Self { start: 0, end: 0 }
    }

    /// Check if this is a synthetic span.
    pub fn is_synthetic(&self) -> bool {
        self.start == 0 && self.end == 0
    }

    /// Merge two spans into a span covering both.
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::synthetic()
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Hard error - compilation/checking cannot proceed
    Error,
    /// Warning - potential issue but not blocking
    Warning,
    /// Informational message
    Info,
    /// Hint for improvement
    Hint,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
            Severity::Hint => write!(f, "hint"),
        }
    }
}

/// Unique error codes for diagnostics.
///
/// Naming convention: `E` followed by three digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum ErrorCode {
    // === Identifier and Reference Errors (E001-E009) ===
    /// Unknown identifier
    E001_UnknownIdentifier,
    /// Type mismatch
    E002_TypeMismatch,
    /// Undefined state referenced
    E003_UndefinedState,
    /// Invalid transition (unknown source/target state)
    E004_InvalidTransition,
    /// Duplicate declaration
    E005_DuplicateDeclaration,
    /// Unreachable state
    E006_UnreachableState,
    /// Invalid pattern parameter
    E007_InvalidPatternParameter,
    /// Invalid scope expression
    E008_InvalidScopeExpression,
    /// Undefined event
    E009_UndefinedEvent,

    // === Dependency and Structure Errors (E010-E019) ===
    /// Cyclic dependency detected
    E010_CyclicDependency,
    /// Missing required field
    E011_MissingRequiredField,
    /// Invalid refinement mapping
    E012_InvalidRefinementMapping,
    /// Component not found
    E013_ComponentNotFound,
    /// Behavior not found
    E014_BehaviorNotFound,
    /// Pattern not found
    E015_PatternNotFound,
    /// Constraint violation
    E016_ConstraintViolation,

    // === State Machine Errors (E020-E029) ===
    /// Multiple initial states
    E020_MultipleInitialStates,
    /// No initial state
    E021_NoInitialState,
    /// Terminal state has outgoing transitions
    E022_TerminalStateTransitions,
    /// Duplicate transition
    E023_DuplicateTransition,
    /// Invalid fairness specification
    E024_InvalidFairness,
    /// Temporal property error
    E025_TemporalPropertyError,
    /// Potential deadlock detected
    E026_DeadlockDetected,
    /// Potential livelock detected
    E027_LivelockDetected,
    /// Invalid transition source state
    E028_InvalidTransitionSource,
    /// Invalid transition target state
    E029_InvalidTransitionTarget,

    // === Pattern and Type Errors (E030-E039) ===
    /// Pattern composition conflict
    E030_PatternCompositionConflict,
    /// Type parameter bound violation
    E031_TypeParameterBoundViolation,
    /// Incompatible pattern application
    E032_IncompatiblePatternApplication,
    /// Missing type argument
    E033_MissingTypeArgument,
    /// Invalid type annotation
    E034_InvalidTypeAnnotation,

    // === Parse Errors (E040-E049) ===
    /// Syntax error
    E040_SyntaxError,
    /// Unexpected token
    E041_UnexpectedToken,
    /// Expected token
    E042_ExpectedToken,
    /// Invalid literal
    E043_InvalidLiteral,

    // === Structural Analysis (E050-E059) ===
    /// Unsupported language
    E050_UnsupportedLanguage,
    /// Import parse error
    E051_ImportParseError,
    /// Unused import
    E052_UnusedImport,
    /// Feature not yet implemented (parsed but no backend)
    E053_UnimplementedFeature,
    /// Effect block simultaneous read-write confusion
    E054_EffectReadWriteConfusion,
}

impl ErrorCode {
    /// Get the error code as a string (e.g., "E001").
    pub fn code(&self) -> &'static str {
        match self {
            ErrorCode::E001_UnknownIdentifier => "E001",
            ErrorCode::E002_TypeMismatch => "E002",
            ErrorCode::E003_UndefinedState => "E003",
            ErrorCode::E004_InvalidTransition => "E004",
            ErrorCode::E005_DuplicateDeclaration => "E005",
            ErrorCode::E006_UnreachableState => "E006",
            ErrorCode::E007_InvalidPatternParameter => "E007",
            ErrorCode::E008_InvalidScopeExpression => "E008",
            ErrorCode::E009_UndefinedEvent => "E009",
            ErrorCode::E010_CyclicDependency => "E010",
            ErrorCode::E011_MissingRequiredField => "E011",
            ErrorCode::E012_InvalidRefinementMapping => "E012",
            ErrorCode::E013_ComponentNotFound => "E013",
            ErrorCode::E014_BehaviorNotFound => "E014",
            ErrorCode::E015_PatternNotFound => "E015",
            ErrorCode::E016_ConstraintViolation => "E016",
            ErrorCode::E020_MultipleInitialStates => "E020",
            ErrorCode::E021_NoInitialState => "E021",
            ErrorCode::E022_TerminalStateTransitions => "E022",
            ErrorCode::E023_DuplicateTransition => "E023",
            ErrorCode::E024_InvalidFairness => "E024",
            ErrorCode::E025_TemporalPropertyError => "E025",
            ErrorCode::E026_DeadlockDetected => "E026",
            ErrorCode::E027_LivelockDetected => "E027",
            ErrorCode::E028_InvalidTransitionSource => "E028",
            ErrorCode::E029_InvalidTransitionTarget => "E029",
            ErrorCode::E030_PatternCompositionConflict => "E030",
            ErrorCode::E031_TypeParameterBoundViolation => "E031",
            ErrorCode::E032_IncompatiblePatternApplication => "E032",
            ErrorCode::E033_MissingTypeArgument => "E033",
            ErrorCode::E034_InvalidTypeAnnotation => "E034",
            ErrorCode::E040_SyntaxError => "E040",
            ErrorCode::E041_UnexpectedToken => "E041",
            ErrorCode::E042_ExpectedToken => "E042",
            ErrorCode::E043_InvalidLiteral => "E043",
            ErrorCode::E050_UnsupportedLanguage => "E050",
            ErrorCode::E051_ImportParseError => "E051",
            ErrorCode::E052_UnusedImport => "E052",
            ErrorCode::E053_UnimplementedFeature => "E053",
            ErrorCode::E054_EffectReadWriteConfusion => "E054",
        }
    }

    /// Get the default severity for this error code.
    pub fn default_severity(&self) -> Severity {
        match self {
            // Info diagnostics
            ErrorCode::E050_UnsupportedLanguage => Severity::Info,
            // Warnings by default
            ErrorCode::E006_UnreachableState => Severity::Warning,
            ErrorCode::E026_DeadlockDetected => Severity::Warning,
            ErrorCode::E027_LivelockDetected => Severity::Warning,
            ErrorCode::E052_UnusedImport => Severity::Warning,
            ErrorCode::E053_UnimplementedFeature => Severity::Warning,
            ErrorCode::E054_EffectReadWriteConfusion => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// A single diagnostic message with full context.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Error code for categorization
    pub code: ErrorCode,
    /// Severity level
    pub severity: Severity,
    /// Human-readable message
    pub message: String,
    /// Source code location
    pub span: Span,
    /// Optional suggestions for fixing the issue
    pub suggestions: Vec<String>,
    /// Optional source file path
    pub file: Option<String>,
    /// Optional labels for additional context
    pub labels: Vec<DiagnosticLabel>,
}

/// A label attached to a specific source location.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticLabel {
    /// The message for this label
    pub message: String,
    /// The span this label points to
    pub span: Span,
    /// Whether this is a primary (the main issue) or secondary (related info) label
    pub primary: bool,
}

impl Diagnostic {
    /// Create a new diagnostic with the given code, message, and span.
    pub fn new(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: code.default_severity(),
            code,
            message: message.into(),
            span,
            suggestions: Vec::new(),
            file: None,
            labels: Vec::new(),
        }
    }

    /// Create an error diagnostic.
    pub fn error(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(code, message, span).with_severity(Severity::Error)
    }

    /// Create a warning diagnostic.
    pub fn warning(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(code, message, span).with_severity(Severity::Warning)
    }

    /// Create an info diagnostic.
    pub fn info(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(code, message, span).with_severity(Severity::Info)
    }

    /// Create a hint diagnostic.
    pub fn hint(code: ErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(code, message, span).with_severity(Severity::Hint)
    }

    /// Set the severity level.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Add a suggestion for fixing the issue.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add multiple suggestions.
    pub fn with_suggestions(mut self, suggestions: impl IntoIterator<Item = String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }

    /// Set the source file path.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Add a primary label.
    pub fn with_primary_label(mut self, message: impl Into<String>, span: Span) -> Self {
        self.labels.push(DiagnosticLabel {
            message: message.into(),
            span,
            primary: true,
        });
        self
    }

    /// Add a secondary label.
    pub fn with_secondary_label(mut self, message: impl Into<String>, span: Span) -> Self {
        self.labels.push(DiagnosticLabel {
            message: message.into(),
            span,
            primary: false,
        });
        self
    }

    /// Format the diagnostic for display with source code context.
    pub fn format_with_source(&self, source: &str, filename: Option<&str>) -> String {
        let mut output = String::new();

        // Header: error[E001]: message
        output.push_str(&format!(
            "{}[{}]: {}\n",
            self.severity,
            self.code,
            self.message
        ));

        // Location: --> file:line:col
        let (line, col) = offset_to_line_col(source, self.span.start);
        let file_str = filename
            .or(self.file.as_deref())
            .unwrap_or("<unknown>");
        output.push_str(&format!("  --> {}:{}:{}\n", file_str, line, col));

        // Source snippet
        if !self.span.is_synthetic() {
            let snippet = get_source_line(source, self.span.start);
            let col_start = offset_to_col(source, self.span.start);
            let underline_len = if self.span.start == self.span.end {
                1
            } else {
                (self.span.end - self.span.start).min(snippet.len() - col_start + 1)
            };

            output.push_str(&format!("   |\n"));
            output.push_str(&format!("{:3}| {}\n", line, snippet));
            output.push_str(&format!(
                "   | {}{}\n",
                " ".repeat(col_start),
                "^".repeat(underline_len)
            ));
        }

        // Labels
        for label in &self.labels {
            let (label_line, _label_col) = offset_to_line_col(source, label.span.start);
            if label_line == line {
                output.push_str(&format!(
                    "   |       {} {}: {}\n",
                    if label.primary { "!!" } else { "--" },
                    if label.primary { "primary" } else { "secondary" },
                    label.message
                ));
            } else {
                output.push_str(&format!(
                    "   |\n{:3}| {}\n   | {} {}: {}\n",
                    label_line,
                    get_source_line(source, label.span.start),
                    " ".repeat(offset_to_col(source, label.span.start)),
                    if label.primary { "!!" } else { "--" },
                    label.message
                ));
            }
        }

        // Suggestions
        if !self.suggestions.is_empty() {
            output.push_str("\n  help: ");
            for (i, suggestion) in self.suggestions.iter().enumerate() {
                if i > 0 {
                    output.push_str("\n        ");
                }
                output.push_str(suggestion);
            }
            output.push('\n');
        }

        output
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}]: {}",
            self.severity, self.code, self.message
        )?;
        if !self.span.is_synthetic() {
            write!(f, " at {}", self.span)?;
        }
        if !self.suggestions.is_empty() {
            write!(f, "\n  help: {}", self.suggestions.join("; "))?;
        }
        Ok(())
    }
}

/// A collection of diagnostics.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    /// All diagnostics
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Create an empty diagnostics collection.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add a diagnostic.
    pub fn add(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Add an error diagnostic.
    pub fn error(&mut self, code: ErrorCode, message: impl Into<String>, span: Span) {
        self.add(Diagnostic::error(code, message, span));
    }

    /// Add a warning diagnostic.
    pub fn warning(&mut self, code: ErrorCode, message: impl Into<String>, span: Span) {
        self.add(Diagnostic::warning(code, message, span));
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    /// Check if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Warning)
    }

    /// Get the number of diagnostics.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if there are no diagnostics.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get all error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.severity == Severity::Error)
    }

    /// Get all warning diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.severity == Severity::Warning)
    }

    /// Merge another diagnostics collection into this one.
    pub fn merge(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    /// Clear all diagnostics.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for diagnostic in &self.items {
            writeln!(f, "{}", diagnostic)?;
        }
        Ok(())
    }
}

// Helper functions for source location handling

/// Convert a byte offset to line and column numbers (1-indexed).
fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Get the column number for an offset (1-indexed).
fn offset_to_col(source: &str, offset: usize) -> usize {
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            col = 1;
        } else {
            col += 1;
        }
    }
    col
}

/// Get the source line containing the given offset.
fn get_source_line(source: &str, offset: usize) -> &str {
    // Find start of line
    let line_start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    // Find end of line
    let line_end = source[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(source.len());
    &source[line_start..line_end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        let span = Span::new(10, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
        assert!(!span.is_synthetic());
    }

    #[test]
    fn test_synthetic_span() {
        let span = Span::synthetic();
        assert!(span.is_synthetic());
        assert_eq!(span.start, 0);
        assert_eq!(span.end, 0);
    }

    #[test]
    fn test_span_merge() {
        let a = Span::new(5, 15);
        let b = Span::new(10, 25);
        let merged = a.merge(&b);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 25);
    }

    #[test]
    fn test_diagnostic_creation() {
        let span = Span::new(0, 10);
        let diag = Diagnostic::error(
            ErrorCode::E001_UnknownIdentifier,
            "Unknown identifier 'foo'",
            span,
        );
        assert_eq!(diag.code, ErrorCode::E001_UnknownIdentifier);
        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.message, "Unknown identifier 'foo'");
        assert_eq!(diag.span, span);
        assert!(diag.suggestions.is_empty());
    }

    #[test]
    fn test_diagnostic_with_suggestion() {
        let span = Span::new(0, 10);
        let diag = Diagnostic::error(ErrorCode::E001_UnknownIdentifier, "Unknown 'foo'", span)
            .with_suggestion("Did you mean 'bar'?");
        assert_eq!(diag.suggestions.len(), 1);
        assert_eq!(diag.suggestions[0], "Did you mean 'bar'?");
    }

    #[test]
    fn test_diagnostics_collection() {
        let mut diags = Diagnostics::new();
        assert!(diags.is_empty());
        assert!(!diags.has_errors());

        diags.error(ErrorCode::E001_UnknownIdentifier, "test", Span::synthetic());
        assert!(diags.has_errors());
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ErrorCode::E001_UnknownIdentifier.code(), "E001");
        assert_eq!(ErrorCode::E010_CyclicDependency.code(), "E010");
        assert_eq!(ErrorCode::E040_SyntaxError.code(), "E040");
    }

    #[test]
    fn test_format_with_source() {
        let source = "system Test { }";
        let span = Span::new(7, 11); // "Test"
        let diag = Diagnostic::error(
            ErrorCode::E001_UnknownIdentifier,
            "System 'Test' is not defined",
            span,
        )
        .with_file("test.intent")
        .with_suggestion("Define the system first");

        let formatted = diag.format_with_source(source, None);
        assert!(formatted.contains("error[E001]"));
        assert!(formatted.contains("System 'Test' is not defined"));
        assert!(formatted.contains("test.intent:1:8"));
        assert!(formatted.contains("help:"));
    }

    #[test]
    fn test_offset_to_line_col() {
        let source = "line one\nline two\nline three";
        assert_eq!(offset_to_line_col(source, 0), (1, 1));
        assert_eq!(offset_to_line_col(source, 9), (2, 1)); // Start of line two
        assert_eq!(offset_to_line_col(source, 10), (2, 2)); // Second char of line two
        assert_eq!(offset_to_line_col(source, 18), (3, 1)); // Start of line three
    }
}
