//! Abstract Syntax Tree for the Intent language.
//!
//! This module defines the AST types for parsed Intent source.

// Re-export Span from diagnostics for use in AST nodes
pub use crate::diagnostic::Span;

use std::fmt;

/// A qualified name with optional path segments (e.g., `std.patterns.Retry`).
#[derive(Debug, Clone)]
pub struct QualifiedName {
    /// Path segments (e.g., ["std", "patterns", "Retry"])
    pub segments: Vec<String>,
    /// Source code span
    pub span: Span,
}

impl PartialEq for QualifiedName {
    fn eq(&self, other: &Self) -> bool {
        self.segments == other.segments
    }
}

impl Eq for QualifiedName {}

impl std::hash::Hash for QualifiedName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.segments.hash(state);
    }
}

impl QualifiedName {
    /// Create a new qualified name from segments.
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments, span: Span::synthetic() }
    }

    /// Create a qualified name from a single identifier.
    pub fn simple(name: impl Into<String>) -> Self {
        Self {
            segments: vec![name.into()],
            span: Span::synthetic(),
        }
    }

    /// Set the span on this qualified name (builder pattern).
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = span;
        self
    }

    /// Create a qualified name from dotted string with a span.
    pub fn from_dotted(dotted: &str, span: Span) -> Self {
        Self {
            segments: dotted.split('.').map(|s| s.to_string()).collect(),
            span,
        }
    }

    /// Get the simple name (last segment).
    pub fn name(&self) -> &str {
        self.segments.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// Get the namespace (all segments except last).
    pub fn namespace(&self) -> &[String] {
        if self.segments.is_empty() {
            &[]
        } else {
            &self.segments[..self.segments.len() - 1]
        }
    }

    /// Check if this is a simple name (single segment).
    pub fn is_simple(&self) -> bool {
        self.segments.len() == 1
    }

    /// Convert to dotted string representation.
    pub fn to_dotted(&self) -> String {
        self.segments.join(".")
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_dotted())
    }
}

impl From<String> for QualifiedName {
    fn from(s: String) -> Self {
        Self::new(s.split('.').map(|part| part.to_string()).collect())
    }
}

impl From<&str> for QualifiedName {
    fn from(s: &str) -> Self {
        Self::new(s.split('.').map(|part| part.to_string()).collect())
    }
}

/// Top-level declaration in an Intent file.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Import(ImportDecl),
    System(SystemDecl),
    Pattern(PatternDecl),
    Rationale(RationaleDecl),
    Distilled(DistilledPattern),
    Predicate(PredicateDecl),
    Event(EventDecl),
    Message(MessageDecl),
}

/// Event declaration with optional payload type.
#[derive(Debug, Clone, PartialEq)]
pub struct EventDecl {
    pub name: String,
    pub payload: Option<crate::types::SpannedType>,
    pub span: Span,
}

/// Message declaration for typed inter-behavior communication.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageDecl {
    pub channel: String,
    pub name: String,
    pub payload: Option<crate::types::SpannedType>,
    pub span: Span,
}

/// Selective import specification.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectiveImport {
    /// Import everything: import X from "source"
    All,
    /// Import only specific items: import X { A, B } from "source"
    Only(Vec<String>),
    /// Import everything except: import X except { A, B } from "source"
    Except(Vec<String>),
}

/// Import declaration for patterns and templates.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub kind: ImportKind,
    pub name: String,
    /// Optional alias: import X as Y
    pub alias: Option<String>,
    /// Selective import specification
    pub selective: SelectiveImport,
    pub source: String,
    pub with_params: Vec<(String, ParamValue)>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Pattern,
    Template,
}

/// Visibility control for declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Private,
    Public,
    Internal,
}

/// A system declaration - the primary container.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemDecl {
    pub name: String,
    /// Visibility control (pub, internal, private)
    pub visibility: Visibility,
    pub description: Option<String>,
    /// `refines AbstractSystem`
    pub refines: Option<String>,
    /// Component list declaration: `components [A, B, C]`
    pub components_decl: Vec<String>,
    /// Component definitions
    pub components: Vec<ComponentDecl>,
    /// Constraints
    pub constraints: Vec<ConstraintDecl>,
    /// Behaviors
    pub behaviors: Vec<BehaviorDecl>,
    /// Patterns defined locally
    pub patterns: Vec<PatternDecl>,
    /// Pattern applications
    pub applies: Vec<PatternApplication>,
    /// Predicates
    pub predicates: Vec<PredicateDecl>,
    /// Invariants
    pub invariants: Vec<InvariantDecl>,
    /// Let bindings
    pub let_bindings: Vec<(String, ScopeExpr)>,
    /// Rationale
    pub rationales: Vec<RationaleDecl>,
    /// System properties (platform, ci, status, etc.)
    pub properties: Vec<(String, PropertyValue)>,
    /// Distillation markers
    pub distilled: Vec<DistilledPattern>,
    /// Uses template
    pub uses: Vec<String>,
    /// Event declarations
    pub events: Vec<EventDecl>,
    /// Message declarations
    pub messages: Vec<MessageDecl>,
    /// Function declarations
    pub functions: Vec<FunctionDecl>,
    /// Protocol declarations
    pub protocols: Vec<ProtocolDecl>,
    /// Constraint templates
    pub constraint_templates: Vec<ConstraintTemplate>,
    /// Constraint applications
    pub constraint_applications: Vec<ConstraintApplication>,
    /// Span in source text
    pub span: Span,
}

