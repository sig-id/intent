# Intent Language Specification

**Version:** 0.3.0  
**Status:** Living document  
**Updated:** 2026-02-16  
**Grammar:** [`src/parser/intent.lalrpop`](src/parser/intent.lalrpop)

---

## 1. Overview

Intent is a domain-specific language for machine-verifiable architectural design constraints. It captures the complete specification lifecycle:

```
Ideation ──► Refinement ──► Specification ──► TLA+ ──► Implementation
    ▲                                                        │
    └────────────────── Distillation ◄───────────────────────┘
```

**Core abstractions:**
- **System** — hierarchical decomposition into subsystems
- **Model** — data schemas with type constraints and invariants
- **Interface** — contracts exposed by a module
- **Adapter** — connects interfaces (many-to-many relationships)
- **Behavior** — state machines with events, effects, and temporal properties
- **Pattern** — reusable parameterized behaviors
- **Constraint** — cross-cutting structural and non-functional properties
- **Concern** — flat collection of related constraints (v0.2 compatible)

Intent operates at the architectural level. It does not replace prose specifications, formal models (TLA+/Quint), or implementation-level contracts. It captures the machine-checkable subset of architectural intent.

---

## 2. Lexical Structure

### 2.1 Character Set

Intent source files are UTF-8 encoded. The grammar operates on ASCII keywords and identifiers; string literals may contain arbitrary UTF-8.

### 2.2 Whitespace and Comments

```intent
// Line comment
/* Block comment */
```

Whitespace is insignificant except within string literals.

### 2.3 Identifiers

```
IDENT = [a-zA-Z_][a-zA-Z0-9_]*
```

Identifiers name systems, concerns, scopes, constraints, models, etc. Case-sensitive.

### 2.4 Glob Patterns

```
PREFIX_GLOB = *[a-zA-Z0-9_]+     // e.g., *Client
SUFFIX_GLOB = [a-zA-Z_][a-zA-Z0-9_]*\*    // e.g., Dgraph*
```

### 2.5 Literals

| Type | Syntax | Examples |
|------|--------|----------|
| Int | `[0-9]+(_[0-9]+)*` | `5`, `100`, `1_000_000` |
| Float | `[0-9]+\.[0-9]+` | `0.03`, `1.5` |
| Percent | `[0-9]+(\.[0-9]+)?%` | `5%`, `2.5%` |
| Duration | `[0-9]+[μsmhd]` | `100μs`, `30s`, `5m`, `2h`, `7d` |
| String | `"[^"]*"` | `"reason text"` |

### 2.6 Keywords

```
// Core
system     concern     scope       constraint   layer
model      interface   adapter     behavior     pattern

// Composition
subsystems  parent      refines     refinement_map  action_map
strengthens implements  use

// Model
fields      enum        derived     invariant

// Interface
owner       operation   requires    ensures     protocol

// Adapter
connects    mapping     transforms  error_handling

// Behavior
states      transitions initial     terminal    on
where       within      after       property    fairness
weak        strong      subscribes  emits       effect
handles     publishes   command     guard       event_sourced
events      stream

// Pattern
parameters  applies

// Constraint
must_not    depend_on   reference   must_depend_on  must_reference
occur_only_in           must_implement
forall      exists      in          matches     predicate
depends_on  references  category

// Lifecycle
maturity    sketch      draft       spec        final
progression stage       extends     current_stage
distilled   pattern     from        applies_to  extracted
commit      insight     observation recommendation  status  proposed
accepted    rejected

// Deployment
deployment  platform    mapping     dependencies
pipeline    stages      runs        gate        timeout
triggers    tooling

// Rationale
decided     because     alternatives    revisit     when

// Operators
let         all
```

---

## 3. Top-Level Declarations

A file contains zero or more top-level declarations:

```
File = { TopLevel }
TopLevel = System | Concern | Deployment | Pipeline | Tooling
         | DistilledPattern | Insight | Pattern
```

---

## 4. System Declaration

Systems provide hierarchical decomposition. A system can contain subsystems, models, interfaces, behaviors, and constraints.

```
System = "system" IDENT [ "refines" IDENT ] "{" { SystemItem } "}"
```

### 4.1 Basic System

```intent
system PaymentPlatform {
    maturity: spec
    description "Payment processing with settlement"
    
    subsystems [Ingestion, Processing, Settlement]
    
    model Transaction { ... }
    interface PaymentAPI: Ingestion -> Processing { ... }
    behavior OrderFlow { ... }
    constraint architecture { ... }
    
    decided because { "..." }
}
```

### 4.2 Subsystem

```intent
system Processing {
    parent: PaymentPlatform
    maturity: spec
    
    implements "crates/processing/src"
    
    model ValidationResult { ... }
    behavior TransactionLifecycle { ... }
}
```

### 4.3 Maturity Levels

```intent
maturity: sketch   // Ideation, no verification
maturity: draft    // Structured but incomplete
maturity: spec     // Complete, verifiable
maturity: final    // Verified, locked
```

### 4.4 Refinement

```intent
system Concrete refines Abstract {
    refinement_map {
        Abstract.pending -> [Concrete.queued, Concrete.validating]
        Abstract.done -> [Concrete.settled]
    }
    
    action_map {
        Abstract.process -> [Concrete.validate, Concrete.execute]
    }
    
    strengthens Abstract.safety with local_safety
}
```

---

## 5. Concern Declaration (v0.2 Compatible)

Concerns are flat collections of constraints. Retained for backward compatibility.

```
Concern = "concern" IDENT "{" { ConcernItem } "}"
```

```intent
concern ResilientStorage {
    scope storage_backends { [DgraphClient, MilvusClient] }
    
    constraint no_direct_access {
        [services] must_not depend_on storage_backends
    }
    
    decided because { "Circuit breakers prevent cascading failures." }
}
```

---

## 6. Model Declaration

