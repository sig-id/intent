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
    MustNotReach { from: String, to: String },
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