impl Default for SystemDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            visibility: Visibility::default(),
            description: None,
            refines: None,
            components_decl: Vec::new(),
            components: Vec::new(),
            constraints: Vec::new(),
            behaviors: Vec::new(),
            patterns: Vec::new(),
            applies: Vec::new(),
            predicates: Vec::new(),
            invariants: Vec::new(),
            let_bindings: Vec::new(),
            rationales: Vec::new(),
            properties: Vec::new(),
            distilled: Vec::new(),
            uses: Vec::new(),
            events: Vec::new(),
            messages: Vec::new(),
            functions: Vec::new(),
            protocols: Vec::new(),
            constraint_templates: Vec::new(),
            constraint_applications: Vec::new(),
            span: Span::synthetic(),
        }
    }
}

/// A component declaration.
///
/// Components are structural by default. A component with behaviors
/// is behavioral and will transpile to TLA+.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDecl {
    pub name: String,
    /// Path to implementation
    pub implements: Option<String>,
    /// Entities contained in this component
    pub contains: Vec<String>,
    /// Dependency restriction
    pub depends_only: Vec<String>,
    /// Nested components
    pub components: Vec<ComponentDecl>,
    /// Behaviors (makes component behavioral -> transpiles to TLA+)
    pub behaviors: Vec<BehaviorDecl>,
    /// Behavior bindings (explicit pattern applications)
    pub binds: Vec<BehaviorBinding>,
    pub span: Span,
}

impl Default for ComponentDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            implements: None,
            contains: Vec::new(),
            depends_only: Vec::new(),
            components: Vec::new(),
            behaviors: Vec::new(),
            binds: Vec::new(),
            span: Span::synthetic(),
        }
    }
}

/// A binding of a behavior/pattern to a component.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorBinding {
    /// The behavior/pattern being bound (can be qualified: std.patterns.Retry)
    pub behavior: QualifiedName,
    /// Optional alias for the binding
    pub alias: Option<String>,
    /// Parameters for the binding
    pub params: Vec<(String, ParamValue)>,
    pub span: Span,
}

/// A constraint declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintDecl {
    pub name: String,
    pub rules: Vec<ConstraintRule>,
    pub span: Span,
}

/// A constraint template with parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintTemplate {
    pub name: String,
    pub params: Vec<(String, String)>,  // (name, type)
    pub rules: Vec<ConstraintRule>,
    pub span: Span,
}

/// A constraint application from a template.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintApplication {
    pub template_name: String,
    pub args: Vec<String>,
    pub span: Span,
}

/// Suppression for a constraint rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Suppression {
    pub exception: Vec<String>,
    pub reason: Option<String>,
    pub expires: Option<String>,
    pub tracking: Option<String>,
}

/// Constraint rules using predicates and operators.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintRule {
    /// `!rule` - negation
    Not(Box<ConstraintRule>),
    /// `a && b` - conjunction
    And(Box<ConstraintRule>, Box<ConstraintRule>),
    /// `a || b` - disjunction
    Or(Box<ConstraintRule>, Box<ConstraintRule>),
    /// `a => b` - implication
    Implies(Box<ConstraintRule>, Box<ConstraintRule>),
    /// `a <=> b` - biconditional (if and only if)
    Iff(Box<ConstraintRule>, Box<ConstraintRule>),
    /// `forall x in S: rule` or `forall x in S where filter: rule`
    Forall {
        var: String,
        domain: ScopeExpr,
        filter: Option<Expr>,
        body: Box<ConstraintRule>,
    },
    /// `exists x in S: rule` or `exists x in S where filter: rule`
    Exists {
        var: String,
        domain: ScopeExpr,
        filter: Option<Expr>,
        body: Box<ConstraintRule>,
    },
    /// Predicate call: `A.depends(B)`, `A.references(B)`, etc.
    Predicate(PredicateCall),
    /// Comparison: `p99(op) < 100ms`
    Comparison { lhs: Expr, op: ComparisonOp, rhs: Expr },
    /// User-defined predicate call: `A.myPredicate(B, C)`
    Call { subject: ScopeExpr, name: String, args: Vec<ScopeExpr> },
    /// Non-functional constraint: `p99(op) < 100ms`
    NFConstraint { metric: NFMetric, op: ComparisonOp, value: Expr },
    /// Suppressed rule: `rule allow { exception: [...], reason: "..." }`
    Suppressed { rule: Box<ConstraintRule>, suppression: Suppression },
}