Models define data schemas with type constraints and invariants.

```
Model = "model" IDENT "{" { ModelItem } "}"
ModelItem = Fields | Enum | Derived | Invariant
```

### 6.1 Fields

```intent
model Transaction {
    fields {
        id: UUID
        amount: Decimal { min: 0.01, max: 1_000_000 }
        currency: Currency
        status: Status
        created_at: Timestamp
        settled_at: Timestamp?    // optional
    }
}
```

### 6.2 Field Constraints

```intent
fields {
    amount: Decimal { min: 0, max: 100_000 }
    retries: Int { max: 5 }
    email: String { pattern: ".*@.*" }
    rate: Float { default: 0.03 }
}
```

### 6.3 Enums

```intent
model Order {
    enum Side { buy, sell }
    enum Status { pending, filled, cancelled }
    
    fields {
        side: Side
        status: Status
    }
}
```

### 6.4 Derived Fields

```intent
model Transaction {
    fields {
        created_at: Timestamp
        settled_at: Timestamp?
    }
    
    derived processing_time { settled_at - created_at }
}
```

### 6.5 Model Invariants

```intent
model Transaction {
    fields {
        amount: Decimal
        fee: Decimal
        net: Decimal
    }
    
    invariant fee_calculation { net == amount - fee }
    invariant positive_net { net > 0 }
}
```

---

## 7. Interface Declaration

Interfaces define contracts exposed by a module. Each interface has an owner and defines operations with pre/post conditions.

```
Interface = "interface" IDENT [ "extends" IDENT ] "{" { InterfaceItem } "}"
InterfaceItem = Owner | Maturity | Operation | Protocol | InterfaceInvariant
```

### 7.1 Interface with Owner

```intent
interface SettlementAPI {
    owner: Settlement
    maturity: spec

    operation settle(batch: Batch) -> Result<SettlementId, Error> {
        requires { batch.transactions.all(t => t.status == processing) }
        ensures { result.ok => batch.transactions.all(t => t.status == settled) }
        ensures { result.err => batch.transactions.all(t => t.status == failed) }
    }

    operation get_status(id: SettlementId) -> Option<SettlementStatus>
}
```

### 7.2 Interface Invariants

```intent
interface OrderAPI {
    owner: OrderService

    operation submit(order: Order) -> OrderId
    operation cancel(id: OrderId) -> Result

    invariant idempotent_cancel {
        forall id: cancel(id).ok => cancel(id) == cancel(id)
    }
}
```

### 7.3 Protocol Sequences

```intent
interface AuthAPI {
    owner: AuthService

    operation login(creds: Credentials) -> Token
    operation refresh(token: Token) -> Token
    operation logout(token: Token) -> void

    protocol session_flow {
        login -> (refresh)* -> logout
    }
}
```

### 7.4 Interface Inheritance

```intent
interface AsyncSettlementAPI extends SettlementAPI {
    operation settle_async(batch: Batch) -> SettlementId

    ensures { result -> settle_async_callback within 30s }
}
```

---

## 8. Adapter Declaration

Adapters connect interfaces, enabling many-to-many relationships between modules.

```
Adapter = "adapter" IDENT "{" { AdapterItem } "}"
AdapterItem = Connects | Mapping | Transforms | ErrorHandling
```

### 8.1 Basic Adapter

```intent
adapter SettlementAdapter {
    connects: Processing.SettlementPort -> SettlementAPI

    mapping {
        Processing.batch_request -> SettlementAPI.settle
        Processing.status_query -> SettlementAPI.get_status
    }

    transforms {
        batch.transactions -> batch.transactions.map(t => SettlementTransaction {
            id: t.id,
            amount: t.amount,
            currency: t.currency
        })
    }

    error_handling {
        SettlementAPI.settle.Timeout -> Processing.BatchFailed
        SettlementAPI.settle.Rejected -> Processing.BatchRejected
    }
}
```

### 8.2 Many-to-Many Relationships

```intent
// Multiple consumers of one interface
adapter IngestionSettlement {
    connects: Ingestion.SinkPort -> SettlementAPI
}

adapter ReportingSettlement {
    connects: Reporting.QueryPort -> SettlementAPI
}

// One consumer of multiple interfaces
adapter OrchestrationAdapter {
    connects: Processing.Outbound -> [SettlementAPI, NotificationAPI, AuditAPI]
}
```

---

## 9. Behavior Declaration

Behaviors define state machines with events, effects, and temporal properties.

```
Behavior = "behavior" IDENT [ "composes" IdentList ] "{" { BehaviorItem } "}"
BehaviorItem = EventChannels | StatesDecl | TransitionsDecl | CommandDecl
            | Property | Fairness | EventSource | BehaviorInvariant | AppliesPattern
```

### 9.1 States

```intent
behavior OrderLifecycle {
    states {
        pending     { initial: true }
        validating
        processing
        settled     { terminal: true }
        failed      { terminal: true }
    }
}
```

### 9.2 Transitions

```intent
behavior OrderLifecycle {
    transitions {
        pending -> validating       on receive
        validating -> processing    on valid
        validating -> failed        on invalid
        processing -> settled       on confirmed
        processing -> failed        on timeout
    }
}
```

### 9.3 Guarded Transitions

```intent
transitions {
    pending -> express    on receive   where { amount > 10_000 }
    pending -> standard   on receive   where { amount <= 10_000 }
    settled -> reversed   on reversal  within { 24h }
    waiting -> retry      on timeout   after { delay * backoff^attempts }
}
```

### 9.4 Temporal Properties

```intent
behavior OrderLifecycle {
    property eventual_completion {
        always(pending => eventually(settled | failed))
    }

    property failure_permanent {
        always(failed => always(failed))
    }

    property ordering_preserved {
        always(settled => was(processing))
    }
}
```

### 9.5 Fairness

```intent
behavior OrderLifecycle {
    fairness {
        weak(validating -> processing | failed)
        strong(processing -> settled | failed)
    }
}
```

