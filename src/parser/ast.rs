/// Byte offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A parsed Intent concern — the top-level unit of an `.intent` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Concern {
    pub name: String,
    pub items: Vec<ConcernItem>,
    pub span: Option<Span>,
}

/// Items that can appear inside a `concern { }` block.
#[derive(Debug, Clone, PartialEq)]
pub enum ConcernItem {
    Scope(ScopeDecl),
    Constraint(ConstraintDecl),
    Layer(LayerDecl),
    Apply(PatternApplication),
    DecidedBecause(Vec<String>),
    RejectedAlternatives(Vec<(String, String)>),
    RevisitWhen(Vec<String>),
    UseScope { concern: String, scope: String },
    Parameter(ParameterDecl),
    Invariant(InvariantDecl),
    StateMachine(StateMachineDecl),
    Bridge(BridgeDecl),
    /// `let name = scope_expr`
    Let { name: String, expr: ScopeExpr },
    /// `predicate name(params) { rules }`
    Predicate(PredicateDecl),
}

/// A layer declaration for layered architecture constraints.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerDecl {
    pub name: String,
    pub entities: Vec<String>,
}

/// A scope declaration, either an entity list or an access boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeDecl {
    pub name: String,
    pub kind: ScopeKind,
    pub within: Option<Vec<String>>,
    pub lang: Option<String>,
}

/// The two forms of scope declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeKind {
    /// `scope foo { [A, B, C] }` — names a set of entities.
    EntityList(Vec<String>),
    /// `scope foo { only [A] accesses B }` — declares an access boundary.
    OnlyAccesses {
        accessors: Vec<String>,
        target: String,
    },
}

/// A constraint declaration containing one or more rules.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintDecl {
    pub name: String,
    pub rules: Vec<ConstraintRule>,
    pub status: Option<ConstraintStatus>,
    pub covers: Vec<String>,
}

/// Constraint status for plan-mode tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintStatus {
    Planned,
    Active,
    Deferred,
}

/// Constraint rule variants.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintRule {
    /// `[A, B] must_not depend_on C`
    MustNotDependOn { from: Vec<String>, target: String },
    /// `[A, B] must_not reference [C, D]`
    MustNotReference {
        from: Vec<String>,
        targets: Vec<String>,
    },
    /// `[A, B] must_depend_on C`
    MustDependOn { from: Vec<String>, target: String },
    /// `[A, B] must_reference [C, D]`
    MustReference {
        from: Vec<String>,
        targets: Vec<String>,
    },
    /// `Pattern occur_only_in [module_a, module_b]`
    OccurOnlyIn {
        pattern: String,
        modules: Vec<String>,
    },
    /// `TypeName must_implement TraitName`
    MustImplement {
        type_name: String,
        trait_name: String,
    },
    /// `when_present field requires [fields]`
    WhenPresent {
        field: String,
        requires: Vec<String>,
    },
    /// `mutually_exclusive [fields]`
    MutuallyExclusive {
        fields: Vec<String>,
    },
    /// `forall var in domain: body` or `forall var in domain { body }`
    Forall {
        var: String,
        domain: ScopeExpr,
        body: Vec<ConstraintRule>,
        /// Optional where clause for filtering (v0.3)
        where_clause: Option<WhereClause>,
    },
    /// `exists var in domain: body` or `exists var in domain { body }`
    Exists {
        var: String,
        domain: ScopeExpr,
        body: Vec<ConstraintRule>,
        /// Optional where clause for filtering (v0.3)
        where_clause: Option<WhereClause>,
    },
    /// `condition => consequence`
    Implies {
        condition: Condition,
        consequence: Box<ConstraintRule>,
    },
    /// `name(args)` — predicate application
    Call {
        name: String,
        args: Vec<ScopeExpr>,
    },
    /// `(|x| body)(arg)` — lambda application (v0.3)
    LambdaApply {
        lambda: LambdaExpr,
        arg: ScopeExpr,
    },
}

/// A pattern application (`apply Pattern(...) to Target { refines "..." }`).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternApplication {
    pub pattern: String,
    pub params: Vec<(String, ParamValue)>,
    pub target: String,
    pub refines: Option<String>,
}

/// Parameter values in pattern applications and parameter declarations.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Duration(u64),
    Str(String),
    Float(f64),
}

/// A parameter declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterDecl {
    pub name: String,
    pub value: ParamValue,
}

/// An invariant declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct InvariantDecl {
    pub name: String,
    pub expressions: Vec<InvariantExpr>,
}

/// An invariant expression.
#[derive(Debug, Clone, PartialEq)]
pub enum InvariantExpr {
    Comparison {
        lhs: ArithExpr,
        op: ComparisonOp,
        rhs: ArithExpr,
    },
}