/// Non-functional metrics for performance constraints.
#[derive(Debug, Clone, PartialEq)]
pub enum NFMetric {
    P50(String),   // p50(operation)
    P95(String),   // p95(operation)
    P99(String),   // p99(operation)
    Throughput(String),  // throughput(scope)
    Memory,        // memory
    Cpu,           // cpu
}

/// Built-in predicates with method-style syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum PredicateCall {
    /// `A.depends(B)` or `A.depends(B, C, ...)`
    Depends { from: ScopeExpr, to: Vec<ScopeExpr> },
    /// `A.references(B)` or `A.references(B, C, ...)`
    References { from: ScopeExpr, to: Vec<ScopeExpr> },
    /// `A.implements(T)`
    Implements { entity: ScopeExpr, trait_name: String },
    /// `A.contains(B)` or `A.contains(B, C, ...)`
    Contains { container: ScopeExpr, entities: Vec<ScopeExpr> },
    /// `A.depends_transitively(B)` or `A.depends_transitively(B, C, ...)`
    DependsTransitively { from: ScopeExpr, to: Vec<ScopeExpr> },
}

/// Set expressions for scope composition.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeExpr {
    /// List of entity names: `[A, B, C]`
    EntityList(Vec<String>),
    /// Qualified identifier: `A` or `std.patterns.Retry`
    Ident(QualifiedName),
    /// Glob pattern: `*Client` or `Dgraph*`
    Glob(String),
    /// Set union: `A | B`
    Union(Box<ScopeExpr>, Box<ScopeExpr>),
    /// Set intersection: `A & B`
    Intersection(Box<ScopeExpr>, Box<ScopeExpr>),
    /// Set difference: `A \ B`
    Difference(Box<ScopeExpr>, Box<ScopeExpr>),
    /// Pattern match filter: `{ x | x matches Pattern }`
    Matches { var: String, pattern: String },
    /// Filtered set: { x | x.field == value }
    Filtered { var: String, condition: Expr },
    /// All entities: `all`
    All,
}

/// General expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Duration(u64),
    String(String),
    Bool(bool),
    Ident(String),
    DottedName(String),
    Call { name: String, args: Vec<Expr> },
    BinOp { lhs: Box<Expr>, op: ArithOp, rhs: Box<Expr> },
    CompOp { lhs: Box<Expr>, op: ComparisonOp, rhs: Box<Expr> },
    LogicalOp { lhs: Box<Expr>, op: LogicalOp, rhs: Box<Expr> },
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    /// count(state) - cardinality of nodes in this state
    Count(String),

    // TLA+ primitives

    /// CHOOSE x \in S : P(x) - Select arbitrary element satisfying predicate
    Choose { var: String, domain: Box<Expr>, predicate: Box<Expr> },
    /// LET x == e IN body - Local definitions
    Let { bindings: Vec<(String, Expr)>, body: Box<Expr> },
    /// IF cond THEN e1 ELSE e2 - Conditional expression
    IfThenElse { cond: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr> },
    /// CASE x OF ... - Multi-way conditional
    Case { arms: Vec<(Expr, Expr)>, default: Option<Box<Expr>> },
    /// SUBSET S - Power set
    Subset(Box<Expr>),
    /// UNION S - Big union (union of all elements of S)
    BigUnion(Box<Expr>),
    /// DOMAIN f - Domain of function/record
    Domain(Box<Expr>),
    /// [f EXCEPT ![x] = y] - Function update
    Except { base: Box<Expr>, updates: Vec<(Vec<Expr>, Expr)> },
    /// [x \in S |-> e] - Function literal
    FunctionLiteral { var: String, domain: Box<Expr>, body: Box<Expr> },
    /// [field: value, ...] - Record literal
    Record(Vec<(String, Expr)>),
    /// r.field - Record field access
    FieldAccess { record: Box<Expr>, field: String },
    /// <<e1, e2, ...>> - Tuple literal
    Tuple(Vec<Expr>),
    /// {e1, e2, ...} - Set literal
    SetLiteral(Vec<Expr>),
    /// e[i] - Sequence/function application
    Index { base: Box<Expr>, index: Box<Expr> },
    /// S \ T - Set difference
    SetDiff { lhs: Box<Expr>, rhs: Box<Expr> },
    /// S \union T - Set union
    SetUnion { lhs: Box<Expr>, rhs: Box<Expr> },
    /// S \intersect T - Set intersection
    SetIntersect { lhs: Box<Expr>, rhs: Box<Expr> },
    /// x \in S - Set membership
    In { element: Box<Expr>, set: Box<Expr> },
    /// \A x \in S : P(x) - Universal quantification
    Forall { var: String, domain: Box<Expr>, body: Box<Expr> },
    /// \E x \in S : P(x) - Existential quantification
    Exists { var: String, domain: Box<Expr>, body: Box<Expr> },

    /// ASSUME P - Declare assumption for model checking
    Assume(Box<Expr>),
    /// Inline TLA+ code: tla!("[]<>(state = done)")
    TlaInline { code: String },
}