### 9.6 Event-Driven Behaviors with Effects

```intent
behavior OrderProcessor {
    // Event channels this behavior subscribes to
    subscribes [OrderCreated, PaymentCompleted, InventoryReserved]

    // Commands this behavior emits
    emits [ReserveInventory, ProcessPayment, ShipOrder]

    states {
        idle        { initial: true }
        reserving
        charging
        completed   { terminal: true }
    }

    transitions {
        idle -> reserving on OrderCreated
            effect { emit ReserveInventory(order_id, items) }

        reserving -> charging on InventoryReserved
            effect { emit ProcessPayment(order_id, total) }

        charging -> completed on PaymentCompleted
            effect { emit ShipOrder(order_id, address) }
    }
}
```

### 9.7 Command Handlers

```intent
behavior PaymentHandler {
    // Commands: handled by this behavior
    handles [ProcessPayment, RefundPayment]

    // Events: published as outcomes
    publishes [PaymentCompleted, PaymentFailed]

    states { idle, processing }

    command ProcessPayment(cmd) {
        guard { cmd.amount > 0 }

        idle -> processing {
            effect {
                if validate_card(cmd.card) {
                    emit PaymentCompleted(cmd.id, cmd.amount)
                } else {
                    emit PaymentFailed(cmd.id, "invalid_card")
                }
            }
        }
    }
}
```

### 9.8 Event Sourcing

```intent
behavior AccountAggregate {
    event_sourced true
    stream "accounts/{account_id}"

    events {
        AccountOpened { account_id: UUID, owner: String }
        MoneyDeposited { account_id: UUID, amount: Decimal }
        MoneyWithdrawn { account_id: UUID, amount: Decimal }
    }

    // State derived from event history
    state balance {
        initial: 0
        on MoneyDeposited: + event.amount
        on MoneyWithdrawn: - event.amount
    }

    command Deposit(amount: Decimal) {
        guard { amount > 0 }
        emits [MoneyDeposited]
    }

    command Withdraw(amount: Decimal) {
        guard { balance >= amount }
        emits [MoneyWithdrawn]
    }

    invariant positive_balance {
        balance >= 0
    }
}
```

### 9.9 Applying Patterns

```intent
behavior OrderFulfillment {
    applies Saga {
        steps: [
            { command: ReserveInventory, success: InventoryReserved },
            { command: ProcessPayment, success: PaymentCompleted }
        ]
        compensate: {
            ReserveInventory -> ReleaseInventory,
            ProcessPayment -> RefundPayment
        }
        timeout: 30m
    }
}
```

### 9.10 Composed Behaviors

```intent
behavior SystemFlow composes [Ingestion.Flow, Processing.Flow] {
    invariant ordering {
        Ingestion.received(t1) < Ingestion.received(t2)
            => Processing.started(t1) <= Processing.started(t2)
    }
}
```

### 9.11 Refinement

```intent
behavior OrderLifecycle {
    refines "formal/tla/OrderFlow.tla"
}
```

---

## 10. Pattern Declaration

Patterns define reusable, parameterized behaviors that can encode any design pattern.

```
Pattern = "pattern" IDENT [ TypeParams ] "{" { PatternItem } "}"
PatternItem = Parameters | Behavior
```

### 10.1 Pattern Definition

```intent
pattern Saga<Step, Compensate> {
    parameters {
        steps: [Step]
        compensate: Step -> Compensate
        timeout: Duration
    }

    behavior {
        states {
            pending     { initial: true }
            running(i: Int)
            compensating(i: Int)
            completed   { terminal: true }
            failed      { terminal: true }
        }

        transitions {
            pending -> running(0) on trigger

            running(i) -> running(i + 1)
                on steps[i].success
                where { i + 1 < steps.length }
                effect { emit steps[i + 1].command }

            running(i) -> completed
                on steps[i].success
                where { i + 1 == steps.length }

            running(i) -> compensating(i)
                on steps[i].failure | timeout

            compensating(i) -> compensating(i - 1)
                on compensate(steps[i]).complete
                where { i > 0 }

            compensating(0) -> failed
                on compensate(steps[0]).complete
        }
    }
}
```

### 10.2 Pattern Composition

```intent
pattern Retry<Op> {
    parameters {
        max_attempts: Int
        initial_delay: Duration
        backoff: Float
    }

    behavior {
        states [pending, attempting(n: Int), waiting(n: Int), succeeded, exhausted]
        initial pending
        terminal [succeeded, exhausted]

        transitions {
            pending -> attempting(1) on invoke
                effect { emit Op }

            attempting(n) -> succeeded on Op.success

            attempting(n) -> waiting(n) on Op.failure
                where { n < max_attempts }

            attempting(n) -> exhausted on Op.failure
                where { n >= max_attempts }

            waiting(n) -> attempting(n + 1)
                after { initial_delay * backoff^(n-1) }
                effect { emit Op }
        }
    }
}

pattern CircuitBreaker<Op> {
    parameters {
        failure_threshold: Int
        success_threshold: Int
        timeout: Duration
    }

    behavior {
        states [closed, open, halfopen]
        initial closed

        transitions {
            closed -> open on Op.failure
                where { failures >= failure_threshold }

            open -> halfopen after { timeout }

            halfopen -> closed on Op.success
                where { successes >= success_threshold }

            halfopen -> open on Op.failure
        }
    }
}
```

### 10.3 Nested Pattern Application

```intent
// Compose patterns: Retry wrapped in CircuitBreaker
behavior ResilientCall {
    applies CircuitBreaker<Retry<ApiCall>> {
        failure_threshold: 5
        success_threshold: 3
        timeout: 30s
        max_attempts: 3
        initial_delay: 100ms
        backoff: 2.0
    }
}
```

### 10.4 Pattern Library

Standard patterns provided:

| Pattern | Purpose |
|---------|---------|
| `Retry` | Retry with configurable backoff |
| `CircuitBreaker` | Fail fast when downstream unhealthy |
| `Timeout` | Abort if operation exceeds duration |
| `Bulkhead` | Limit concurrent executions |
| `RateLimiter` | Throttle requests over time |
| `Saga` | Distributed transaction with compensation |
| `ProcessManager` | Long-running workflow coordinator |
| `Outbox` | Reliable event publishing |

---

## 11. Scope Declarations

Scopes define named sets of code entities.

```
Scope = "scope" IDENT "{" ScopeBody [ "within" IdentList ] "}"
```

### 9.1 Entity List

```intent
scope storage_backends {
    [DgraphClient, MilvusClient, RedisClient]
}
```

### 9.2 Access Boundary

```intent
scope storage_boundary {
    only [StorageCoordinator] accesses storage_backends
}
```

### 9.3 Module Restriction

```intent
scope backends {
    [DgraphClient]
    within [storage, pipeline]
}
```

### 9.4 Set Expressions

```intent
let backends = [DgraphClient, MilvusClient]
let cache = [RedisClient]
let external = backends | cache           // union
let core = services & pipeline            // intersection
let safe = core \ test_helpers            // difference
let clients = { e | e matches *Client }   // comprehension
```

**Operator precedence** (highest to lowest): `&`, `|`, `\`

### 9.5 Cross-Concern References

```intent
use ResilientStorage.storage_backends
```

---

## 12. Constraint Declarations

Constraints assert properties over code structure and system behavior.

```
Constraint = "constraint" IDENT "{" { ConstraintItem } "}"
```

### 12.1 Structural Rules

```intent
constraint architecture {
    [services] must_not depend_on storage_backends
    [services] must_not reference [AuthMiddleware]
    [services] must_depend_on storage
    [services] must_reference [AppError]
    *Client occur_only_in [storage]
    DgraphClient must_implement GraphStore
}
```

### 12.2 Layer Declarations

```intent
constraint layered {
    layer presentation { [routes, handlers] }
    layer application { [services] }
    layer infrastructure { [storage] }
}
```

Layers generate implicit `must_not depend_on` constraints: lower layers cannot depend on higher layers.

### 12.3 Quantifiers

```intent
constraint error_handling {
    forall s in services: s must_reference [AppError]
    exists s in services: s must_depend_on logging
    
    forall m in [services, pipeline] {
        m must_not depend_on external
        m must_reference [Result]
    }
}
```

### 12.4 Implication

```intent
constraint caching_discipline {
    forall m in services:
        m depends_on cache => m must_depend_on cache_invalidation
}
```

### 12.5 Predicates

```intent
predicate isolated(source, target) {
    source must_not depend_on target
    source must_not reference target
}

constraint boundaries {
    isolated(services, storage_backends)
    isolated(pipeline | rag, auth)
}
```

### 12.6 Non-Functional Constraints

```intent
constraint performance {
    category: non_functional
    
    latency {
        operation settle: p99 < 100ms
        operation validate: p99 < 10ms
    }
    
    throughput {
        system: > 10_000 tps
        subsystem Processing: > 15_000 tps
    }
    
    resources {
        memory: < 4GB per_instance
        cpu: < 2 cores per_instance
    }
}

constraint budget {
    category: non_functional
    
    infrastructure {
        monthly: < $10_000
        per_transaction: < $0.001
    }
}
```

### 12.7 Stage-Scoped Constraints

```intent
constraint layering {
    stage alpha {
        [services] may depend_on [storage]
    }
    
    stage beta {
        [services] must_not depend_on [storage]
        [services] must depend_on [StorageCoordinator]
    }
}
```

---

## 13. Progression Declaration

Progression defines implementation stages with scoped verification.

```intent
system PaymentPlatform {
    progression {
        stage alpha {
            scope: [Ingestion, Processing]
            constraints: [architecture]
            target: "Single-tenant MVP"
        }
        
        stage beta {
            extends: alpha
            scope: [Ingestion, Processing, Settlement]
            constraints: [architecture, performance]
            target: "Multi-tenant, monitored"
        }
        
        stage ga {
            scope: all
            constraints: all
            target: "Full SLA compliance"
        }
    }
    
    current_stage: beta
}
```

---

## 14. Apply Pattern

Apply a pattern to a specific context.

```intent
apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
    to StorageCoordinator.dgraph_circuit_breaker {
        refines "formal/tla/CircuitBreaker.tla"
    }
