/// Byte offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Maturity level for systems and concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Maturity {
    Sketch,
    Draft,
    #[default]
    Spec,
    Final,
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
    Behavior(BehaviorDecl),
    Bridge(BridgeDecl),
    /// `let name = scope_expr`
    Let { name: String, expr: ScopeExpr },
    /// `predicate name(params) { rules }`
    Predicate(PredicateDecl),
    /// `model Name { ... }`
    Model(ModelDecl),
    /// `distilled from "..." { ... }`
    Distilled(DistilledFrom),
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
    /// Maturity level of this system.
    pub maturity: Maturity,
    /// Optional prose description.
    pub description: Option<String>,
    /// Parent system name (if this is a subsystem).
    pub parent: Option<String>,
    /// Names of subsystem systems.
    pub subsystems: Vec<String>,
    /// Implementation path binding.
    pub implements: Option<String>,
    /// Shared scopes visible to subsystems.
    pub scopes: Vec<ScopeDecl>,
    /// Cross-subsystem constraints.
    pub constraints: Vec<ConstraintDecl>,
    /// Models defined in this system.
    pub models: Vec<ModelDecl>,
    /// Interfaces defined in this system.
    pub interfaces: Vec<InterfaceDecl>,
    /// Behaviors defined in this system.
    pub behaviors: Vec<BehaviorDecl>,
    /// Let bindings.
    pub let_bindings: Vec<(String, ScopeExpr)>,
    /// Predicates.
    pub predicates: Vec<PredicateDecl>,
    /// Pattern applications.
    pub applies: Vec<PatternApplication>,
    /// Path to abstract TLA+ spec this system refines.
    pub refines: Option<String>,
    /// Explicit mapping from abstract states to concrete states.
    pub refinement_map: Option<RefinementMap>,
    /// Progression stages.
    pub progression: Option<Progression>,
    /// Current implementation stage.
    pub current_stage: Option<String>,
    /// Rationale: decided because.
    pub decided_because: Vec<String>,
    /// Rationale: rejected alternatives.
    pub rejected_alternatives: Vec<(String, String)>,
    /// Rationale: revisit when.
    pub revisit_when: Vec<String>,
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
    Deployment(DeploymentDecl),
    Pipeline(PipelineDecl),
    Tooling(ToolingDecl),
    DistilledPattern(DistilledPatternDecl),
    Insight(InsightDecl),
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Model declarations
// ═══════════════════════════════════════════════════════════════════════════

/// A model declaration defining data schema with invariants.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub enums: Vec<EnumDecl>,
    pub derived: Vec<DerivedField>,
    pub invariants: Vec<ModelInvariant>,
}

/// A field declaration in a model.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub type_name: String,
    pub optional: bool,
    pub constraints: Vec<FieldConstraint>,
}

/// Constraints on a field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldConstraint {
    Min(ParamValue),
    Max(ParamValue),
    Pattern(String),
    Default(ParamValue),
}

/// An enum declaration in a model.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<String>,
}

/// A derived field in a model.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedField {
    pub name: String,
    pub expr: String,
}

/// A model invariant.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInvariant {
    pub name: String,
    pub expr: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Interface declarations
// ═══════════════════════════════════════════════════════════════════════════

/// An interface declaration for subsystem contracts.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub name: String,
    pub source: String,
    pub target: String,
    pub maturity: Maturity,
    pub operations: Vec<OperationDecl>,
    pub protocols: Vec<ProtocolDecl>,
    pub invariants: Vec<ModelInvariant>,
}

/// An operation declaration in an interface.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDecl {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub return_type: String,
    pub requires: Vec<String>,
    pub ensures: Vec<String>,
}

/// A protocol declaration in an interface.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecl {
    pub name: String,
    pub steps: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Behavior declarations (enhanced statemachine)
// ═══════════════════════════════════════════════════════════════════════════

/// A behavior declaration with state machine and temporal properties.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDecl {
    pub name: String,
    pub maturity: Maturity,
    pub composes: Vec<String>,
    pub states: Vec<StateDecl>,
    pub transitions: Vec<TransitionDecl>,
    pub properties: Vec<TemporalProperty>,
    pub fairness: Vec<FairnessSpec>,
    pub invariants: Vec<ModelInvariant>,
    pub refines: Option<String>,
}

/// A state declaration in a behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    pub name: String,
    pub initial: bool,
    pub terminal: bool,
}

/// A transition declaration in a behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionDecl {
    pub from: String,
    pub to: String,
    pub on_event: String,
    pub guard: Option<String>,
    pub timing: Option<TransitionTiming>,
}

