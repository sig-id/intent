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
}

/// A pattern application (`apply Pattern(...) to Target { refines "..." }`).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternApplication {
    pub pattern: String,
    pub params: Vec<(String, ParamValue)>,
    pub target: String,
    pub refines: Option<String>,
}

/// Parameter values in pattern applications.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Duration(u64),
    Str(String),
}