```

### 14.1 Parameters

| Type | Syntax | Example |
|------|--------|---------|
| Integer | `N` | `threshold: 5` |
| Duration | `Ns` | `timeout: 30s` |
| String | `"..."` | `name: "dgraph"` |
| Float | `N.N` | `rate: 0.03` |

---

## 15. Distillation

### 15.1 Distilled Patterns

Distilled patterns capture reusable behaviors extracted from implementation. The `commit` field is **required**.

```intent
distilled pattern RetryWithBackoff {
    source: "crates/client/src/*.rs"
    commit: "a1b2c3d"              // required: commit hash
    extracted: "2026-02-15"

    observation {
        "Exponential backoff emerged in all client implementations."
    }

    parameters {
        max_retries: Int { default: 3 }
        initial_delay: Duration { default: 100ms }
        backoff_factor: Float { default: 2.0 }
    }

    behavior {
        states [attempting, waiting, succeeded, exhausted]
        initial attempting
        terminal [succeeded, exhausted]

        transitions {
            attempting -> succeeded on success
            attempting -> waiting on failure where { retries < max_retries }
            attempting -> exhausted on failure where { retries >= max_retries }
            waiting -> attempting after { initial_delay * backoff_factor^retries }
        }
    }

    applies_to {
        *Client.call
        *Gateway.invoke
    }
}
```

### 15.2 Distillation Markers

```intent
concern CircuitBreaking {
    distilled from "crates/storage/src/coordinator.rs" {
        commit: "abc123"
        observation: "Circuit breaker emerged in error handling"
    }

    apply CircuitBreaker(threshold: 5, timeout: 30s)
        to StorageCoordinator.dgraph
}
```

### 15.3 Insights

```intent
insight LatentCoupling {
    discovered: "2026-02-10"
    source: "Code review"
    
    observation {
        "Services A and B both use Cache but invalidate inconsistently."
    }
    
    recommendation {
        constraint cache_discipline {
            [ServiceA, ServiceB] must depend_on [CacheInvalidator]
        }
    }
    
    status: proposed
}
```

---

## 16. Rationale Annotations

### 16.1 Decided Because

```intent
decided because {
    "Dgraph and Milvus are external dependencies with independent failure modes."
    "Circuit breakers prevent cascading failures."
}
```

### 16.2 Rejected Alternatives

```intent
rejected alternatives {
    retry_only: "Retries without circuit breaking cause request pileup."
    failover: "Neither Dgraph nor Milvus runs replicas."
}
```

### 16.3 Revisit When

```intent
revisit when {
    "Dgraph runs in replicated HA configuration"
    "A third storage backend is added"
}
```

---

## 17. Deployment

```intent
deployment Production {
    platform: kubernetes
    
    mapping {
        Ingestion -> "ingestion" { replicas: 3, cpu: "500m", memory: "1Gi" }
        Processing -> "processing" { replicas: 5, cpu: "1", memory: "2Gi" }
    }
    
    dependencies {
        postgres: "postgres.db.svc:5432"
        redis: "redis.cache.svc:6379"
    }
}
```

---

## 18. Pipeline

```intent
pipeline CI {
    stages {
        lint {
            runs: ["cargo clippy", "cargo fmt --check"]
            gate: must_pass
        }
        
        intent_check {
            runs: ["intent check formal/intent/ --codebase src/"]
            gate: must_pass
        }
        
        test {
            runs: ["cargo test"]
            gate: must_pass
        }
        
        model_check {
            runs: ["apalache-mc check formal/tla/*.tla"]
            gate: must_pass
            timeout: 30m
        }
    }
    
    triggers {
        pull_request: [lint, intent_check, test]
        merge: all
        nightly: [model_check]
    }
}
```

---

## 19. Tooling

```intent
tooling {
    language rust { edition: 2024 }
    framework: axum
    
    storage {
        primary: postgres { version: ">= 15" }
        cache: redis { version: ">= 7" }
    }
    
    formal {
        spec: tla_plus
        checker: apalache
        mbt: quint
    }
    
    decided because {
        "Rust for performance-critical processing."
        "TLA+ for proven formal verification."
    }
}
```

---

## 20. Formal Grammar (EBNF)

```ebnf
(* ═══════════════════════════════════════════════════════════════════════════
   TOP LEVEL
   ═══════════════════════════════════════════════════════════════════════════ *)

File          = { TopLevel } ;
TopLevel      = System | Concern | Deployment | Pipeline | Tooling
              | DistilledPattern | Insight | Pattern ;

(* ═══════════════════════════════════════════════════════════════════════════
   SYSTEM
   ═══════════════════════════════════════════════════════════════════════════ *)

System        = "system" IDENT [ "refines" IDENT ] "{" { SystemItem } "}" ;
SystemItem    = Maturity | Description | Parent | SubsystemsDecl
              | Model | Interface | Adapter | Behavior | Constraint | Scope
              | Implements | Progression | CurrentStage
              | RefinementMap | ActionMap | Strengthens
              | Rationale | Apply | Let | Predicate ;

Maturity      = "maturity" ":" ( "sketch" | "draft" | "spec" | "final" ) ;
Description   = "description" STRING ;
Parent        = "parent" ":" IDENT ;
SubsystemsDecl = "subsystems" IdentList ;
Implements    = "implements" STRING ;
CurrentStage  = "current_stage" ":" IDENT ;

RefinementMap = "refinement_map" "{" { RefinementEntry } "}" ;
RefinementEntry = DottedName "->" IdentList ;
ActionMap     = "action_map" "{" { ActionEntry } "}" ;
ActionEntry   = DottedName "->" IdentList ;
Strengthens   = "strengthens" DottedName "with" IDENT ;

(* ═══════════════════════════════════════════════════════════════════════════
   CONCERN (v0.2 compatible)
   ═══════════════════════════════════════════════════════════════════════════ *)

Concern       = "concern" IDENT "{" { ConcernItem } "}" ;
ConcernItem   = Scope | Constraint | Layer | Apply | Rationale
              | UseScope | Let | Predicate | Parameter | Invariant
              | Behavior | Model | Interface | Adapter | Distilled ;

(* ═══════════════════════════════════════════════════════════════════════════
   MODEL
   ═══════════════════════════════════════════════════════════════════════════ *)

Model         = "model" IDENT "{" { ModelItem } "}" ;
ModelItem     = Fields | ModelInvariant | Enum | Derived ;

Fields        = "fields" "{" { FieldDecl } "}" ;
FieldDecl     = IDENT ":" TypeExpr [ FieldConstraints ] ;
TypeExpr      = IDENT [ "?" ] ;
FieldConstraints = "{" { FieldConstraint } "}" ;
FieldConstraint = "min" ":" Value | "max" ":" Value
                | "pattern" ":" STRING | "default" ":" Value ;

Enum          = "enum" IDENT "{" IDENT { "," IDENT } "}" ;
Derived       = "derived" IDENT "{" Expr "}" ;
ModelInvariant = "invariant" IDENT "{" Expr "}" ;

(* ═══════════════════════════════════════════════════════════════════════════
   INTERFACE (per-module contracts)
   ═══════════════════════════════════════════════════════════════════════════ *)