/// Timing constraint on a transition.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionTiming {
    Within(String),
    After(String),
}

/// A temporal property in a behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalProperty {
    pub name: String,
    pub expr: TemporalExpr,
}

/// Temporal expressions for properties.
#[derive(Debug, Clone, PartialEq)]
pub enum TemporalExpr {
    Always(String),
    Eventually(String),
    AlwaysEventually { premise: String, conclusion: String },
    Was(String),
    Raw(String),
}

/// Fairness specification.
#[derive(Debug, Clone, PartialEq)]
pub struct FairnessSpec {
    pub kind: FairnessKind,
    pub from: String,
    pub to: String,
    pub alt: Option<String>,
}

/// Fairness kind (weak or strong).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FairnessKind {
    Weak,
    Strong,
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Progression (implementation staging)
// ═══════════════════════════════════════════════════════════════════════════

/// Progression declaration with implementation stages.
#[derive(Debug, Clone, PartialEq)]
pub struct Progression {
    pub stages: Vec<Stage>,
}

/// A single implementation stage.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub name: String,
    pub extends: Option<String>,
    pub scope: Option<ScopeExpr>,
    pub constraints: StageConstraints,
    pub behaviors: StageBehaviors,
    pub target: Option<String>,
}

/// Constraints for a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageConstraints {
    All,
    List(Vec<String>),
}

/// Behaviors for a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageBehaviors {
    All,
    List(Vec<BehaviorRef>),
}

/// Reference to a behavior, optionally with a subset of states.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRef {
    pub name: String,
    pub subset: Option<Vec<String>>,
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Distillation
// ═══════════════════════════════════════════════════════════════════════════

/// A distilled pattern extracted from implementation.
#[derive(Debug, Clone, PartialEq)]
pub struct DistilledPatternDecl {
    pub name: String,
    pub source: Option<String>,
    pub extracted: Option<String>,
    pub observation: Option<String>,
    pub parameters: Vec<DistilledParam>,
    pub behavior: Option<BehaviorDecl>,
    pub applies_to: Vec<String>,
}

/// A parameter in a distilled pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct DistilledParam {
    pub name: String,
    pub type_name: String,
    pub constraints: Vec<FieldConstraint>,
}

/// A distillation marker on a concern item.
#[derive(Debug, Clone, PartialEq)]
pub struct DistilledFrom {
    pub source: String,
    pub commit: Option<String>,
    pub observation: Option<String>,
}

/// An insight captured from implementation review.
#[derive(Debug, Clone, PartialEq)]
pub struct InsightDecl {
    pub name: String,
    pub discovered: Option<String>,
    pub source: Option<String>,
    pub observation: Option<String>,
    pub recommendation: Vec<ConcernItem>,
    pub status: InsightStatus,
}

/// Status of an insight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsightStatus {
    #[default]
    Proposed,
    Accepted,
    Rejected,
}

// ═══════════════════════════════════════════════════════════════════════════
// v0.3: Deployment and Tooling
// ═══════════════════════════════════════════════════════════════════════════

/// A deployment declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct DeploymentDecl {
    pub name: String,
    pub platform: Option<String>,
    pub mappings: Vec<DeploymentMapping>,
    pub dependencies: Vec<(String, String)>,
    pub constraints: Vec<ConstraintDecl>,
}

/// A deployment mapping from subsystem to deployment unit.
#[derive(Debug, Clone, PartialEq)]
pub struct DeploymentMapping {
    pub subsystem: String,
    pub target: String,
    pub config: Vec<(String, ParamValue)>,
}

/// A pipeline declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineDecl {
    pub name: String,
    pub stages: Vec<PipelineStage>,
    pub triggers: Vec<PipelineTrigger>,
}

/// A stage in a pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineStage {
    pub name: String,
    pub runs: Vec<String>,
    pub gate: Option<String>,
    pub timeout: Option<u64>,
}

/// A trigger for a pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineTrigger {
    pub event: String,
    pub stages: TriggerStages,
}

/// Which stages to run for a trigger.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerStages {
    All,
    List(Vec<String>),
}

/// A tooling declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolingDecl {
    pub languages: Vec<ToolingLanguage>,
    pub framework: Option<String>,
    pub storage: Vec<ToolingStorage>,
    pub formal: Vec<(String, String)>,
    pub decided_because: Vec<String>,
}

/// A language specification in tooling.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolingLanguage {
    pub name: String,
    pub config: Vec<(String, ParamValue)>,
}

/// A storage specification in tooling.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolingStorage {
    pub role: String,
    pub backend: String,
    pub config: Vec<(String, ParamValue)>,
}