/// Comparison operators for invariant expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// Arithmetic expressions for invariants.
#[derive(Debug, Clone, PartialEq)]
pub enum ArithExpr {
    Literal(f64),
    Ident(String),
    BinOp {
        lhs: Box<ArithExpr>,
        op: ArithOp,
        rhs: Box<ArithExpr>,
    },
    Neg(Box<ArithExpr>),
}

/// Arithmetic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// A state machine declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StateMachineDecl {
    pub name: String,
    pub states: Vec<String>,
    pub initial: String,
    pub terminal: Vec<String>,
    pub transitions: Vec<(String, String)>,
    pub invariants: Vec<SmInvariant>,
    pub refines: Option<String>,
}

/// A state machine invariant.
#[derive(Debug, Clone, PartialEq)]
pub struct SmInvariant {
    pub name: String,
    pub kind: SmInvariantKind,
}

/// State machine invariant kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum SmInvariantKind {
    /// `must_not reach from -> to`
    MustNotReach { from: String, to: String },
    /// `was(state)` - state must have been visited (for invariants like "DELIVERED => was(SHIPPED)")
    WasVisited { target_state: String, required_prior: String },
    /// `terminal_states are_absorbing` - terminal states have no outgoing transitions
    TerminalAbsorbing,
    /// Custom TLA+ expression for temporal invariant
    Custom { expr: String },
}

/// A bridge declaration connecting entities across languages.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeDecl {
    pub name: String,
    pub source: BridgeEndpoint,
    pub sink: BridgeEndpoint,
    pub events: Vec<String>,
    pub constraint_type: BridgeConstraintType,
}

/// An endpoint in a bridge (entity with optional language).
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeEndpoint {
    pub entity: String,
    pub lang: Option<String>,
}

/// Bridge constraint types.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeConstraintType {
    Bidirectional,
    FunctionSignaturesMatch,
}

// --- v0.2: Set algebra, quantifiers, predicates, implication ---

/// Set expressions for scope composition (set algebra on entity sets).
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeExpr {
    /// `[A, B, C]` — literal entity set
    EntityList(Vec<String>),
    /// Bare name — scope, let binding, quantifier variable, or entity
    Ident(String),
    /// Glob pattern (`*Client`, `Service*`)
    Glob(String),
    /// `a | b` — set union
    Union(Box<ScopeExpr>, Box<ScopeExpr>),
    /// `a & b` — set intersection
    Intersection(Box<ScopeExpr>, Box<ScopeExpr>),
    /// `a \ b` — set difference
    Difference(Box<ScopeExpr>, Box<ScopeExpr>),
    /// `{ x | x matches *Pattern }` — set comprehension
    Comprehension { var: String, pattern: String },
}

/// Condition (testable property) for implication antecedents.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// `entity depends_on target`
    DependsOn { entity: String, target: String },
    /// `entity references target`
    References { entity: String, target: String },
}

/// A predicate definition — parameterized, reusable constraint template.
#[derive(Debug, Clone, PartialEq)]
pub struct PredicateDecl {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<ConstraintRule>,
}

// --- v0.3: System hierarchy, refinement, lambdas ---

/// A system declaration for hierarchical composition of subsystems.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemDecl {
    pub name: String,
    /// Optional prose description.
    pub description: Option<String>,
    /// Names of subsystem systems.
    pub subsystems: Vec<String>,
    /// Shared scopes visible to subsystems.
    pub scopes: Vec<ScopeDecl>,
    /// Cross-subsystem constraints.
    pub constraints: Vec<ConstraintDecl>,
    /// Path to abstract TLA+ spec this system refines.
    pub refines: Option<String>,
    /// Explicit mapping from abstract states to concrete states.
    pub refinement_map: Option<RefinementMap>,
    /// Span in source text.
    pub span: Option<Span>,
}

/// A refinement map for explicit mapping between abstract and concrete states.
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementMap {
    /// List of mappings from abstract -> concrete states.
    pub mappings: Vec<RefinementMapping>,
}

/// A single mapping in a refinement map.
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementMapping {
    /// Abstract state (possibly dotted, e.g., "abstract.completed").
    pub abstract_state: String,
    /// Concrete states that map to this abstract state.
    pub concrete_states: Vec<String>,
}

/// A lambda expression: `|params| body`.
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaExpr {
    pub params: Vec<String>,
    pub body: Box<ConstraintRule>,
}

/// A where clause for filtering in quantifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    pub condition: Condition,
}

/// Items that can appear inside a `system { }` block.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemItem {
    /// `description "..."`
    Description(String),
    /// `subsystems [A, B, C]`
    Subsystems(Vec<String>),
    /// `scope shared { [A, B] }`
    Scope(ScopeDecl),
    /// `constraint name { ... }`
    Constraint(ConstraintDecl),
    /// `refines "path/to/spec.tla"`
    Refines(String),
    /// `refinement_map { ... }`
    RefinementMap(RefinementMap),
}

/// Top-level declaration in an Intent file.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Concern(Concern),
    System(SystemDecl),
}