/// Expressions used by executable metadata blocks such as fixtures, projections,
/// and concrete implementation bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaExpr {
    Int(i64),
    Duration(u64),
    String(String),
    Bool(bool),
    Null,
    Ident(String),
    DottedName(String),
    Ref(String),
    Call { name: String, args: Vec<MetaExpr> },
    List(Vec<MetaExpr>),
    Binary { lhs: Box<MetaExpr>, op: MetaOp, rhs: Box<MetaExpr> },
    Exists { source: String, filter: Option<Box<MetaExpr>> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    Lt, Gt, Le, Ge, Eq, Ne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add, Sub, Mul, Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not, Neg,
}

/// A predicate definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PredicateDecl {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<ConstraintRule>,
}

/// An invariant declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct InvariantDecl {
    pub name: String,
    pub expr: Expr,
}

/// Bounded values for variable constraints.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueBounds {
    /// Minimum value (for numeric types)
    pub min: Option<Expr>,
    /// Maximum value (for numeric types)
    pub max: Option<Expr>,
    /// Allowed values (for enumerated types)
    pub values: Option<Vec<Expr>>,
}

/// Variable declaration for behaviors.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDecl {
    pub name: String,
    pub type_name: String,
    pub initial_value: Option<Expr>,
    /// Value bounds for constrained variables
    pub bounds: Option<ValueBounds>,
}

/// A function declaration within a behavior or system.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<(String, String)>,  // (name, type)
    pub return_type: Option<String>,
    pub body: Expr,
    pub span: Span,
}

/// A behavior declaration (state machine).
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDecl {
    pub name: String,
    /// `composes [A.Flow, B.Flow]`
    pub composes: Vec<String>,
    /// `nodes: replicas` - optional node set for distributed systems
    pub nodes: Option<String>,
    /// Parameters (like pattern parameters, but for behaviors)
    pub parameters: Vec<PatternParam>,
    /// Explicit variable declarations
    pub variables: Vec<VariableDecl>,
    /// Function declarations
    pub functions: Vec<FunctionDecl>,
    /// States
    pub states: Vec<StateDecl>,
    /// Executable fixture metadata
    pub fixtures: Vec<FixtureDecl>,
    /// Executable projection metadata
    pub projections: Vec<ProjectionDecl>,
    /// Transitions
    pub transitions: Vec<TransitionDecl>,
    /// Temporal properties
    pub properties: Vec<TemporalProperty>,
    /// Fairness
    pub fairness: Vec<FairnessSpec>,
    /// Invariants
    pub invariants: Vec<InvariantDecl>,
    /// Refines external TLA+ spec
    pub refines: Option<String>,
    /// Applied patterns
    pub applies: Vec<PatternApplication>,
    /// Refinement mappings
    pub refinement_map: Option<RefinementMap>,
    /// Strengthening clauses
    pub strengthens: Vec<Strengthens>,
    pub span: Span,
}

impl Default for BehaviorDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            composes: Vec::new(),
            nodes: None,
            parameters: Vec::new(),
            variables: Vec::new(),
            functions: Vec::new(),
            states: Vec::new(),
            fixtures: Vec::new(),
            projections: Vec::new(),
            transitions: Vec::new(),
            properties: Vec::new(),
            fairness: Vec::new(),
            invariants: Vec::new(),
            refines: None,
            applies: Vec::new(),
            refinement_map: None,
            strengthens: Vec::new(),
            span: Span::synthetic(),
        }
    }
}

/// A state declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    pub name: String,
    pub initial: bool,
    pub terminal: bool,
    /// Parent state for hierarchical states
    pub parent: Option<String>,
    /// Nested substates
    pub substates: Vec<StateDecl>,
    /// Actions to execute on state entry
    pub entry_actions: Vec<EffectStmt>,
    /// Actions to execute on state exit
    pub exit_actions: Vec<EffectStmt>,
}

impl Default for StateDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            initial: false,
            terminal: false,
            parent: None,
            substates: Vec::new(),
            entry_actions: Vec::new(),
            exit_actions: Vec::new(),
        }
    }
}

/// Source of a transition.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionSource {
    /// Single source state: `pending`
    State(String),
    /// Wildcard: `*` (matches any state)
    Wildcard,
    /// Multiple source states: `[state1, state2]`
    States(Vec<String>),
}

impl TransitionSource {
    /// Get the single state name if this is a State variant.
    pub fn as_state(&self) -> Option<&str> {
        match self {
            TransitionSource::State(s) => Some(s),
            _ => None,
        }
    }

    /// Check if this is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, TransitionSource::Wildcard)
    }

    /// Get all referenced states.
    pub fn states(&self) -> Vec<&str> {
        match self {
            TransitionSource::State(s) => vec![s.as_str()],
            TransitionSource::Wildcard => vec![],
            TransitionSource::States(states) => states.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Convert to string representation for display.
    pub fn to_string_repr(&self) -> String {
        match self {
            TransitionSource::State(s) => s.clone(),
            TransitionSource::Wildcard => "*".to_string(),
            TransitionSource::States(states) => format!("[{}]", states.join(", ")),
        }
    }
}

impl fmt::Display for TransitionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_repr())
    }
}

