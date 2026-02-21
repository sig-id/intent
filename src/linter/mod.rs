//! Linter for the Intent language.
//!
//! This module provides comprehensive syntax checking and linting capabilities:
//! - Parse error detection
//! - Semantic validation
//! - Style and best practice checks
//! - Dead code detection
//!
//! # Example
//!
//! ```
//! use intent::linter::{Linter, LinterConfig, LintResult};
//!
//! let config = LinterConfig::default();
//! let linter = Linter::new(config);
//! let result = linter.lint_file("example.intent", "system Test { }");
//!
//! for diagnostic in &result.diagnostics.items {
//!     println!("{}", diagnostic);
//! }
//! ```

mod checks;

use crate::diagnostic::{Diagnostic, Diagnostics, ErrorCode, Severity, Span};
use crate::parser::{self, ast::*};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Configuration for the linter.
#[derive(Debug, Clone)]
pub struct LinterConfig {
    /// Enable pedantic checks (more warnings)
    pub pedantic: bool,
    /// Allow unused components
    pub allow_unused: bool,
    /// Maximum allowed state machine complexity
    pub max_states: usize,
    /// Maximum allowed transitions
    pub max_transitions: usize,
    /// Require descriptions on systems
    pub require_descriptions: bool,
    /// Check for naming conventions
    pub check_naming: bool,
    /// Enabled lint rules
    pub enabled_rules: HashSet<LintRule>,
    /// Disabled lint rules
    pub disabled_rules: HashSet<LintRule>,
}

impl Default for LinterConfig {
    fn default() -> Self {
        let enabled_rules: HashSet<LintRule> = [
            LintRule::UndefinedIdentifier,
            LintRule::UnusedComponent,
            LintRule::UnusedState,
            LintRule::DuplicateDeclaration,
            LintRule::InvalidTransition,
            LintRule::UnreachableState,
            LintRule::MissingInitialState,
            LintRule::MultipleInitialStates,
            LintRule::TerminalStateTransitions,
            LintRule::NamingConvention,
            LintRule::MissingDescription,
            LintRule::EmptyBlock,
            LintRule::DeprecatedSyntax,
            LintRule::CyclicalDependency,
            LintRule::PatternNotFound,
            LintRule::InvalidPatternParameter,
            LintRule::MissingTerminalState,
        ].iter().cloned().collect();

        Self {
            pedantic: false,
            allow_unused: false,
            max_states: 100,
            max_transitions: 500,
            require_descriptions: false,
            check_naming: true,
            enabled_rules,
            disabled_rules: HashSet::new(),
        }
    }
}

/// A lint rule identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LintRule {
    // === Syntax & Parsing ===
    /// Syntax errors from parsing
    SyntaxError,
    /// Unexpected token
    UnexpectedToken,

    // === Identifier & Reference ===
    /// Reference to undefined identifier
    UndefinedIdentifier,
    /// Unused component declaration
    UnusedComponent,
    /// Unused state declaration
    UnusedState,
    /// Duplicate declaration
    DuplicateDeclaration,

    // === State Machine ===
    /// Invalid transition (unknown state)
    InvalidTransition,
    /// State is unreachable from initial
    UnreachableState,
    /// No initial state defined
    MissingInitialState,
    /// Multiple initial states defined
    MultipleInitialStates,
    /// Terminal state has outgoing transitions
    TerminalStateTransitions,
    /// No terminal state defined
    MissingTerminalState,

    // === Style & Best Practices ===
    /// Naming convention violation
    NamingConvention,
    /// Missing description
    MissingDescription,
    /// Empty block
    EmptyBlock,
    /// Deprecated syntax usage
    DeprecatedSyntax,

    // === Dependency ===
    /// Cyclical dependency detected
    CyclicalDependency,

    // === Pattern ===
    /// Applied pattern not found
    PatternNotFound,
    /// Invalid pattern parameter
    InvalidPatternParameter,
}

