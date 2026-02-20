/// Byte offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Top-level declaration in an Intent file.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Import(ImportDecl),
    System(SystemDecl),
    Pattern(PatternDecl),
    Rationale(RationaleDecl),
}

/// Import declaration for patterns and templates.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub kind: ImportKind,
    pub name: String,
    pub source: String,
    pub with_params: Vec<(String, ParamValue)>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Pattern,
    Template,
}

/// A system declaration - the primary container.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemDecl {
    pub name: String,
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
    /// Span in source text
    pub span: Option<Span>,
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
    pub span: Option<Span>,
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
            span: None,
        }
    }
}

/// A constraint declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintDecl {
    pub name: String,
    pub rules: Vec<ConstraintRule>,
    pub span: Option<Span>,
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
    /// `forall x in S: rule`
    Forall { var: String, domain: ScopeExpr, body: Box<ConstraintRule> },
    /// `exists x in S: rule`
    Exists { var: String, domain: ScopeExpr, body: Box<ConstraintRule> },
    /// Predicate call: `A.depends(B)`, `A.references(B)`, etc.
    Predicate(PredicateCall),
    /// Comparison: `p99(op) < 100ms`
    Comparison { lhs: Expr, op: ComparisonOp, rhs: Expr },
    /// User-defined predicate call: `A.myPredicate(B, C)`
    Call { subject: ScopeExpr, name: String, args: Vec<ScopeExpr> },
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
}

/// Set expressions for scope composition.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeExpr {
    EntityList(Vec<String>),
    Ident(String),
    Glob(String),
    Union(Box<ScopeExpr>, Box<ScopeExpr>),
    Intersection(Box<ScopeExpr>, Box<ScopeExpr>),
    Difference(Box<ScopeExpr>, Box<ScopeExpr>),
    Matches { var: String, pattern: String },
    /// Filtered set: { x | x.field == value }
    Filtered { var: String, condition: Expr },
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

/// A behavior declaration (state machine).
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDecl {
    pub name: String,
    /// `composes [A.Flow, B.Flow]`
    pub composes: Vec<String>,
    /// `nodes: replicas` - optional node set for distributed systems
    pub nodes: Option<String>,
    /// States
    pub states: Vec<StateDecl>,
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
    pub span: Option<Span>,
}

impl Default for BehaviorDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            composes: Vec::new(),
            nodes: None,
            states: Vec::new(),
            transitions: Vec::new(),
            properties: Vec::new(),
            fairness: Vec::new(),
            invariants: Vec::new(),
            refines: None,
            applies: Vec::new(),
            refinement_map: None,
            strengthens: Vec::new(),
            span: None,
        }
    }
}

/// A state declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    pub name: String,
    pub initial: bool,
    pub terminal: bool,
}

/// A transition declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionDecl {
    pub from: String,
    pub to: String,
    pub on_event: String,
    pub guard: Option<Expr>,
    pub effects: Vec<EffectStmt>,
    pub timing: Option<TransitionTiming>,
    pub span: Option<Span>,
}

/// An effect statement.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectStmt {
    pub kind: EffectKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectKind {
    Emit { name: String, args: Vec<Expr> },
    If { cond: Expr, then_effects: Vec<EffectStmt>, else_effects: Option<Vec<EffectStmt>> },
    Expr(Expr),
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

/// Temporal expressions (LTL-complete).
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

/// A pattern declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub parameters: Vec<PatternParam>,
    pub behavior: Option<BehaviorDecl>,
    pub span: Option<Span>,
}

/// A pattern parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternParam {
    pub name: String,
    pub type_name: String,
    pub constraints: Vec<FieldConstraint>,
}

/// Field constraints.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldConstraint {
    Min(ParamValue),
    Max(ParamValue),
    Default(ParamValue),
}

/// A pattern application.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternApplication {
    pub pattern: String,
    pub type_args: Vec<String>,
    pub params: Vec<(String, ParamValue)>,
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
    pub observation: Option<Vec<String>>,
    pub parameters: Vec<PatternParam>,
    pub behavior: Option<BehaviorDecl>,
    pub applies_to: Option<GlobPattern>,
    pub span: Option<Span>,
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
    pub span: Option<Span>,
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
}

/// Intermediate type for parsing component items.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentItemParsed {
    Implements(String),
    Contains(Vec<String>),
    DependsOnly(Vec<String>),
    Component(ComponentDecl),
    Behavior(BehaviorDecl),
}

/// Intermediate type for parsing behavior items.
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorItemParsed {
    Nodes(String),
    States(Vec<StateDecl>),
    Transitions(Vec<TransitionDecl>),
    Property(TemporalProperty),
    Fairness(Vec<FairnessSpec>),
    Invariant(InvariantDecl),
    Refines(String),
    Applies(PatternApplication),
    Map(RefinementMap),
    Strengthens(Strengthens),
}

/// Intermediate type for parsing pattern items.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternItemParsed {
    Parameters(Vec<PatternParam>),
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
}

/// Intermediate type for parsing distilled pattern items.
#[derive(Debug, Clone, PartialEq)]
pub enum DistilledPatternItemParsed {
    Source(String),
    Commit(String),
    Extracted(String),
    Observation(Vec<String>),
    Parameters(Vec<PatternParam>),
    Behavior(BehaviorDecl),
    AppliesTo(GlobPattern),
}