/// Target of a transition.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionTarget {
    /// Single target state: `completed`
    State(String),
    /// Self transition: `self` (stay in current state)
    Self_,
    /// Multiple target states: `[state1, state2]` (non-deterministic choice)
    States(Vec<String>),
    /// Fork into parallel branches: `fork { branch1, branch2 }`
    Fork { branches: Vec<ParallelBranch> },
    /// Join parallel branches: `join { state1, state2 } -> target`
    Join { sync_states: Vec<String>, target: String },
}

/// A parallel branch in a fork transition.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBranch {
    /// Target state for this branch
    pub target: String,
    /// Optional guard condition for this branch
    pub condition: Option<Expr>,
}

impl TransitionTarget {
    /// Get the single state name if this is a State variant.
    pub fn as_state(&self) -> Option<&str> {
        match self {
            TransitionTarget::State(s) => Some(s),
            _ => None,
        }
    }

    /// Check if this is a self transition.
    pub fn is_self(&self) -> bool {
        matches!(self, TransitionTarget::Self_)
    }

    /// Get all referenced states.
    pub fn states(&self) -> Vec<&str> {
        match self {
            TransitionTarget::State(s) => vec![s.as_str()],
            TransitionTarget::Self_ => vec![],
            TransitionTarget::States(states) => states.iter().map(|s| s.as_str()).collect(),
            TransitionTarget::Fork { branches } => {
                branches.iter().map(|b| b.target.as_str()).collect()
            }
            TransitionTarget::Join { sync_states, target } => {
                let mut states: Vec<&str> = sync_states.iter().map(|s| s.as_str()).collect();
                states.push(target.as_str());
                states
            }
        }
    }

    /// Convert to string representation for display.
    pub fn to_string_repr(&self) -> String {
        match self {
            TransitionTarget::State(s) => s.clone(),
            TransitionTarget::Self_ => "self".to_string(),
            TransitionTarget::States(states) => format!("[{}]", states.join(", ")),
            TransitionTarget::Fork { branches } => {
                let branch_strs: Vec<String> = branches
                    .iter()
                    .map(|b| {
                        if let Some(ref _cond) = b.condition {
                            format!("{} if {}", b.target, "<cond>")
                        } else {
                            b.target.clone()
                        }
                    })
                    .collect();
                format!("fork {{ {} }}", branch_strs.join(", "))
            }
            TransitionTarget::Join { sync_states, target } => {
                format!("join {{ {} }} -> {}", sync_states.join(", "), target)
            }
        }
    }
}

impl fmt::Display for TransitionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_repr())
    }
}

/// A transition declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionDecl {
    /// Source state(s)
    pub from: TransitionSource,
    /// Target state(s)
    pub to: TransitionTarget,
    pub on_event: String,
    pub inputs: Vec<TransitionInput>,
    pub bindings: Vec<TransitionBinding>,
    pub guard: Option<Expr>,
    pub expects: Vec<Expr>,
    pub effects: Vec<EffectStmt>,
    pub timing: Option<TransitionTiming>,
    pub span: Span,
}

/// A transition-local input for executable-style transitions.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionInput {
    pub name: String,
    pub type_name: String,
    pub domain: Option<Expr>,
    pub default_value: Option<Expr>,
    pub span: Span,
}

/// A named executable fixture block.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureDecl {
    pub name: String,
    pub steps: Vec<FixtureStep>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FixtureStep {
    Insert {
        target: String,
        fields: Vec<(String, MetaExpr)>,
        bind: Option<String>,
    },
    Call {
        path: String,
        args: Vec<(String, MetaExpr)>,
        bind: Option<String>,
    },
    Bind {
        name: String,
        value: MetaExpr,
    },
}

/// A named projection that maps concrete metadata/state into model states.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionDecl {
    pub name: String,
    pub source: Option<ProjectionSource>,
    pub clauses: Vec<ProjectionClause>,
    pub else_state: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionSource {
    pub source: String,
    pub filter: Option<MetaExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionClause {
    pub condition: MetaExpr,
    pub state: String,
    pub span: Span,
}

/// Concrete implementation bindings for executable transitions.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionBinding {
    Call {
        path: String,
        args: Vec<(String, MetaExpr)>,
    },
    Update {
        target: String,
        assignments: Vec<(String, MetaExpr)>,
        filter: Option<MetaExpr>,
    },
}

/// An effect statement.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectStmt {
    pub kind: EffectKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectKind {
    Emit { name: String, args: Vec<Expr> },
    Send { channel: String, message: String, args: Vec<Expr> },
    Receive { channel: String, message: String, filter: Option<Expr> },
    If { cond: Expr, then_effects: Vec<EffectStmt>, else_effects: Option<Vec<EffectStmt>> },
    Expr(Expr),
    /// Variable assignment: `var = expr`
    Assign { var: String, value: Expr },
}

/// Timing constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionTiming {
    After(Expr),
}

/// A temporal property.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalProperty {
    pub name: String,
    pub expr: TemporalExpr,
}