impl LintRule {
    /// Get the error code for this lint rule.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            LintRule::SyntaxError => ErrorCode::E040_SyntaxError,
            LintRule::UnexpectedToken => ErrorCode::E041_UnexpectedToken,
            LintRule::UndefinedIdentifier => ErrorCode::E001_UnknownIdentifier,
            LintRule::UnusedComponent => ErrorCode::E001_UnknownIdentifier,
            LintRule::UnusedState => ErrorCode::E006_UnreachableState,
            LintRule::DuplicateDeclaration => ErrorCode::E005_DuplicateDeclaration,
            LintRule::InvalidTransition => ErrorCode::E004_InvalidTransition,
            LintRule::UnreachableState => ErrorCode::E006_UnreachableState,
            LintRule::MissingInitialState => ErrorCode::E021_NoInitialState,
            LintRule::MultipleInitialStates => ErrorCode::E020_MultipleInitialStates,
            LintRule::TerminalStateTransitions => ErrorCode::E022_TerminalStateTransitions,
            LintRule::MissingTerminalState => ErrorCode::E021_NoInitialState,
            LintRule::NamingConvention => ErrorCode::E001_UnknownIdentifier,
            LintRule::MissingDescription => ErrorCode::E011_MissingRequiredField,
            LintRule::EmptyBlock => ErrorCode::E040_SyntaxError,
            LintRule::DeprecatedSyntax => ErrorCode::E041_UnexpectedToken,
            LintRule::CyclicalDependency => ErrorCode::E010_CyclicDependency,
            LintRule::PatternNotFound => ErrorCode::E015_PatternNotFound,
            LintRule::InvalidPatternParameter => ErrorCode::E007_InvalidPatternParameter,
        }
    }

    /// Get the default severity for this rule.
    pub fn default_severity(&self) -> Severity {
        match self {
            LintRule::SyntaxError => Severity::Error,
            LintRule::UnexpectedToken => Severity::Error,
            LintRule::UndefinedIdentifier => Severity::Error,
            LintRule::UnusedComponent => Severity::Warning,
            LintRule::UnusedState => Severity::Warning,
            LintRule::DuplicateDeclaration => Severity::Error,
            LintRule::InvalidTransition => Severity::Error,
            LintRule::UnreachableState => Severity::Warning,
            LintRule::MissingInitialState => Severity::Error,
            LintRule::MultipleInitialStates => Severity::Error,
            LintRule::TerminalStateTransitions => Severity::Warning,
            LintRule::MissingTerminalState => Severity::Info,
            LintRule::NamingConvention => Severity::Hint,
            LintRule::MissingDescription => Severity::Info,
            LintRule::EmptyBlock => Severity::Warning,
            LintRule::DeprecatedSyntax => Severity::Warning,
            LintRule::CyclicalDependency => Severity::Error,
            LintRule::PatternNotFound => Severity::Error,
            LintRule::InvalidPatternParameter => Severity::Error,
        }
    }

    /// Get the rule name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            LintRule::SyntaxError => "syntax-error",
            LintRule::UnexpectedToken => "unexpected-token",
            LintRule::UndefinedIdentifier => "undefined-identifier",
            LintRule::UnusedComponent => "unused-component",
            LintRule::UnusedState => "unused-state",
            LintRule::DuplicateDeclaration => "duplicate-declaration",
            LintRule::InvalidTransition => "invalid-transition",
            LintRule::UnreachableState => "unreachable-state",
            LintRule::MissingInitialState => "missing-initial-state",
            LintRule::MultipleInitialStates => "multiple-initial-states",
            LintRule::TerminalStateTransitions => "terminal-state-transitions",
            LintRule::MissingTerminalState => "missing-terminal-state",
            LintRule::NamingConvention => "naming-convention",
            LintRule::MissingDescription => "missing-description",
            LintRule::EmptyBlock => "empty-block",
            LintRule::DeprecatedSyntax => "deprecated-syntax",
            LintRule::CyclicalDependency => "cyclical-dependency",
            LintRule::PatternNotFound => "pattern-not-found",
            LintRule::InvalidPatternParameter => "invalid-pattern-parameter",
        }
    }
}

/// Result of linting a file.
#[derive(Debug, Clone)]
pub struct LintResult {
    /// File path that was linted
    pub file: PathBuf,
    /// Source code that was linted
    pub source: String,
    /// Parsed top-level declarations (if parsing succeeded)
    pub top_levels: Vec<TopLevel>,
    /// All diagnostics collected
    pub diagnostics: Diagnostics,
    /// Whether the file passed linting (no errors)
    pub passed: bool,
}

impl LintResult {
    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Check if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics.has_warnings()
    }

    /// Get all error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.errors()
    }

    /// Get all warning diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.warnings()
    }

    /// Format diagnostics with source code context.
    pub fn format_diagnostics(&self) -> String {
        let mut output = String::new();
        for diagnostic in &self.diagnostics.items {
            output.push_str(&diagnostic.format_with_source(&self.source, self.file.to_str()));
            output.push('\n');
        }
        output
    }
}

/// The main linter struct.
pub struct Linter {
    config: LinterConfig,
}

impl Linter {
    /// Create a new linter with the given configuration.
    pub fn new(config: LinterConfig) -> Self {
        Self { config }
    }

    /// Create a linter with default configuration.
    pub fn default_linter() -> Self {
        Self::new(LinterConfig::default())
    }

    /// Check if a lint rule is enabled.
    fn is_rule_enabled(&self, rule: LintRule) -> bool {
        !self.config.disabled_rules.contains(&rule)
            && (self.config.enabled_rules.contains(&rule) || self.config.enabled_rules.is_empty())
    }

    /// Lint a single file.
    pub fn lint_file<P: AsRef<Path>>(&self, path: P, source: &str) -> LintResult {
        let path = path.as_ref().to_path_buf();
        let mut diagnostics = Diagnostics::new();

        // Phase 1: Parse the file
        let top_levels = match parser::parse(source) {
            Ok(tops) => tops,
            Err(e) => {
                diagnostics.add(Diagnostic::error(
                    ErrorCode::E040_SyntaxError,
                    e.to_string(),
                    Span::synthetic(),
                ).with_file(path.to_string_lossy().to_string()));

                return LintResult {
                    file: path,
                    source: source.to_string(),
                    top_levels: Vec::new(),
                    diagnostics,
                    passed: false,
                };
            }
        };

        // Phase 2: Run semantic checks
        for top_level in &top_levels {
            self.check_top_level(top_level, &mut diagnostics, source);
        }

        // Phase 3: Run style checks
        if self.config.check_naming {
            for top_level in &top_levels {
                self.check_naming(top_level, &mut diagnostics);
            }
        }

        // Phase 4: Run pedantic checks if enabled
        if self.config.pedantic {
            for top_level in &top_levels {
                self.check_pedantic(top_level, &mut diagnostics);
            }
        }

        LintResult {
            file: path,
            source: source.to_string(),
            top_levels,
            passed: !diagnostics.has_errors(),
            diagnostics,
        }
    }

    /// Lint multiple files.
    pub fn lint_files(&self, files: &[(PathBuf, String)]) -> Vec<LintResult> {
        files
            .iter()
            .map(|(path, source)| self.lint_file(path, source))
            .collect()
    }

    /// Check a top-level declaration.
    fn check_top_level(&self, top: &TopLevel, diagnostics: &mut Diagnostics, _source: &str) {
        match top {
            TopLevel::System(system) => {
                self.check_system(system, diagnostics);
            }
            TopLevel::Pattern(pattern) => {
                self.check_pattern(pattern, diagnostics);
            }
            TopLevel::Rationale(rationale) => {
                self.check_rationale(rationale, diagnostics);
            }
            TopLevel::Distilled(distilled) => {
                self.check_distilled(distilled, diagnostics);
            }
            TopLevel::Predicate(predicate) => {
                self.check_predicate(predicate, diagnostics);
            }
            TopLevel::Import(import) => {
                self.check_import(import, diagnostics);
            }
            TopLevel::Event(event) => {
                self.check_event(event, diagnostics);
            }
        }
    }