Interface     = "interface" IDENT [ "extends" IDENT ] "{" { InterfaceItem } "}" ;
InterfaceItem = Owner | Maturity | Operation | Protocol | InterfaceInvariant ;

Owner         = "owner" ":" IDENT ;

Operation     = "operation" IDENT "(" [ Params ] ")" "->" TypeExpr
                [ "{" { OpClause } "}" ] ;
OpClause      = "requires" "{" Expr "}" | "ensures" "{" Expr "}" ;

Protocol      = "protocol" IDENT "{" ProtocolExpr "}" ;
ProtocolExpr  = IDENT { "->" IDENT }
              | "(" ProtocolExpr ")" ( "*" | "?" )
              | ProtocolExpr "|" ProtocolExpr ;

InterfaceInvariant = "invariant" IDENT "{" Expr "}" ;

(* ═══════════════════════════════════════════════════════════════════════════
   ADAPTER (connects interfaces)
   ═══════════════════════════════════════════════════════════════════════════ *)

Adapter       = "adapter" IDENT "{" { AdapterItem } "}" ;
AdapterItem   = Connects | AdapterMapping | Transforms | ErrorHandling ;

Connects      = "connects" ":" DottedName "->" ( IDENT | IdentList ) ;
AdapterMapping = "mapping" "{" { DottedName "->" DottedName } "}" ;
Transforms    = "transforms" "{" { Expr } "}" ;
ErrorHandling = "error_handling" "{" { DottedName "->" DottedName } "}" ;

(* ═══════════════════════════════════════════════════════════════════════════
   BEHAVIOR (state machines with events and effects)
   ═══════════════════════════════════════════════════════════════════════════ *)

Behavior      = "behavior" IDENT [ "composes" IdentList ] "{" { BehaviorItem } "}" ;
BehaviorItem  = EventChannels | EventSource | StatesDecl | TransitionsDecl
              | CommandDecl | Property | Fairness | BehaviorInvariant
              | RefinesClause | AppliesPattern ;

EventChannels = ( "subscribes" | "emits" | "handles" | "publishes" ) IdentList ;
EventSource   = "event_sourced" "true" [ "stream" STRING ]
              | "events" "{" { EventDecl } "}" ;
EventDecl     = IDENT "{" { IDENT ":" TypeExpr } "}" ;

StatesDecl    = "states" ( "{" { StateDecl } "}" | "[" StateList "]" ) ;
StateList     = IDENT [ "(" IDENT ":" TypeExpr ")" ] { "," StateListElem } ;
StateListElem = IDENT [ "(" IDENT ":" TypeExpr ")" ] ;
StateDecl     = IDENT [ "(" IDENT ":" TypeExpr ")" ] [ "{" { StateModifier } "}" ] ;
StateModifier = "initial" ":" "true" | "terminal" ":" "true" ;

DerivedState  = "state" IDENT "{" { DerivedStateItem } "}" ;
DerivedStateItem = "initial" ":" Value | "on" IDENT ":" Expr ;

TransitionsDecl = "transitions" "{" { TransitionDecl } "}" ;
TransitionDecl  = IDENT [ "(" Expr ")" ] "->" IDENT [ "(" Expr ")" ] "on" IDENT
                  [ "where" "{" Expr "}" ]
                  [ "effect" "{" { EffectStmt } "}" ]
                  [ ( "within" | "after" ) "{" Expr "}" ] ;

EffectStmt    = "emit" IDENT [ "(" [ Expr { "," Expr } ] ")" ]
              | "if" Expr "{" { EffectStmt } "}" [ "else" "{" { EffectStmt } "}" ]
              | Expr ;

CommandDecl   = "command" IDENT "(" [ ParamDecl { "," ParamDecl } ] ")" "{" { CommandItem } "}" ;
CommandItem   = "guard" "{" Expr "}" | "emits" IdentList | TransitionDecl ;

Property      = "property" IDENT "{" TemporalExpr "}" ;
TemporalExpr  = "always" "(" Expr [ "=>" "eventually" "(" Expr ")" ] ")"
              | "eventually" "(" Expr ")"
              | "was" "(" IDENT ")"
              | Expr ;

Fairness      = "fairness" "{" { FairnessSpec } "}" ;
FairnessSpec  = ( "weak" | "strong" ) "(" IDENT "->" IDENT [ "|" IDENT ] ")" ;

BehaviorInvariant = "invariant" IDENT "{" Expr "}" ;

AppliesPattern = "applies" IDENT [ TypeArgs ] "{" { PatternArg } "}" ;
TypeArgs      = "<" IDENT { "," IDENT } ">" ;
PatternArg    = IDENT ":" ( Value | IdentList | MappingLiteral ) ;
MappingLiteral = "{" { IDENT "->" IDENT [ "," ] } "}" ;

(* ═══════════════════════════════════════════════════════════════════════════
   PATTERN (reusable parameterized behaviors)
   ═══════════════════════════════════════════════════════════════════════════ *)

Pattern       = "pattern" IDENT [ TypeParams ] "{" { PatternItem } "}" ;
TypeParams    = "<" IDENT { "," IDENT } ">" ;
PatternItem   = PatternParams | Behavior ;

PatternParams = "parameters" "{" { ParamDecl } "}" ;
ParamDecl     = IDENT ":" TypeExpr [ "{" { FieldConstraint } "}" ] ;

(* ═══════════════════════════════════════════════════════════════════════════
   CONSTRAINT
   ═══════════════════════════════════════════════════════════════════════════ *)

Constraint    = "constraint" IDENT "{" { ConstraintItem } "}" ;
ConstraintItem = ConstraintRule | Category | ConstraintStage | Layer
               | LatencySpec | ThroughputSpec | ResourceSpec | BudgetSpec ;

Category      = "category" ":" IDENT ;
ConstraintStage = "stage" IDENT "{" { ConstraintRule } "}" ;
Layer         = "layer" IDENT "{" EntityRef "}" ;