/// Temporal expressions (LTL-expressive; some operators require TLC backend).
#[derive(Debug, Clone, PartialEq)]
pub enum TemporalExpr {
    /// G φ - globally/always
    Always(Box<TemporalExpr>),
    /// F φ - finally/eventually
    Eventually(Box<TemporalExpr>),
    /// X φ - next
    Next(Box<TemporalExpr>),
    /// !φ - negation
    Not(Box<TemporalExpr>),
    /// φ U ψ - until (strong): φ holds until ψ becomes true, ψ must eventually hold
    Until { lhs: Box<TemporalExpr>, rhs: Box<TemporalExpr> },
    /// φ R ψ - release: ψ holds until and including when φ becomes true (or forever)
    Release { lhs: Box<TemporalExpr>, rhs: Box<TemporalExpr> },
    /// φ W ψ - weak until: φ holds until ψ, but ψ need not eventually hold
    WeakUntil { lhs: Box<TemporalExpr>, rhs: Box<TemporalExpr> },
    /// φ M ψ - strong release (mighty): like release but φ must eventually hold
    StrongRelease { lhs: Box<TemporalExpr>, rhs: Box<TemporalExpr> },
    /// Legacy: always(P => eventually(Q))
    AlwaysImplies { premise: Box<TemporalExpr>, conclusion: Box<TemporalExpr> },
    /// Atomic proposition (state name)
    State(String),
    /// count(state) - cardinality of nodes in this state
    Count(String),
    /// Integer literal for comparisons
    Int(i64),
    /// Logical binary operators
    BinOp { lhs: Box<TemporalExpr>, op: TemporalOp, rhs: Box<TemporalExpr> },
}

impl TemporalExpr {
    /// Return a new expression with all State and Count identifiers prefixed.
    ///
    /// Used during pattern expansion to namespace state references,
    /// e.g., prefix "saga_" transforms State("pending") → State("saga_pending").
    pub fn prefix_state_refs(&self, prefix: &str) -> TemporalExpr {
        match self {
            TemporalExpr::State(name) => TemporalExpr::State(format!("{}{}", prefix, name)),
            TemporalExpr::Count(name) => TemporalExpr::Count(format!("{}{}", prefix, name)),
            TemporalExpr::Int(n) => TemporalExpr::Int(*n),
            TemporalExpr::Always(inner) => TemporalExpr::Always(Box::new(inner.prefix_state_refs(prefix))),
            TemporalExpr::Eventually(inner) => TemporalExpr::Eventually(Box::new(inner.prefix_state_refs(prefix))),
            TemporalExpr::Next(inner) => TemporalExpr::Next(Box::new(inner.prefix_state_refs(prefix))),
            TemporalExpr::Not(inner) => TemporalExpr::Not(Box::new(inner.prefix_state_refs(prefix))),
            TemporalExpr::Until { lhs, rhs } => TemporalExpr::Until {
                lhs: Box::new(lhs.prefix_state_refs(prefix)),
                rhs: Box::new(rhs.prefix_state_refs(prefix)),
            },
            TemporalExpr::Release { lhs, rhs } => TemporalExpr::Release {
                lhs: Box::new(lhs.prefix_state_refs(prefix)),
                rhs: Box::new(rhs.prefix_state_refs(prefix)),
            },
            TemporalExpr::WeakUntil { lhs, rhs } => TemporalExpr::WeakUntil {
                lhs: Box::new(lhs.prefix_state_refs(prefix)),
                rhs: Box::new(rhs.prefix_state_refs(prefix)),
            },
            TemporalExpr::StrongRelease { lhs, rhs } => TemporalExpr::StrongRelease {
                lhs: Box::new(lhs.prefix_state_refs(prefix)),
                rhs: Box::new(rhs.prefix_state_refs(prefix)),
            },
            TemporalExpr::AlwaysImplies { premise, conclusion } => TemporalExpr::AlwaysImplies {
                premise: Box::new(premise.prefix_state_refs(prefix)),
                conclusion: Box::new(conclusion.prefix_state_refs(prefix)),
            },
            TemporalExpr::BinOp { lhs, op, rhs } => TemporalExpr::BinOp {
                lhs: Box::new(lhs.prefix_state_refs(prefix)),
                op: *op,
                rhs: Box::new(rhs.prefix_state_refs(prefix)),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalOp {
    Or, And, Implies, Iff,
    Lt,   // <
    Le,   // <=
    Gt,   // >
    Ge,   // >=
    Eq,   // ==
    Ne,   // !=
}

/// Fairness specification.
#[derive(Debug, Clone, PartialEq)]
pub struct FairnessSpec {
    pub kind: FairnessKind,
    pub from: String,
    pub to: String,
    /// Alternative target states (may be empty)
    pub alts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FairnessKind {
    Weak,
    Strong,
}

/// Type parameter with optional bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    /// Parameter name
    pub name: String,
    /// Optional bounds (e.g., Entity, State, Event, Ord, Hash)
    pub bounds: Vec<TypeBound>,
    /// Source span
    pub span: Span,
}

/// Type bound for generic parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeBound {
    /// Must be orderable: Ord
    Ord,
    /// Must be hashable: Hash
    Hash,
    /// Must be an entity type
    Entity,
    /// Must be a state type
    State,
    /// Must be an event type
    Event,
    /// Must be a component type
    Component,
    /// Must be a behavior type
    Behavior,
    /// Custom bound with name
    Named(String),
}

/// Constraint on a pattern type parameter, specifying required fields/capabilities.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereConstraint {
    /// Type parameter being constrained
    pub type_param: String,
    /// Required fields with their type bounds
    pub required_fields: Vec<(String, TypeBound)>,
    /// Source span
    pub span: Span,
}

/// A pattern declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternDecl {
    pub name: String,
    /// Type parameters with optional bounds (e.g., <T: Ord, K: Hash + Eq>)
    pub type_params: Vec<TypeParam>,
    /// Where constraints on type parameters
    pub where_constraints: Vec<WhereConstraint>,
    /// Extended pattern (inheritance)
    pub extends: Option<String>,
    /// Required interfaces for pattern
    pub requires: Vec<RequiredInterface>,
    pub parameters: Vec<PatternParam>,
    pub behavior: Option<BehaviorDecl>,
    pub span: Span,
}

/// Required interface for a pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct RequiredInterface {
    pub name: String,
    pub methods: Vec<(String, crate::types::SpannedType)>,
}