    /// Check a system declaration.
    fn check_system(&self, system: &SystemDecl, diagnostics: &mut Diagnostics) {
        // Check for description
        if self.config.require_descriptions && system.description.is_none() {
            if self.is_rule_enabled(LintRule::MissingDescription) {
                diagnostics.add(Diagnostic::new(
                    LintRule::MissingDescription.error_code(),
                    format!("System '{}' is missing a description", system.name),
                    system.span,
                ).with_severity(Severity::Warning)
                 .with_suggestion("Add a description to document the system's purpose"));
            }
        }

        // Collect all declared entities
        let mut declared_components: HashSet<&str> = HashSet::new();
        let mut component_names: Vec<&str> = Vec::new();

        for component in &system.components {
            if declared_components.contains(component.name.as_str()) {
                if self.is_rule_enabled(LintRule::DuplicateDeclaration) {
                    diagnostics.add(Diagnostic::error(
                        LintRule::DuplicateDeclaration.error_code(),
                        format!("Duplicate component declaration: '{}'", component.name),
                        component.span,
                    ).with_suggestion("Remove or rename the duplicate component"));
                }
            } else {
                declared_components.insert(&component.name);
                component_names.push(&component.name);
            }

            // Add contained entities
            for contained in &component.contains {
                declared_components.insert(contained.as_str());
            }
        }

        // Add events as declared entities (usable in constraints)
        for event in &system.events {
            declared_components.insert(&event.name);
        }

        // Check for duplicate declarations in components_decl
        let mut seen_components: HashSet<&str> = HashSet::new();
        for name in &system.components_decl {
            if !seen_components.insert(name.as_str()) {
                if self.is_rule_enabled(LintRule::DuplicateDeclaration) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::DuplicateDeclaration.error_code(),
                        format!("Duplicate component in components list: '{}'", name),
                        Span::synthetic(),
                    ));
                }
            }
        }

        // Check system-level behaviors
        for behavior in &system.behaviors {
            self.check_behavior(behavior, &declared_components, diagnostics);
        }

        // Check component-level behaviors and references
        for component in &system.components {
            self.check_component(component, &declared_components, diagnostics);
        }

        // Check constraints
        for constraint in &system.constraints {
            self.check_constraint(constraint, &declared_components, diagnostics);
        }

        // Check pattern applications
        // Include stdlib patterns (defined in stdlib/patterns.intent)
        let stdlib_patterns: HashSet<&'static str> = [
            "EventSourced", "Stateful", "Heartbeat", "CircuitBreaker",
            "Saga", "TwoPhaseCommit", "Outbox", "Inbox", "Idempotent",
            "Retry", "Timeout", "RateLimiter", "Bulkhead",
        ].iter().cloned().collect();
        let pattern_names: HashSet<&str> = system.patterns.iter().map(|p| p.name.as_str()).collect();
        for applies in &system.applies {
            let pattern_name = applies.pattern.name();
            let pattern_found = pattern_names.contains(pattern_name)
                || stdlib_patterns.contains(pattern_name);
            if !pattern_found {
                if self.is_rule_enabled(LintRule::PatternNotFound) {
                    let mut available: Vec<&str> = pattern_names.iter().cloned().collect();
                    available.extend(stdlib_patterns.iter().cloned());
                    diagnostics.add(Diagnostic::error(
                        LintRule::PatternNotFound.error_code(),
                        format!("Pattern '{}' not found", applies.pattern),
                        Span::synthetic(),
                    ).with_suggestion(format!(
                        "Available patterns: {}",
                        available.join(", ")
                    )));
                }
            }
        }

        // Check for unused components
        if !self.config.allow_unused {
            self.check_unused_components(system, &declared_components, diagnostics);
        }
    }

    /// Check a component declaration.
    fn check_component(
        &self,
        component: &ComponentDecl,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        // Check depends_only references
        for dep in &component.depends_only {
            if !declared.contains(dep.as_str()) {
                if self.is_rule_enabled(LintRule::UndefinedIdentifier) {
                    diagnostics.add(Diagnostic::error(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Component '{}' in depends_only not found", dep),
                        component.span,
                    ).with_suggestion(format!(
                        "Available: {}",
                        declared.iter().cloned().collect::<Vec<_>>().join(", ")
                    )));
                }
            }
        }

        // Check component behaviors
        for behavior in &component.behaviors {
            self.check_behavior(behavior, declared, diagnostics);
        }

        // Check nested components
        for nested in &component.components {
            self.check_component(nested, declared, diagnostics);
        }

        // Check behavior bindings
        for binding in &component.binds {
            // Could check if the behavior/pattern exists
            let _ = binding;
        }
    }

    /// Check a behavior declaration.
    fn check_behavior(
        &self,
        behavior: &BehaviorDecl,
        _declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        let state_names: HashSet<&str> = behavior.states.iter().map(|s| s.name.as_str()).collect();

        // Check for multiple initial states
        let initial_states: Vec<_> = behavior.states.iter().filter(|s| s.initial).collect();
        if initial_states.len() > 1 && self.is_rule_enabled(LintRule::MultipleInitialStates) {
            let names: Vec<_> = initial_states.iter().map(|s| s.name.as_str()).collect();
            diagnostics.add(Diagnostic::error(
                LintRule::MultipleInitialStates.error_code(),
                format!("Behavior '{}' has multiple initial states: {}", behavior.name, names.join(", ")),
                behavior.span,
            ).with_suggestion("Only one state should be marked as initial"));
        }

        // Check for missing initial state
        if initial_states.is_empty() && self.is_rule_enabled(LintRule::MissingInitialState) {
            diagnostics.add(Diagnostic::error(
                LintRule::MissingInitialState.error_code(),
                format!("Behavior '{}' has no initial state", behavior.name),
                behavior.span,
            ).with_suggestion("Add `{ initial: true }` to one state"));
        }

        // Check terminal states
        let terminal_states: HashSet<&str> = behavior
            .states
            .iter()
            .filter(|s| s.terminal)
            .map(|s| s.name.as_str())
            .collect();

        // Check for missing terminal state (only a hint)
        if terminal_states.is_empty() && self.is_rule_enabled(LintRule::MissingTerminalState) {
            diagnostics.add(Diagnostic::new(
                LintRule::MissingTerminalState.error_code(),
                format!("Behavior '{}' has no terminal state", behavior.name),
                behavior.span,
            ).with_severity(Severity::Info)
             .with_suggestion("Consider adding a terminal state to indicate completion"));
        }

        // Compute reachable states
        let reachable = self.compute_reachable_states(behavior);

        // Check transitions
        for transition in &behavior.transitions {
            // Check source states
            for from_state in transition.from.states() {
                if !state_names.contains(from_state) && !transition.from.is_wildcard() {
                    if self.is_rule_enabled(LintRule::InvalidTransition) {
                        diagnostics.add(Diagnostic::error(
                            LintRule::InvalidTransition.error_code(),
                            format!("Transition from undefined state '{}' in behavior '{}'", from_state, behavior.name),
                            transition.span,
                        ).with_suggestion(format!("Available states: {}", state_names.iter().cloned().collect::<Vec<_>>().join(", "))));
                    }
                }
            }

            // Check target states
            for to_state in transition.to.states() {
                if !state_names.contains(to_state) {
                    if self.is_rule_enabled(LintRule::InvalidTransition) {
                        diagnostics.add(Diagnostic::error(
                            LintRule::InvalidTransition.error_code(),
                            format!("Transition to undefined state '{}' in behavior '{}'", to_state, behavior.name),
                            transition.span,
                        ));
                    }
                }
            }

            // Check terminal state with outgoing transition
            if let Some(from) = transition.from.as_state() {
                if terminal_states.contains(from) && self.is_rule_enabled(LintRule::TerminalStateTransitions) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::TerminalStateTransitions.error_code(),
                        format!("Terminal state '{}' has outgoing transition in behavior '{}'", from, behavior.name),
                        transition.span,
                    ));
                }
            }
        }

        // Check unreachable states
        for state in &behavior.states {
            if !reachable.contains(&state.name) && !state.initial {
                if self.is_rule_enabled(LintRule::UnreachableState) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::UnreachableState.error_code(),
                        format!("State '{}' in behavior '{}' is unreachable", state.name, behavior.name),
                        Span::synthetic(),
                    ).with_suggestion("Add a transition to this state or remove it"));
                }
            }
        }

        // Check temporal properties
        for property in &behavior.properties {
            self.check_temporal_expr(&property.expr, &state_names, diagnostics);
        }

        // Check fairness specifications
        for fairness in &behavior.fairness {
            if !state_names.contains(fairness.from.as_str()) {
                if self.is_rule_enabled(LintRule::InvalidTransition) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::InvalidTransition.error_code(),
                        format!("Fairness references undefined state '{}' in behavior '{}'", fairness.from, behavior.name),
                        behavior.span,
                    ));
                }
            }
            if !state_names.contains(fairness.to.as_str()) {
                if self.is_rule_enabled(LintRule::InvalidTransition) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::InvalidTransition.error_code(),
                        format!("Fairness references undefined state '{}' in behavior '{}'", fairness.to, behavior.name),
                        behavior.span,
                    ));
                }
            }
            for alt in &fairness.alts {
                if !state_names.contains(alt.as_str()) {
                    if self.is_rule_enabled(LintRule::InvalidTransition) {
                        diagnostics.add(Diagnostic::warning(
                            LintRule::InvalidTransition.error_code(),
                            format!("Fairness alternative references undefined state '{}' in behavior '{}'", alt, behavior.name),
                            behavior.span,
                        ));
                    }
                }
            }
        }

        // Check invariants (TLA+ expressions)
        for invariant in &behavior.invariants {
            self.check_expr(&invariant.expr, &state_names, diagnostics);
        }
    }

    /// Compute all reachable states from initial states.
    fn compute_reachable_states(&self, behavior: &BehaviorDecl) -> HashSet<String> {
        let mut reachable = HashSet::new();

        // Start from initial states
        for state in &behavior.states {
            if state.initial {
                reachable.insert(state.name.clone());
            }
        }

        // BFS to find all reachable states
        let mut changed = true;
        while changed {
            changed = false;
            for transition in &behavior.transitions {
                // Check if any source state is reachable
                let source_reachable = match &transition.from {
                    TransitionSource::State(s) => reachable.contains(s),
                    TransitionSource::Wildcard => true,
                    TransitionSource::States(states) => states.iter().any(|s| reachable.contains(s)),
                };

                if source_reachable {
                    for target in transition.to.states() {
                        if reachable.insert(target.to_string()) {
                            changed = true;
                        }
                    }
                }
            }
        }

        reachable
    }

    /// Check a temporal expression for undefined state references.
    fn check_temporal_expr(
        &self,
        expr: &TemporalExpr,
        state_names: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        match expr {
            TemporalExpr::State(name) => {
                if !state_names.contains(name.as_str()) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Temporal property references undefined state '{}'", name),
                        Span::synthetic(),
                    ));
                }
            }
            TemporalExpr::Count(name) => {
                if !state_names.contains(name.as_str()) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Count references undefined state '{}'", name),
                        Span::synthetic(),
                    ));
                }
            }
            TemporalExpr::Always(inner)
            | TemporalExpr::Eventually(inner)
            | TemporalExpr::Next(inner)
            | TemporalExpr::Not(inner) => {
                self.check_temporal_expr(inner, state_names, diagnostics);
            }
            TemporalExpr::Until { lhs, rhs }
            | TemporalExpr::Release { lhs, rhs }
            | TemporalExpr::WeakUntil { lhs, rhs }
            | TemporalExpr::StrongRelease { lhs, rhs }
            | TemporalExpr::AlwaysImplies { premise: lhs, conclusion: rhs }
            | TemporalExpr::BinOp { lhs, rhs, .. } => {
                self.check_temporal_expr(lhs, state_names, diagnostics);
                self.check_temporal_expr(rhs, state_names, diagnostics);
            }
            TemporalExpr::Int(_) => {}
        }
    }

    /// Check an expression for undefined identifiers and other issues.
    fn check_expr(
        &self,
        expr: &Expr,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        match expr {
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Duration(_)
            | Expr::String(_)
            | Expr::Bool(_) => {}

            Expr::Ident(name) => {
                if !declared.contains(name.as_str()) {
                    diagnostics.add(Diagnostic::hint(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Identifier '{}' may not be defined in this context", name),
                        Span::synthetic(),
                    ));
                }
            }

            Expr::DottedName(path) => {
                // Check the first segment
                if let Some(first) = path.split('.').next() {
                    if !declared.contains(first) {
                        diagnostics.add(Diagnostic::hint(
                            LintRule::UndefinedIdentifier.error_code(),
                            format!("Identifier '{}' in path '{}' may not be defined", first, path),
                            Span::synthetic(),
                        ));
                    }
                }
            }

            Expr::Call { name, args } => {
                let _ = name;
                for arg in args {
                    self.check_expr(arg, declared, diagnostics);
                }
            }

            Expr::BinOp { lhs, rhs, .. }
            | Expr::CompOp { lhs, rhs, .. }
            | Expr::LogicalOp { lhs, rhs, .. }
            | Expr::SetDiff { lhs, rhs }
            | Expr::SetUnion { lhs, rhs }
            | Expr::SetIntersect { lhs, rhs }
            | Expr::In { element: lhs, set: rhs } => {
                self.check_expr(lhs, declared, diagnostics);
                self.check_expr(rhs, declared, diagnostics);
            }

            Expr::UnaryOp { expr, .. } => {
                self.check_expr(expr, declared, diagnostics);
            }

            Expr::Count(name) => {
                if !declared.contains(name.as_str()) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Count references undefined identifier '{}'", name),
                        Span::synthetic(),
                    ));
                }
            }

            // TLA+ primitives

            Expr::Choose { var, domain, predicate } => {
                self.check_expr(domain, declared, diagnostics);
                // Add loop variable to scope for predicate
                let mut declared_with_var = declared.clone();
                declared_with_var.insert(var.as_str());
                self.check_expr(predicate, &declared_with_var, diagnostics);
            }

            Expr::Let { bindings, body } => {
                // Check binding values with current scope
                for (_, binding_expr) in bindings {
                    self.check_expr(binding_expr, declared, diagnostics);
                }
                // Add binding names to scope for body
                let mut declared_with_bindings = declared.clone();
                for (name, _) in bindings {
                    declared_with_bindings.insert(name.as_str());
                }
                self.check_expr(body, &declared_with_bindings, diagnostics);
            }

            Expr::IfThenElse { cond, then_expr, else_expr } => {
                self.check_expr(cond, declared, diagnostics);
                self.check_expr(then_expr, declared, diagnostics);
                self.check_expr(else_expr, declared, diagnostics);
            }

            Expr::Case { arms, default } => {
                for (cond, body) in arms {
                    self.check_expr(cond, declared, diagnostics);
                    self.check_expr(body, declared, diagnostics);
                }
                if let Some(default_expr) = default {
                    self.check_expr(default_expr, declared, diagnostics);
                }
            }

            Expr::Subset(expr)
            | Expr::BigUnion(expr)
            | Expr::Domain(expr)
            | Expr::Assume(expr) => {
                self.check_expr(expr, declared, diagnostics);
            }

            Expr::Except { base, updates } => {
                self.check_expr(base, declared, diagnostics);
                for (indices, value) in updates {
                    for idx in indices {
                        self.check_expr(idx, declared, diagnostics);
                    }
                    self.check_expr(value, declared, diagnostics);
                }
            }

            Expr::FunctionLiteral { var, domain, body } => {
                self.check_expr(domain, declared, diagnostics);
                // Add function variable to scope for body
                let mut declared_with_var = declared.clone();
                declared_with_var.insert(var.as_str());
                self.check_expr(body, &declared_with_var, diagnostics);
            }

            Expr::Record(fields) => {
                for (_, value) in fields {
                    self.check_expr(value, declared, diagnostics);
                }
            }

            Expr::FieldAccess { record, .. } => {
                self.check_expr(record, declared, diagnostics);
            }

            Expr::Tuple(elems) | Expr::SetLiteral(elems) => {
                for elem in elems {
                    self.check_expr(elem, declared, diagnostics);
                }
            }

            Expr::Index { base, index } => {
                self.check_expr(base, declared, diagnostics);
                self.check_expr(index, declared, diagnostics);
            }

            Expr::Forall { var, domain, body }
            | Expr::Exists { var, domain, body } => {
                self.check_expr(domain, declared, diagnostics);
                // Add quantifier variable to scope for body
                let mut declared_with_var = declared.clone();
                declared_with_var.insert(var.as_str());
                self.check_expr(body, &declared_with_var, diagnostics);
            }

            Expr::TlaInline { .. } => {
                // Inline TLA+ code is not checked
            }
        }
    }

    /// Check a constraint declaration.
    fn check_constraint(
        &self,
        constraint: &ConstraintDecl,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        for rule in &constraint.rules {
            self.check_constraint_rule(rule, declared, diagnostics);
        }
    }

    /// Check a constraint rule.
    fn check_constraint_rule(
        &self,
        rule: &ConstraintRule,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        match rule {
            ConstraintRule::Not(inner) => {
                self.check_constraint_rule(inner, declared, diagnostics);
            }
            ConstraintRule::And(a, b)
            | ConstraintRule::Or(a, b)
            | ConstraintRule::Implies(a, b)
            | ConstraintRule::Iff(a, b) => {
                self.check_constraint_rule(a, declared, diagnostics);
                self.check_constraint_rule(b, declared, diagnostics);
            }
            ConstraintRule::Forall { var, domain, body, .. }
            | ConstraintRule::Exists { var, domain, body, .. } => {
                self.check_scope_expr(domain, declared, diagnostics);
                // Add loop variable to declared set for body checking
                let mut declared_with_var = declared.clone();
                declared_with_var.insert(var.as_str());
                self.check_constraint_rule(body, &declared_with_var, diagnostics);
            }
            ConstraintRule::Predicate(pred) => {
                self.check_predicate_call(pred, declared, diagnostics);
            }
            ConstraintRule::Call { subject, args, .. } => {
                self.check_scope_expr(subject, declared, diagnostics);
                for arg in args {
                    self.check_scope_expr(arg, declared, diagnostics);
                }
            }
            ConstraintRule::Comparison { .. } | ConstraintRule::NFConstraint { .. } => {}
            ConstraintRule::Suppressed { rule, .. } => {
                self.check_constraint_rule(rule, declared, diagnostics);
            }
        }
    }

    /// Check a scope expression.
    fn check_scope_expr(
        &self,
        expr: &ScopeExpr,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        match expr {
            ScopeExpr::Ident(qname) => {
                let dotted = qname.to_dotted();
                if qname.is_simple() && !declared.contains(dotted.as_str()) {
                    diagnostics.add(Diagnostic::warning(
                        LintRule::UndefinedIdentifier.error_code(),
                        format!("Unknown identifier '{}' in scope expression", dotted),
                        Span::synthetic(),
                    ));
                }
            }
            ScopeExpr::EntityList(names) => {
                for name in names {
                    if !declared.contains(name.as_str()) {
                        diagnostics.add(Diagnostic::warning(
                            LintRule::UndefinedIdentifier.error_code(),
                            format!("Entity '{}' may not be defined", name),
                            Span::synthetic(),
                        ));
                    }
                }
            }
            ScopeExpr::Union(a, b)
            | ScopeExpr::Intersection(a, b)
            | ScopeExpr::Difference(a, b) => {
                self.check_scope_expr(a, declared, diagnostics);
                self.check_scope_expr(b, declared, diagnostics);
            }
            ScopeExpr::Glob(_) | ScopeExpr::All | ScopeExpr::Matches { .. } => {}
            ScopeExpr::Filtered { condition, .. } => {
                let _ = condition;
            }
        }
    }

    /// Check a predicate call.
    fn check_predicate_call(
        &self,
        pred: &PredicateCall,
        declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        match pred {
            PredicateCall::Depends { from, to }
            | PredicateCall::References { from, to } => {
                self.check_scope_expr(from, declared, diagnostics);
                for target in to {
                    self.check_scope_expr(target, declared, diagnostics);
                }
            }
            PredicateCall::Implements { entity, .. } => {
                self.check_scope_expr(entity, declared, diagnostics);
            }
            PredicateCall::Contains { container, entities } => {
                self.check_scope_expr(container, declared, diagnostics);
                for entity in entities {
                    self.check_scope_expr(entity, declared, diagnostics);
                }
            }
        }
    }

    /// Check a pattern declaration.
    fn check_pattern(&self, pattern: &PatternDecl, diagnostics: &mut Diagnostics) {
        // Check for empty parameters block
        if pattern.parameters.is_empty() && pattern.behavior.is_none() {
            if self.is_rule_enabled(LintRule::EmptyBlock) {
                diagnostics.add(Diagnostic::warning(
                    LintRule::EmptyBlock.error_code(),
                    format!("Pattern '{}' has no parameters or behavior", pattern.name),
                    pattern.span,
                ));
            }
        }

        // Check behavior if present
        if let Some(behavior) = &pattern.behavior {
            self.check_behavior(behavior, &HashSet::new(), diagnostics);
        }
    }

    /// Check a rationale declaration.
    fn check_rationale(&self, rationale: &RationaleDecl, _diagnostics: &mut Diagnostics) {
        let _ = rationale;
    }

    /// Check a distilled pattern declaration.
    fn check_distilled(&self, distilled: &DistilledPattern, _diagnostics: &mut Diagnostics) {
        let _ = distilled;
    }

    /// Check a predicate declaration.
    fn check_predicate(&self, predicate: &PredicateDecl, _diagnostics: &mut Diagnostics) {
        let _ = predicate;
    }

    /// Check an import declaration.
    fn check_import(&self, import: &ImportDecl, _diagnostics: &mut Diagnostics) {
        let _ = import;
    }

    /// Check an event declaration.
    fn check_event(&self, event: &EventDecl, _diagnostics: &mut Diagnostics) {
        let _ = event;
    }

    /// Check for unused components.
    fn check_unused_components(
        &self,
        system: &SystemDecl,
        _declared: &HashSet<&str>,
        diagnostics: &mut Diagnostics,
    ) {
        // Collect all referenced components
        let mut referenced: HashSet<&str> = HashSet::new();

        // From constraints
        for constraint in &system.constraints {
            for rule in &constraint.rules {
                self.collect_referenced_entities(rule, &mut referenced);
            }
        }

        // From component depends_only
        for component in &system.components {
            for dep in &component.depends_only {
                referenced.insert(dep.as_str());
            }
        }

        // From component contains
        for component in &system.components {
            for contained in &component.contains {
                referenced.insert(contained.as_str());
            }
        }

        // Check for unused
        for component in &system.components {
            if !referenced.contains(component.name.as_str()) {
                // Component is unused, but check if it has behaviors (it's self-contained)
                if component.behaviors.is_empty() && component.components.is_empty() {
                    if self.is_rule_enabled(LintRule::UnusedComponent) {
                        diagnostics.add(Diagnostic::warning(
                            LintRule::UnusedComponent.error_code(),
                            format!("Component '{}' is never referenced", component.name),
                            component.span,
                        ).with_suggestion("Consider removing unused component or adding references"));
                    }
                }
            }
        }
    }

    /// Collect referenced entities from a constraint rule.
    fn collect_referenced_entities<'a>(&self, rule: &'a ConstraintRule, referenced: &mut HashSet<&'a str>) {
        match rule {
            ConstraintRule::Not(inner) => {
                self.collect_referenced_entities(inner, referenced);
            }
            ConstraintRule::And(a, b)
            | ConstraintRule::Or(a, b)
            | ConstraintRule::Implies(a, b)
            | ConstraintRule::Iff(a, b) => {
                self.collect_referenced_entities(a, referenced);
                self.collect_referenced_entities(b, referenced);
            }
            ConstraintRule::Forall { domain, body, .. }
            | ConstraintRule::Exists { domain, body, .. } => {
                self.collect_scope_entities(domain, referenced);
                self.collect_referenced_entities(body, referenced);
            }
            ConstraintRule::Predicate(pred) => {
                self.collect_predicate_entities(pred, referenced);
            }
            ConstraintRule::Call { subject, args, .. } => {
                self.collect_scope_entities(subject, referenced);
                for arg in args {
                    self.collect_scope_entities(arg, referenced);
                }
            }
            ConstraintRule::Comparison { .. } | ConstraintRule::NFConstraint { .. } => {}
            ConstraintRule::Suppressed { rule, .. } => {
                self.collect_referenced_entities(rule, referenced);
            }
        }
    }

    /// Collect entities from a scope expression.
    fn collect_scope_entities<'a>(&self, expr: &'a ScopeExpr, referenced: &mut HashSet<&'a str>) {
        match expr {
            ScopeExpr::Ident(qname) => {
                if qname.is_simple() {
                    referenced.insert(&qname.segments[0]);
                }
            }
            ScopeExpr::EntityList(names) => {
                for name in names {
                    referenced.insert(name.as_str());
                }
            }
            ScopeExpr::Union(a, b)
            | ScopeExpr::Intersection(a, b)
            | ScopeExpr::Difference(a, b) => {
                self.collect_scope_entities(a, referenced);
                self.collect_scope_entities(b, referenced);
            }
            ScopeExpr::Glob(_) | ScopeExpr::All | ScopeExpr::Matches { .. } => {}
            ScopeExpr::Filtered { .. } => {}
        }
    }

    /// Collect entities from a predicate call.
    fn collect_predicate_entities<'a>(&self, pred: &'a PredicateCall, referenced: &mut HashSet<&'a str>) {
        match pred {
            PredicateCall::Depends { from, to }
            | PredicateCall::References { from, to } => {
                self.collect_scope_entities(from, referenced);
                for target in to {
                    self.collect_scope_entities(target, referenced);
                }
            }
            PredicateCall::Implements { entity, .. } => {
                self.collect_scope_entities(entity, referenced);
            }
            PredicateCall::Contains { container, entities } => {
                self.collect_scope_entities(container, referenced);
                for entity in entities {
                    self.collect_scope_entities(entity, referenced);
                }
            }
        }
    }

    /// Check naming conventions.
    fn check_naming(&self, top: &TopLevel, diagnostics: &mut Diagnostics) {
        if !self.is_rule_enabled(LintRule::NamingConvention) {
            return;
        }

        match top {
            TopLevel::System(system) => {
                // System names should be PascalCase
                if !is_pascal_case(&system.name) {
                    diagnostics.add(Diagnostic::hint(
                        LintRule::NamingConvention.error_code(),
                        format!("System name '{}' should be PascalCase", system.name),
                        system.span,
                    ).with_suggestion("Use PascalCase for system names (e.g., MySystem)"));
                }

                // Check component names
                for component in &system.components {
                    if !is_pascal_case(&component.name) {
                        diagnostics.add(Diagnostic::hint(
                            LintRule::NamingConvention.error_code(),
                            format!("Component name '{}' should be PascalCase", component.name),
                            component.span,
                        ));
                    }
                }

                // Check behavior names
                for behavior in &system.behaviors {
                    if !is_pascal_case(&behavior.name) {
                        diagnostics.add(Diagnostic::hint(
                            LintRule::NamingConvention.error_code(),
                            format!("Behavior name '{}' should be PascalCase", behavior.name),
                            behavior.span,
                        ));
                    }

                    // State names should be snake_case
                    for state in &behavior.states {
                        if !is_snake_case(&state.name) {
                            diagnostics.add(Diagnostic::hint(
                                LintRule::NamingConvention.error_code(),
                                format!("State name '{}' should be snake_case", state.name),
                                Span::synthetic(),
                            ).with_suggestion("Use snake_case for state names (e.g., in_progress)"));
                        }
                    }
                }

                // Check constraint names
                for constraint in &system.constraints {
                    if !is_snake_case(&constraint.name) {
                        diagnostics.add(Diagnostic::hint(
                            LintRule::NamingConvention.error_code(),
                            format!("Constraint name '{}' should be snake_case", constraint.name),
                            constraint.span,
                        ));
                    }
                }
            }
            TopLevel::Pattern(pattern) => {
                if !is_pascal_case(&pattern.name) {
                    diagnostics.add(Diagnostic::hint(
                        LintRule::NamingConvention.error_code(),
                        format!("Pattern name '{}' should be PascalCase", pattern.name),
                        pattern.span,
                    ));
                }
            }
            _ => {}
        }
    }

    /// Check pedantic rules.
    fn check_pedantic(&self, top: &TopLevel, diagnostics: &mut Diagnostics) {
        match top {
            TopLevel::System(system) => {
                // Check for overly complex state machines
                for behavior in &system.behaviors {
                    if behavior.states.len() > self.config.max_states {
                        diagnostics.add(Diagnostic::warning(
                            ErrorCode::E025_TemporalPropertyError,
                            format!(
                                "Behavior '{}' has {} states, which exceeds the limit of {}",
                                behavior.name, behavior.states.len(), self.config.max_states
                            ),
                            behavior.span,
                        ).with_suggestion("Consider splitting into multiple behaviors"));
                    }

                    if behavior.transitions.len() > self.config.max_transitions {
                        diagnostics.add(Diagnostic::warning(
                            ErrorCode::E025_TemporalPropertyError,
                            format!(
                                "Behavior '{}' has {} transitions, which exceeds the limit of {}",
                                behavior.name, behavior.transitions.len(), self.config.max_transitions
                            ),
                            behavior.span,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Check if a string is PascalCase.
fn is_pascal_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars().next().unwrap().is_uppercase() && !s.contains('_')
}

/// Check if a string is snake_case.
fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| c.is_lowercase() || c == '_' || c.is_numeric())
}

/// Convenience function to lint a single file.
pub fn lint(source: &str) -> LintResult {
    let linter = Linter::default_linter();
    linter.lint_file(Path::new("<stdin>"), source)
}

/// Convenience function to lint a file with custom configuration.
pub fn lint_with_config(config: LinterConfig, path: &Path, source: &str) -> LintResult {
    let linter = Linter::new(config);
    linter.lint_file(path, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_valid_system() {
        let source = r#"
            system TestSystem {
                description "A test system"

                component API {
                    implements "src/api"
                }

                constraint isolation {
                    API.references([AppError])
                }
            }
        "#;
        let result = lint(source);
        assert!(result.passed, "Expected no errors: {:?}", result.diagnostics.items);
    }

    #[test]
    fn test_lint_syntax_error() {
        let source = "system { }";
        let result = lint(source);
        assert!(!result.passed);
        assert!(result.has_errors());
    }

    #[test]
    fn test_lint_missing_initial_state() {
        let source = r#"
            system Test {
                behavior Flow {
                    states { pending processing }
                    transitions { pending -> processing on start }
                }
            }
        "#;
        let result = lint(source);
        assert!(result.has_errors());
    }

    #[test]
    fn test_lint_multiple_initial_states() {
        let source = r#"
            system Test {
                behavior Flow {
                    states {
                        pending { initial: true }
                        processing { initial: true }
                    }
                }
            }
        "#;
        let result = lint(source);
        assert!(result.has_errors());
    }

    #[test]
    fn test_lint_invalid_transition() {
        let source = r#"
            system Test {
                behavior Flow {
                    states {
                        pending { initial: true }
                        completed { terminal: true }
                    }
                    transitions {
                        pending -> unknown on start
                    }
                }
            }
        "#;
        let result = lint(source);
        assert!(result.has_errors());
    }

    #[test]
    fn test_lint_unreachable_state() {
        let source = r#"
            system Test {
                behavior Flow {
                    states {
                        pending { initial: true }
                        completed { terminal: true }
                        unreachable_state
                    }
                    transitions {
                        pending -> completed on finish
                    }
                }
            }
        "#;
        let result = lint(source);
        assert!(result.has_warnings());
    }

    #[test]
    fn test_lint_naming_convention() {
        let source = r#"
            system test_system {
                behavior flow {
                    states { PendingState InProgress }
                    transitions { PendingState -> InProgress on start }
                }
            }
        "#;
        let linter = Linter::new(LinterConfig {
            check_naming: true,
            ..LinterConfig::default()
        });
        let result = linter.lint_file(Path::new("test.intent"), source);
        // Should have hints about naming conventions
        let hints: Vec<_> = result.diagnostics.items.iter()
            .filter(|d| d.severity == Severity::Hint)
            .collect();
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_lint_duplicate_component() {
        let source = r#"
            system Test {
                component API { }
                component API { }
            }
        "#;
        let result = lint(source);
        assert!(result.has_errors());
    }

    #[test]
    fn test_lint_valid_state_machine() {
        let source = r#"
            system Test {
                behavior OrderLifecycle {
                    states {
                        pending { initial: true }
                        processing
                        completed { terminal: true }
                        cancelled { terminal: true }
                    }
                    transitions {
                        pending -> processing on start
                        processing -> completed on finish
                        processing -> cancelled on cancel
                        pending -> cancelled on cancel
                    }
                }
            }
        "#;
        let result = lint(source);
        assert!(result.passed, "Expected no errors: {:?}", result.diagnostics.items);
    }

    #[test]
    fn test_compute_reachable_states() {
        let source = r#"
            system Test {
                behavior Flow {
                    states {
                        a { initial: true }
                        b
                        c
                        d
                    }
                    transitions {
                        a -> b on t1
                        b -> c on t2
                        c -> d on t3
                    }
                }
            }
        "#;
        let result = lint(source);
        // All states should be reachable
        let unreachable: Vec<_> = result.diagnostics.items.iter()
            .filter(|d| d.code == ErrorCode::E006_UnreachableState)
            .collect();
        assert!(unreachable.is_empty(), "Unexpected unreachable states: {:?}", unreachable);
    }
}