LatencySpec   = "latency" "{" { "operation" IDENT ":" Percentile "<" DURATION } "}" ;
Percentile    = "p50" | "p90" | "p99" | "p999" ;

ThroughputSpec = "throughput" "{" { ThroughputEntry } "}" ;
ThroughputEntry = ( "system" | "subsystem" IDENT ) ":" ">" INT IDENT ;

ResourceSpec  = "resources" "{" { IDENT ":" "<" Value IDENT } "}" ;
BudgetSpec    = "infrastructure" "{" { IDENT ":" "<" Value } "}" ;

ConstraintRule = EntityRef "must_not" "depend_on" EntityName
               | EntityRef "must_not" "reference" EntityRef
               | EntityRef "must_depend_on" EntityName
               | EntityRef "must_reference" EntityRef
               | EntityName "occur_only_in" EntityRef
               | IDENT "must_implement" IDENT
               | "forall" IDENT "in" ScopeExpr ":" ConstraintRule
               | "forall" IDENT "in" ScopeExpr "{" { ConstraintRule } "}"
               | "exists" IDENT "in" ScopeExpr ":" ConstraintRule
               | "exists" IDENT "in" ScopeExpr "{" { ConstraintRule } "}"
               | IDENT "depends_on" EntityName "=>" ConstraintRule
               | IDENT "references" EntityName "=>" ConstraintRule
               | IDENT "(" ScopeExpr { "," ScopeExpr } ")" ;

(* ═══════════════════════════════════════════════════════════════════════════
   PROGRESSION
   ═══════════════════════════════════════════════════════════════════════════ *)

Progression   = "progression" "{" { Stage } "}" ;
Stage         = "stage" IDENT "{" { StageItem } "}" ;
StageItem     = "scope" ":" ScopeExpr
              | "extends" ":" IDENT
              | "constraints" ":" ( "all" | IdentList )
              | "behaviors" ":" ( "all" | BehaviorRefList )
              | "target" ":" STRING ;

BehaviorRefList = "[" BehaviorRef { "," BehaviorRef } "]" ;
BehaviorRef   = IDENT [ "(" "subset" ":" IdentList ")" ] ;

(* ═══════════════════════════════════════════════════════════════════════════
   DISTILLATION (requires commit hash)
   ═══════════════════════════════════════════════════════════════════════════ *)

DistilledPattern = "distilled" "pattern" IDENT "{" { DistilledItem } "}" ;
DistilledItem = "source" ":" STRING | "commit" ":" STRING | "extracted" ":" STRING
              | "observation" "{" STRING "}" | PatternParams
              | Behavior | AppliesTo ;

AppliesTo     = "applies_to" "{" { GlobPattern } "}" ;

Distilled     = "distilled" "from" STRING "{" { DistillMeta } "}" ;
DistillMeta   = "commit" ":" STRING | "observation" ":" STRING ;

Insight       = "insight" IDENT "{" { InsightItem } "}" ;
InsightItem   = "discovered" ":" STRING | "source" ":" STRING
              | "observation" "{" STRING "}"
              | "recommendation" "{" { ConcernItem } "}"
              | "status" ":" ( "proposed" | "accepted" | "rejected" ) ;

(* ═══════════════════════════════════════════════════════════════════════════
   DEPLOYMENT, PIPELINE, TOOLING
   ═══════════════════════════════════════════════════════════════════════════ *)

Deployment    = "deployment" IDENT "{" { DeployItem } "}" ;
DeployItem    = "platform" ":" IDENT
              | "mapping" "{" { IDENT "->" STRING [ "{" { KV } "}" ] } "}"
              | "dependencies" "{" { IDENT ":" STRING } "}"
              | Constraint ;

Pipeline      = "pipeline" IDENT "{" { PipelineItem } "}" ;
PipelineItem  = "stages" "{" { PipelineStage } "}"
              | "triggers" "{" { IDENT ":" ( "all" | IdentList ) } "}" ;
PipelineStage = IDENT "{" { "runs" ":" StringList | "gate" ":" IDENT 
                          | "timeout" ":" DURATION } "}" ;

Tooling       = "tooling" "{" { ToolingItem } "}" ;
ToolingItem   = "language" IDENT [ "{" { KV } "}" ]
              | "framework" ":" IDENT
              | "storage" "{" { IDENT ":" IDENT [ "{" { KV } "}" ] } "}"
              | "formal" "{" { KV } "}"
              | Rationale ;

(* ═══════════════════════════════════════════════════════════════════════════
   SUPPORTING CONSTRUCTS
   ═══════════════════════════════════════════════════════════════════════════ *)

Scope         = "scope" IDENT "{" ScopeBody [ "within" IdentList ] "}" ;
ScopeBody     = "only" IdentList "accesses" IDENT | IdentList ;

Let           = "let" IDENT "=" ScopeExpr ;
Predicate     = "predicate" IDENT "(" IDENT { "," IDENT } ")" "{" { ConstraintRule } "}" ;
Apply         = "apply" IDENT Params "to" DottedName [ "{" "refines" STRING "}" ] ;
Parameter     = "parameter" IDENT ":" Value ;
Invariant     = "invariant" IDENT "{" { InvariantExpr } "}" ;
UseScope      = "use" IDENT "." IDENT ;
RefinesClause = "refines" STRING ;

Rationale     = DecidedBecause | RejectedAlternatives | RevisitWhen ;
DecidedBecause = "decided" "because" "{" { STRING } "}" ;
RejectedAlternatives = "rejected" "alternatives" "{" { IDENT ":" STRING } "}" ;
RevisitWhen   = "revisit" "when" "{" { STRING } "}" ;

(* ═══════════════════════════════════════════════════════════════════════════
   EXPRESSIONS
   ═══════════════════════════════════════════════════════════════════════════ *)