/// A pattern parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternParam {
    pub name: String,
    pub type_name: String,
    pub constraints: Vec<FieldConstraint>,
    /// Source code span
    pub span: Span,
}

/// Field constraints.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldConstraint {
    Min(ParamValue),
    Max(ParamValue),
    Default(ParamValue),
}

/// Pattern reference for nested pattern application.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternRef {
    /// Simple pattern name: Retry
    Simple(String),
    /// Composed/nested pattern: CircuitBreaker<Retry<HttpOp>>
    Composed { outer: String, inner: Box<PatternRef> },
}

impl PatternRef {
    /// Get the outermost pattern name.
    pub fn name(&self) -> &str {
        match self {
            PatternRef::Simple(name) => name,
            PatternRef::Composed { outer, .. } => outer,
        }
    }

    /// Get the pattern name as a simple string (outermost name).
    pub fn to_string_simple(&self) -> String {
        self.name().to_string()
    }
}

impl fmt::Display for PatternRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternRef::Simple(name) => write!(f, "{}", name),
            PatternRef::Composed { outer, inner } => write!(f, "{}<{}>", outer, inner),
        }
    }
}

/// A pattern application.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternApplication {
    /// Pattern reference (supports nesting like CircuitBreaker<Retry<HttpOp>>)
    pub pattern: PatternRef,
    /// Type arguments for generic patterns
    pub type_args: Vec<String>,
    /// Parameter values
    pub params: Vec<(String, ParamValue)>,
    /// Source code span
    pub span: Span,
}

/// Parameter values.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Ident(String),
    Int(i64),
    Float(f64),
    Duration(u64),
    String(String),
    Bool(bool),
    List(Vec<ParamValue>),
    Map(Vec<(String, ParamValue)>),
}

/// Property values for system properties.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Ident(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<PropertyValue>),
    Map(Vec<(String, PropertyValue)>),
}

/// Glob pattern for applies_to.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobPattern {
    pub path: String,
}

/// Distilled pattern declaration (§12).
#[derive(Debug, Clone, PartialEq)]
pub struct DistilledPattern {
    pub name: String,
    pub source: String,
    pub commit: String,
    pub extracted: Option<String>,
    /// Confidence score 0.0-1.0 indicating extraction certainty
    pub confidence: Option<f64>,
    pub observation: Option<Vec<String>>,
    pub parameters: Vec<PatternParam>,
    pub behavior: Option<BehaviorDecl>,
    pub applies_to: Option<GlobPattern>,
    pub span: Span,
}

/// Legacy alias for backward compatibility with grammar.
pub type DistilledFrom = DistilledPattern;

/// Refinement mapping (map { abstract.state -> [concrete1, concrete2] }).
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementMap {
    pub mappings: Vec<(String, Vec<String>)>,
}

/// Strengthening clause (strengthens Abstract.property with LocalProperty).
#[derive(Debug, Clone, PartialEq)]
pub struct Strengthens {
    pub target: String,
    pub with_property: String,
}

/// Recommendation item for rationale.
#[derive(Debug, Clone, PartialEq)]
pub enum RecommendationItem {
    Constraint(ConstraintDecl),
    Invariant(InvariantDecl),
}

/// A protocol declaration for cross-behavior ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecl {
    pub name: String,
    pub phases: Vec<PhaseRef>,
    pub properties: Vec<TemporalProperty>,
    pub span: Span,
}

/// A reference to a phase (behavior.state).
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseRef {
    pub behavior: String,
    pub state: String,
}

/// AST structure kind for code traceability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl { for_type: String },
    Module,
    Const,
    TypeAlias,
    Method { of_trait: Option<String> },
    Behavior,
    Constraint,
}

/// Target AST structure for tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstTarget {
    /// Kind of AST structure
    pub kind: AstKind,
    /// Name of the structure
    pub name: String,
}

/// Traces rationale to a stable AST structure.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeTrace {
    /// Source file path
    pub file: String,
    /// Target AST structure
    pub target: AstTarget,
    /// Optional qualified path within the structure
    pub path: Option<String>,
    /// Git commit hash for snapshot verification
    pub commit: Option<String>,
}

/// A rationale declaration (consolidated insight + rationale).
#[derive(Debug, Clone, PartialEq)]
pub struct RationaleDecl {
    pub name: String,
    pub discovered: Option<String>,
    pub source: Option<String>,
    pub observation: Option<Vec<String>>,
    pub recommendation: Vec<RecommendationItem>,
    pub decided_because: Vec<String>,
    pub rejected: Vec<(String, String)>,
    pub revisit_when: Vec<String>,
    /// Code traceability - links to implementation
    pub traces_to: Vec<CodeTrace>,
    pub span: Span,
}

// ═══════════════════════════════════════════════════════════════════════════
// INTERMEDIATE PARSING TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Intermediate type for parsing system items.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemItemParsed {
    Description(String),
    Refines(String),
    ComponentsDecl(Vec<String>),
    Component(ComponentDecl),
    Constraint(ConstraintDecl),
    Behavior(BehaviorDecl),
    Pattern(PatternDecl),
    Applies(PatternApplication),
    Predicate(PredicateDecl),
    Invariant(InvariantDecl),
    Let(String, ScopeExpr),
    Rationale(RationaleDecl),
    Property(String, PropertyValue),
    Distilled(DistilledPattern),
    Uses(String),
    Event(EventDecl),
    Message(MessageDecl),
    Function(FunctionDecl),
    Protocol(ProtocolDecl),
    ConstraintTemplate(ConstraintTemplate),
    ConstraintApplication(ConstraintApplication),
}

/// Intermediate type for parsing component items.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentItemParsed {
    Implements(String),
    Contains(Vec<String>),
    DependsOnly(Vec<String>),
    Component(ComponentDecl),
    Behavior(BehaviorDecl),
    Binds(BehaviorBinding),
}

/// Intermediate type for parsing behavior items.
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorItemParsed {
    Nodes(String),
    Parameters(Vec<PatternParam>),
    Variables(Vec<VariableDecl>),
    Function(FunctionDecl),
    States(Vec<StateDecl>),
    Fixture(FixtureDecl),
    Projection(ProjectionDecl),
    Transitions(Vec<TransitionDecl>),
    Property(TemporalProperty),
    Fairness(Vec<FairnessSpec>),
    Invariant(InvariantDecl),
    Refines(String),
    Applies(PatternApplication),
    Map(RefinementMap),
    Strengthens(Strengthens),
}

/// Intermediate type for parsing executable-style transition bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionExecutableItemParsed {
    Input(TransitionInput),
    Binding(TransitionBinding),
    Guard(Expr),
    Expect(Expr),
    Effect(EffectStmt),
    Effects(Vec<EffectStmt>),
}

/// Intermediate type for parsing pattern items.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternItemParsed {
    Parameters(Vec<PatternParam>),
    Extends(String),
    Requires(RequiredInterface),
    Behavior(BehaviorDecl),
}

/// Intermediate type for parsing rationale items.
#[derive(Debug, Clone, PartialEq)]
pub enum RationaleItemParsed {
    Discovered(String),
    Source(String),
    Observation(Vec<String>),
    Recommendation(ConstraintDecl),
    RecommendationInvariant(InvariantDecl),
    DecidedBecause(Vec<String>),
    Rejected(Vec<(String, String)>),
    RevisitWhen(Vec<String>),
    TracesTo(Vec<CodeTrace>),
}

/// Intermediate type for parsing distilled pattern items.
#[derive(Debug, Clone, PartialEq)]
pub enum DistilledPatternItemParsed {
    Source(String),
    Commit(String),
    Extracted(String),
    Confidence(f64),
    Observation(Vec<String>),
    Parameters(Vec<PatternParam>),
    Behavior(BehaviorDecl),
    AppliesTo(GlobPattern),
}

/// Intermediate type for parsing protocol items.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolItemParsed {
    Phases(Vec<PhaseRef>),
    Property(TemporalProperty),
}

/// Intermediate type for parsing suppression items.
#[derive(Debug, Clone, PartialEq)]
pub enum SuppressionItemParsed {
    Exception(Vec<String>),
    Reason(String),
    Expires(String),
    Tracking(String),
}

impl SuppressionItemParsed {
    /// Create a suppression item from a field name and string value.
    /// Returns None if the field name is not recognized.
    pub fn from_field(name: &str, value: String) -> Option<Self> {
        match name {
            "reason" => Some(Self::Reason(value)),
            "expires" => Some(Self::Expires(value)),
            "tracking" => Some(Self::Tracking(value)),
            _ => None,
        }
    }
}