ScopeExpr     = SetUnion ;
SetUnion      = SetIntersect { "|" SetIntersect } ;
SetIntersect  = SetDiff { "&" SetDiff } ;
SetDiff       = SetPrimary { "\" SetPrimary } ;
SetPrimary    = "[" EntityName { "," EntityName } "]"
              | "{" IDENT "|" IDENT "matches" Pattern "}"
              | IDENT | PREFIX_GLOB | SUFFIX_GLOB
              | "(" ScopeExpr ")" | "all" ;

Expr          = OrExpr ;
OrExpr        = AndExpr { "||" AndExpr } ;
AndExpr       = CompExpr { "&&" CompExpr } ;
CompExpr      = AddExpr [ CompOp AddExpr ] ;
AddExpr       = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr       = UnaryExpr { ( "*" | "/" ) UnaryExpr } ;
UnaryExpr     = "!" UnaryExpr | "-" UnaryExpr | Primary ;
Primary       = Value | DottedName | "(" Expr ")"
              | "forall" IDENT "in" ScopeExpr ":" Expr
              | "exists" IDENT "in" ScopeExpr ":" Expr
              | IDENT "(" [ Expr { "," Expr } ] ")" ;

CompOp        = "==" | "!=" | "<" | "<=" | ">" | ">=" ;

EntityRef     = "[" EntityName { "," EntityName } "]" | IDENT | PREFIX_GLOB | SUFFIX_GLOB ;
EntityName    = IDENT | PREFIX_GLOB | SUFFIX_GLOB ;
IdentList     = "[" IDENT { "," IDENT } "]" ;
StringList    = "[" STRING { "," STRING } "]" ;
DottedName    = IDENT { "." IDENT } ;
GlobPattern   = DottedName [ "." ( "*" | IDENT ) ] ;

Params        = "(" Param { "," Param } ")" ;
Param         = IDENT ":" Value ;
KV            = IDENT ":" Value ;
Value         = INT | FLOAT | PERCENT | DURATION | STRING | "true" | "false" ;
Pattern       = IDENT | PREFIX_GLOB | SUFFIX_GLOB ;
InvariantExpr = Expr [ "," ] ;

(* ═══════════════════════════════════════════════════════════════════════════
   TERMINALS
   ═══════════════════════════════════════════════════════════════════════════ *)

IDENT         = /[a-zA-Z_][a-zA-Z0-9_]*/ ;
PREFIX_GLOB   = /\*[a-zA-Z0-9_]+/ ;
SUFFIX_GLOB   = /[a-zA-Z_][a-zA-Z0-9_]*\*/ ;
INT           = /[0-9]+(_[0-9]+)*/ ;
FLOAT         = /[0-9]+\.[0-9]+/ ;
PERCENT       = /[0-9]+(\.[0-9]+)?%/ ;
DURATION      = /[0-9]+[μsmhd]/ ;
STRING        = /"[^"]*"/ ;
COMMENT       = /\/\/[^\n]*/ | /\/\*.*?\*\// ;
```

---

## 21. Semantic Rules

### 21.1 Name Resolution

1. **Scope names** resolve within current system/concern, plus `use`-imported scopes
2. **Entity names** resolve as scope first, then literal entity
3. **Glob patterns** expand via regex: `*Foo` → `^.*Foo$`, `Foo*` → `^Foo.*$`

### 21.2 Layer Ordering

Layers declared top-to-bottom. For layers L₁...Lₙ, implicit constraint: `Lⱼ must_not depend_on Lᵢ` for all i < j.

### 21.3 Stage Scoping

When `current_stage` is set, only constraints matching that stage (or unscoped) are evaluated.

### 21.4 Refinement Verification

`system A refines B` generates obligations that A's behaviors satisfy B's properties with state mapping applied.

### 21.5 Interface-Adapter Consistency

1. Adapter's `connects` source must reference a valid module port
2. Adapter's `connects` target must reference a declared interface
3. All operations in mapping must exist on both sides

### 21.6 Pattern Instantiation

1. `applies Pattern { params }` must provide all required parameters
2. Pattern type parameters must be satisfied by concrete types
3. Nested patterns (`Pattern<A< B>>`) resolve innermost-first

### 21.7 Distillation Traceability

1. `distilled pattern` must include `commit` field
2. `distilled from` must include `commit` field
3. Commit hash must be valid in the repository

---

## 22. CLI Usage

```bash
# Full verification
intent check intent/ --codebase src/

# Structural only
intent structural intent/ --codebase src/

# Compile to TLA+
intent compile intent/ --output formal/generated/

# Verify with Apalache
intent verify --tla formal/generated/

# Plan mode (no codebase)
intent plan intent/

# Show progression status
intent progress intent/

# Extract rationale
intent rationale intent/ --output rationale.json

# JSON output
intent check intent/ --codebase src/ --format json
```

---

## 23. Backward Compatibility

| v0.2 Construct | v0.3 Status |
|---------------|-------------|
| `concern C { ... }` | Unchanged |
| `scope`, `constraint`, `layer` | Unchanged |
| `statemachine` | Deprecated alias for `behavior` |
| `parameter`, `invariant` | Unchanged |
| `apply...refines` | Unchanged |
| `forall`, `exists`, `predicate` | Unchanged |
| Set algebra (`\|`, `&`, `\`) | Unchanged |
| `interface A: X -> Y` | Deprecated; use `interface A { owner: X }` + `adapter` |

**New v0.3 constructs:**
| Construct | Purpose |
|-----------|---------|
| `interface { owner }` | Per-module interface declaration |
| `adapter` | Connects interfaces (many-to-many) |
| `behavior` with effects | Event-driven behaviors with `subscribes`, `emits`, `effect` |
| `pattern` | Reusable parameterized behaviors |
| `applies` | Pattern instantiation in behaviors |
| `distilled pattern` with `commit` | Requires commit hash for traceability |

New constructs are additive. All v0.2 files parse without modification (except deprecated interface syntax).
