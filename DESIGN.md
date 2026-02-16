# Intent Language Design Document

**Status:** Living document
**Version:** 0.3.0
**Updated:** 2026-02-16

---

## Introduction

The goal is for AI to generate **known good code** — code that is mathematically proven correct via SMT solvers. A critical gap exists: currently, specification is not formal. Prose documents cannot be machine-verified, and there is no continuous chain of proof from intent to implementation. Intent closes this gap by providing a formal specification language that bridges human intent and machine-verifiable proofs.

---

## 1. Problem Statement

In spec-driven agentic coding workflows, an AI agent writes code guided by specifications and verified by formal models. The verification stack has a gap between prose specifications and executable formal models:

| Layer | Captures | Verified by |
|-------|----------|-------------|
| Prose specs (`spec/*.md`) | Requirements, architecture, rationale | Human review only |
| **??? (Gap)** | **Architectural constraints, pattern conformance, design decisions** | **Nothing** |
| TLA+ models (`formal/`) | Component state machine behavior | Apalache model checking |
| Formal verification | Temporal properties, invariants | SMT solvers |
| Model-based testing | State space exploration | Test generation from specs |
| Tests (`tests/`) | Specific scenarios | CI execution |

**Intent** closes this gap with a machine-verifiable language for architectural design constraints that captures the **entire specification lifecycle**:

```
 Ideation
    │
    ▼  (refinement)
 System Spec ◄───────────────────────────────┐
    │                     (simulation+fix)   │
    │                                        │
    ├── Subsystem A ──(transpile)──► TLA+ ───┼──► Implementation
    ├── Subsystem B ──(transpile)──► TLA+ ───┤         │
    └── Subsystem C ──(transpile)──► TLA+ ───┘         │
    ▲                                                  │
    └──────────────────────────── (distillation) ◄─────┘

```

The feedback loop: distillation captures patterns from implementation and feeds them back into the system specification, enabling iterative refinement.

Two fundamental evolutionary mechanisms:

1. **Refinement** — decomposing abstract intent into concrete subsystem specs
2. **Distillation** — capturing emergent and latent patterns from implementation back into spec

**Living specification:** The intent spec is a living document meant to be frequently modified, with TLA+ re-generated as needed. LLM code generation is negligible in cost, so the system is designed to be flexible — code is cheaply generated and thrown away if needed, just as a traditional compiler treats compiled code. It is even desirable during the spec phase to generate throwaway implementations for distillation and benchmarking purposes only. The specification evolves iteratively; nothing is precious.

---

## 2. Design Principles

### 2.1 Three verification backends, one language

Intent constraints are **structural** (verified by static analysis), **behavioral** (compiled to TLA+ proof obligations), or **descriptive** (evaluated by LLM). The language unifies all three; the compiler routes each constraint to the appropriate backend.

### 2.2 Generate obligations, not models

The behavioral compiler generates **theorems** that existing hand-written TLA+ specs must satisfy. Component-level models remain hand-written. Intent asserts cross-cutting properties over them.

### 2.3 Rationale is metadata, not a layer

Design rationale (the "why") is attached as annotations. It is consumed by agents for decision-making context and by drift monitors for invalidation detection. It does not introduce a separate verification boundary.

### 2.4 Grounding is explicit

The mapping from architectural concepts to code entities is declared, not inferred. No implicit conventions. No LLM-assisted guessing in the verification path.

### 2.5 Incremental specification

Intent specs are added per-concern or per-system, not as a monolithic description. Each file is independently parseable and verifiable.

### 2.6 Friction-minimal

Every element of the language must pay for itself in verification value. No ceremony, no boilerplate. If a constraint cannot be verified, it belongs in a prose spec, not in Intent.

### 2.7 Compositionality

Set algebra over scopes, quantified constraints, parameterized predicates, let bindings, and hierarchical systems. Each feature composes with the others.

### 2.8 Lifecycle-aware (v0.3)

Specifications evolve through maturity levels (`sketch` → `draft` → `spec` → `final`) and implementation stages (`alpha` → `beta` → `ga`). Verification is scoped to current maturity and stage.

### 2.9 Bidirectional (v0.3)

Design flows down (refinement) and implementation insights flow up (distillation). The language supports both directions explicitly.

---

## 3. Core Abstractions

Intent v0.3 is built on **6 core abstractions**:

| Abstraction | Purpose | Transpiles to |
|-------------|---------|---------------|
| **System** | Hierarchical decomposition | Module structure |
| **Model** | Data schema & invariants | Type specs + TLA+ state |
| **Interface** | Contract exposed by a module | API specs + refinement maps |
| **Adapter** | Connects interfaces (many-to-many) | Integration glue + transformations |
| **Behavior** | State machines, events, effects | TLA+ specs + event schemas |
| **Pattern** | Reusable parameterized behaviors | TLA+ templates |
| **Constraint** | Cross-cutting properties | Static checks + TLA+ obligations |

Each abstraction can exist at different **maturity levels**:
- `sketch` — ideation, incomplete, no verification
- `draft` — structured but unverified
- `spec` — verifiable, may have `todo` items
- `final` — complete, fully verified

---

## 4. Position in the Verification Hierarchy

```
Prose Specs (spec/*.md)           Human-readable, authoritative, not machine-checkable
       │
       │  formalized by
       ▼
Intent Specs (intent/*.intent)    Machine-checkable design constraints
       │
       ├── structural ──────────► static analysis (Rust connector via syn)
       ├── behavioral ──────────► TLA+ proof obligations
       └── non-functional ──────► benchmark assertions, CI gates
       │
       ▼
Formal Models (formal/tla/*.tla)  Component-level state machines, verified by Apalache
       │
       │  MBT tests
       ▼
Implementation (crates/, src/)    Rust/TypeScript code under contract
       │
       │  distillation (patterns learned)
       ▼
Intent Specs (updated)            Captured patterns, insights
```

---

## 5. System Hierarchy & Refinement

### 5.1 System Declaration

```intent
system PaymentPlatform {
    maturity: spec

    description "Multi-tenant payment processing with reconciliation"

    subsystems [Ingestion, Processing, Settlement, Reporting]

    model Transaction { ... }
    model Account { ... }

    interface ProcessingQueue {
        owner: Processing
        // ...
    }

    adapter IngestionAdapter {
        connects: Ingestion.Outbound -> ProcessingQueue
    }

    invariant total_balance {
        sum(accounts.balance) == sum(transactions.settled_amount)
    }
}
```

**Implicit conventions:**
- `formal` — defaults to `formal/tla/{SystemName}.tla` if file exists
- `refinement_map` — defaults to `formal/tla/{SystemName}.map.yaml` if file exists
- Only declare explicitly to override the convention

Explicit override example:
```intent
system PaymentPlatform {
    formal "specs/abstract/payment_flow.tla"  // override: non-standard path

    refinement_map {
        abstract.pending -> [Ingestion.queued, Processing.validating]
        abstract.complete -> [Settlement.settled, Reporting.recorded]
    }
}
```

### 5.2 Subsystem Definition

```intent
system Processing {
    maturity: spec
    parent: PaymentPlatform

    model ValidationResult { ... }
    behavior TransactionLifecycle { ... }

    implements "crates/processing/src"

    constraint single_settlement {
        forall t in Transaction: t.settled_count <= 1
    }
}
```

### 5.3 Refinement Operators

```intent
system Concrete refines Abstract {
    // refinement_map loaded from formal/tla/Concrete.map.yaml if exists

    action_map {
        Abstract.process -> [Concrete.validate, Concrete.execute, Concrete.commit]
    }

    strengthens Abstract.safety with local_safety
}
```

---

## 6. Model — Data Schemas with Invariants

```intent
model Transaction {
    fields {
        id: UUID
        amount: Decimal { min: 0.01, max: 1_000_000 }
        currency: Currency
        status: Status
        created_at: Timestamp
        settled_at: Timestamp?
    }

    enum Status { pending, processing, settled, failed, reversed }

    invariant settlement_order { settled_at != null => settled_at >= created_at }
    invariant created_after_migration { created_at >= "2024-01-15" }

    derived age { now() - created_at }
}
```

**Transpiles to TLA+:**
```tla
TypeOK ==
    /\ amount \in Decimal
    /\ amount >= 0.01
    /\ amount <= 1_000_000
    /\ status \in {"pending", "processing", "settled", "failed", "reversed"}

SettlementOrder ==
    settled_at /= NULL => settled_at >= created_at

CreatedAfterMigration ==
    created_at >= "2024-01-15"
```

---

## 7. Interface — Module Contracts

Interfaces are defined per module, describing what that module exposes. Adapters connect interfaces, enabling many-to-many relationships between modules.

### 7.1 Interface Declaration

An interface is owned by a single module and defines its contract:

```intent
interface SettlementAPI {
    maturity: spec
    owner: Settlement

    operation settle(batch: Batch) -> Result<SettlementId, Error> {
        requires { batch.transactions.all(t => t.status == processing) }
        ensures { result.ok => batch.transactions.all(t => t.status == settled) }
        ensures { result.err => batch.transactions.all(t => t.status == failed) }
    }

    operation get_status(id: SettlementId) -> Option<SettlementStatus>

    invariant idempotent {
        forall b in Batch: settle(b).ok => settle(b) == settle(b)
    }

    protocol normal_flow {
        validate -> settle -> confirm
    }
}
```

### 7.2 Adapters — Connecting Interfaces

Adapters bridge interfaces, enabling many-to-many module relationships:

```intent
adapter SettlementAdapter {
    connects: Processing SettlementPort -> SettlementAPI

    mapping {
        Processing.batch_request -> SettlementAPI.settle
        Processing.status_query -> SettlementAPI.get_status
    }

    transforms {
        // Convert Processing's Batch to Settlement's Batch format
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

### 7.3 Many-to-Many Relationships

A single interface can be adapted by multiple consumers, and a single module can consume multiple interfaces:

```intent
// Multiple consumers of SettlementAPI
adapter IngestionSettlement {
    connects: Ingestion.SinkPort -> SettlementAPI
}

adapter ReportingSettlement {
    connects: Reporting.QueryPort -> SettlementAPI
}

// Single module consuming multiple interfaces
adapter OrchestrationAdapter {
    connects: Processing.Outbound -> [SettlementAPI, NotificationAPI, AuditAPI]
}
```

### 7.4 Interface Inheritance

Interfaces can extend other interfaces:

```intent
interface AsyncSettlementAPI extends SettlementAPI {
    operation settle_async(batch: Batch) -> SettlementId

    ensures { result -> settle_async_callback within 30s }
}
```

---

## 8. Behavior — State Machines & Temporal Properties

```intent
behavior TransactionLifecycle {
    maturity: spec

    states {
        pending     { initial: true }
        validating
        processing
        settling
        settled     { terminal: true }
        failed      { terminal: true }
    }

    transitions {
        pending -> validating     on receive
        validating -> processing  on valid        where { amount <= limit }
        validating -> failed      on invalid
        processing -> settling    on approved
        settling -> settled       on confirmed
        settling -> failed        on timeout
        settled -> reversed       on reversal     within { 24h }
    }

    property eventual_settlement {
        always(pending => eventually(settled | failed))
    }

    property no_resurrection {
        always(failed => always(!settled))
    }

    fairness { weak(validating -> processing | failed) }

    formal "formal/tla/TransactionFlow.tla"
}
```

**Transpiles to TLA+:**
```tla
EventualSettlement == [](state = "pending" => <>(state = "settled" \/ state = "failed"))
NoResurrection == [](state = "failed" => [](state /= "settled"))
```

### 8.1 Event-Driven Behaviors with Effects

For event-driven systems, behaviors include **effects** — side effects produced during transitions:

```intent
behavior OrderProcessor {
    maturity: spec

    // Event channels this behavior subscribes to
    subscribes [OrderCreated, PaymentCompleted, InventoryReserved]

    // Commands this behavior emits
    emits [ReserveInventory, ProcessPayment, ShipOrder, NotifyCustomer]

    states {
        idle        { initial: true }
        reserving
        charging
        shipping
        completed   { terminal: true }
        cancelled   { terminal: true }
    }

    transitions {
        idle -> reserving on OrderCreated
            effect { emit ReserveInventory(order_id, items) }

        reserving -> charging on InventoryReserved
            effect { emit ProcessPayment(order_id, total) }

        reserving -> cancelled on InventoryFailed
            effect { emit NotifyCustomer(order_id, "out_of_stock") }

        charging -> shipping on PaymentCompleted
            effect { emit ShipOrder(order_id, address) }

        charging -> cancelled on PaymentFailed
            effect {
                emit RefundInventory(order_id)
                emit NotifyCustomer(order_id, "payment_failed")
            }

        shipping -> completed on OrderShipped
            effect { emit NotifyCustomer(order_id, "shipped") }
    }

    // Effects can have constraints
    invariant unique_emission {
        forall e in emitted: e.correlation_id == current.order_id
    }
}
```

### 8.2 Command vs Event Distinction

Commands represent intent; events represent facts:

```intent
behavior PaymentHandler {
    // Commands: handled by this behavior
    handles [ProcessPayment, RefundPayment]

    // Events: published as outcomes
    publishes [PaymentCompleted, PaymentFailed, RefundProcessed]

    states { idle, processing, completed, refunding }

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

### 8.3 Event Sourcing Patterns

For event-sourced aggregates, behaviors track state evolution through events:

```intent
behavior AccountAggregate {
    event_sourced true
    stream "accounts/{account_id}"

    events {
        AccountOpened { account_id: UUID, owner: String }
        MoneyDeposited { account_id: UUID, amount: Decimal }
        MoneyWithdrawn { account_id: UUID, amount: Decimal }
        AccountClosed { account_id: UUID }
    }

    // State is derived from event history
    state balance {
        initial: 0
        on MoneyDeposited: + event.amount
        on MoneyWithdrawn: - event.amount
    }

    command OpenAccount(owner: String) {
        guard { not exists }
        emits [AccountOpened]
    }

    command Deposit(amount: Decimal) {
        guard { amount > 0 }
        emits [MoneyDeposited]
    }

    command Withdraw(amount: Decimal) {
        guard { balance >= amount }  // refers to derived state
        emits [MoneyWithdrawn]
    }

    invariant positive_balance {
        balance >= 0
    }
}
```

### 8.4 Patterns — Reusable Parameterized Behaviors

Instead of hardcoding specific patterns (saga, circuit breaker, retry), Intent provides a **generic pattern construct** that can encode any behavioral pattern:

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
            pending -> running(0)
                on trigger

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

**Applying a pattern:**

```intent
behavior OrderFulfillment {
    applies Saga {
        steps: [
            { command: ReserveInventory, success: InventoryReserved, failure: InventoryFailed },
            { command: ProcessPayment, success: PaymentCompleted, failure: PaymentFailed },
            { command: ShipOrder, success: OrderShipped, failure: ShipFailed }
        ]
        compensate: {
            ReserveInventory -> ReleaseInventory,
            ProcessPayment -> RefundPayment,
            ShipOrder -> CancelShipment
        }
        timeout: 30m
    }
}
```

### 8.5 Pattern Combinators

Patterns compose through combinators:

```intent
// Retry with exponential backoff
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
            pending -> attempting(1)
                on invoke
                effect { emit Op }

            attempting(n) -> succeeded
                on Op.success

            attempting(n) -> waiting(n)
                on Op.failure
                where { n < max_attempts }

            attempting(n) -> exhausted
                on Op.failure
                where { n >= max_attempts }

            waiting(n) -> attempting(n + 1)
                after { initial_delay * backoff^(n-1) }
                effect { emit Op }
        }
    }
}

// Circuit breaker
pattern CircuitBreaker<Op> {
    parameters {
        failure_threshold: Int
        success_threshold: Int
        timeout: Duration
    }

    behavior {
        states [closed, open, halfopen]
        initial closed

        state counters {
            failures: Int
            successes: Int
        }

        transitions {
            closed -> open
                on Op.failure
                where { counters.failures >= failure_threshold }

            open -> halfopen
                after { timeout }

            halfopen -> closed
                on Op.success
                where { counters.successes >= success_threshold }

            halfopen -> open
                on Op.failure
        }
    }
}

// Compose: Retry wrapped in CircuitBreaker
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

### 8.6 Pattern Library

Common patterns are provided as a standard library:

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

```intent
// Using standard library patterns
behavior PaymentProcessing {
    applies Timeout<Retry<CircuitBreaker<ProcessPayment>>> {
        timeout: 10s
        max_attempts: 3
        initial_delay: 100ms
        backoff: 2.0
        failure_threshold: 5
        success_threshold: 2
        reset_timeout: 30s
    }
}
```

---

## 9. Constraint — Cross-Cutting Properties

### 9.1 Structural Constraints (unchanged from v0.2)

```intent
constraint architecture {
    layer api { [routes, handlers] }
    layer domain { [services, models] }
    layer infra { [storage, external] }

    [services] must_not depend_on storage_backends
    *Client occur_only_in [storage]
}
```

### 9.2 Quantifiers, Predicates, Implication

```intent
predicate isolated(source, target) {
    source must_not depend_on target
    source must_not reference target
}

constraint boundaries {
    isolated(services, storage_backends)

    forall s in services: s must_reference [AppError]

    forall m in services:
        m depends_on cache => m must_depend_on cache_invalidation
}
```

### 9.3 Non-Functional Constraints (v0.3)

```intent
constraint performance {
    category: non_functional

    latency {
        operation settle: p99 < 100ms
        operation validate: p99 < 10ms
    }

    throughput {
        system: > 10_000 tps
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

---

## 10. Progression — Implementation Staging (v0.3)

```intent
system PaymentPlatform {
    progression {
        stage alpha {
            scope: [Ingestion, Processing]
            constraints: [architecture]
            target: "Single-tenant, manual settlement"
        }

        stage beta {
            extends: alpha
            scope: [Ingestion, Processing, Settlement]
            constraints: [architecture, performance]
            target: "Automated settlement, limited throughput"
        }

        stage ga {
            scope: all
            constraints: all
            target: "Multi-tenant, full SLA compliance"
        }
    }

    current_stage: beta
}
```

**Stage-scoped constraints:**

```intent
constraint layering {
    stage alpha {
        [services] may depend_on [storage]  // relaxed
    }

    stage beta {
        [services] must_not depend_on [storage]
        [services] must depend_on [StorageCoordinator]
    }
}
```

---

## 11. Distillation — Learning from Implementation (v0.3)

### 11.1 Distilled Patterns

Distilled patterns capture reusable behaviors extracted from implementation. The `commit` field is **required** to ensure traceability.

```intent
distilled pattern RetryWithBackoff {
    source: "crates/*/src/client.rs"
    commit: "a1b2c3d"  // required: commit hash where pattern was extracted
    extracted: "2026-02-15"

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
            attempting -> waiting on failure where { retry_count < max_retries }
            attempting -> exhausted on failure where { retry_count >= max_retries }
            waiting -> attempting after { initial_delay * backoff_factor^retry_count }
        }
    }

    applies_to {
        *Client.call
        *Gateway.invoke
    }
}
```

### 11.2 Distillation Markers

```intent
concern StorageResilience {
    distilled from "crates/storage/src/coordinator.rs" {
        commit: "abc123"
        observation: "Circuit breaker pattern emerged in error handling"
    }

    apply CircuitBreaker(threshold: 5, timeout: 30s)
        to StorageCoordinator.dgraph_client {
            formal "formal/tla/CircuitBreaker.tla"
        }
}
```

### 11.3 Insights

```intent
insight LatentCoupling {
    discovered: "2026-02-10"
    source: "Implementation review"

    observation {
        "Services A and B both access UserCache but have inconsistent
         invalidation logic."
    }

    recommendation {
        constraint cache_consistency {
            [ServiceA, ServiceB] must depend_on [CacheInvalidator]
        }
    }

    status: proposed
}
```

---

## 12. Deployment & Tooling Specification (v0.3)

### 12.1 Deployment Targets

```intent
deployment Production {
    platform: kubernetes

    mapping {
        Ingestion -> "ingestion-service" { replicas: 3, cpu: "500m", memory: "1Gi" }
        Processing -> "processing-service" { replicas: 5, cpu: "1", memory: "2Gi" }
    }

    dependencies {
        postgres: "postgres-cluster.db.svc:5432"
        redis: "redis-cluster.cache.svc:6379"
    }
}
```

### 12.2 CI/CD Specification

```intent
pipeline Verification {
    stages {
        lint {
            runs: ["cargo clippy", "eslint"]
            gate: must_pass
        }

        intent_check {
            runs: ["intent check formal/intent/ --codebase src/"]
            gate: must_pass
        }

        model_check {
            runs: ["apalache-mc check formal/tla/*.tla"]
            gate: must_pass
            timeout: 30m
        }
    }

    triggers {
        pull_request: [lint, intent_check]
        merge: all
        nightly: [model_check]
    }
}
```

### 12.3 Tooling

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
    }

    decided because {
        "Rust for performance-critical processing."
        "TLA+ for proven formal verification ecosystem."
    }
}
```

---

## 13. Architecture

```
+--------------------------------------------------+
|  CLI (main.rs)                                   |
|  commands: check, structural, compile,           |
|            verify, rationale, plan, progress     |
+------------------+-------------------------------+
                   |
      +------------+------------+
      |            |            |
+-----v-----+ +----v----+ +-----v------+
| Structural| |Behavioral| |Non-Func   |
| Verif.    | |Compile  | |Extract    |
| (syn)     | |(TLA+)   | |(config)   |
+-----+-----+ +----+----+ +-----+------+
      |            |            |
+-----v------------v------------v-----+
|  Parser & AST (parser/)             |
|  + System/Model/Interface/Behavior  |
|  + Rationale + Distillation         |
+---------+---------------------------+
          |
+---------v---------------------------+
|  Plan Mode Validation               |
|  (no codebase required)             |
+-------------------------------------+
```

### 13.1 Parser Layer

LR(1) parser (generated by `lalrpop`) for the `.intent` grammar. v0.3 extends to ~350 lines with system hierarchy, models, interfaces, behaviors, progression, and distillation constructs.

**AST design:** `TopLevel` is either `System` or `Concern`. Systems contain subsystems, models, interfaces, adapters, behaviors, and constraints. Concerns remain flat for backward compatibility.

### 13.2 Structural Verification

Single-pass, independent constraint checking against a prebuilt code index.

**CrateIndex:** The codebase is parsed once using `syn` into:
- Module tree (Rust `mod` declarations)
- File analysis (imports, type references, call references, trait impls)
- Entity reference map (`name -> [(file, line)]`)
- Trait implementation map (`(trait, type) -> [files]`)

**Constraint checkers:** Independent checker modules, one per rule type. v0.3 adds:
- Quantifier expansion (forall/exists over resolved scopes)
- Predicate expansion (inline predicate bodies)
- Stage-scoped evaluation (filter by `current_stage`)

### 13.3 Behavioral Compilation

Compiles behaviors and `apply...formal` blocks to TLA+ obligation modules.

**v0.3 additions:**
- `behavior` temporal properties → TLA+ temporal formulas
- `model` invariants → TLA+ `TypeOK` predicates
- `interface` pre/post → TLA+ action guards/postconditions
- `adapter` transformations → TLA+ data refinement proofs
- Refinement maps → TLA+ refinement obligations

### 13.4 Plan Mode Validation

Validates specifications without a codebase:
- Scope references resolve
- Layers are acyclic
- Parameter invariants hold
- State machine completeness (reachability, orphans)
- Progression stages are well-formed

### 13.5 Non-Functional Extraction

Extracts performance/cost constraints to:
- Benchmark assertion configurations
- CI gate thresholds
- Deployment resource limits

---

## 14. Implementation Decisions

### 14.1 syn for AST analysis

The Rust connector uses `syn` for source-level analysis:
- **No compilation required** — operates on source text
- **Fast** — ~1.5s for ~50k lines
- **No type resolution** — acceptable for architectural constraints

### 14.2 LR(1) parsing via lalrpop

Grammar is self-documenting (`intent.lalrpop` is the authoritative syntax reference). Free error recovery and location tracking.

### 14.3 Module tree resolution

Two strategies:
1. **AST-based:** Walk `lib.rs`/`main.rs`, follow `mod` declarations
2. **Directory fallback:** For orphan files, use directory heuristics

### 14.4 Stage-scoped verification (v0.3)

When `current_stage` is set, only constraints relevant to that stage are evaluated. Constraints without stage annotations apply to all stages.

---

## 15. Verification Flow

```
intent check formal/intent/ --codebase crates/nxbrain-core/src

Phase 1:  Parse .intent files → AST (systems, concerns)
Phase 2:  Build CrateIndex (syn parse all .rs files)
Phase 3:  Resolve scopes (map names to code entities)
Phase 4:  Determine current_stage (if progression defined)
Phase 5:  Filter constraints by stage
Phase 6:  Generate layer constraints (implicit must_not depend_on)
Phase 7:  Expand quantifiers and predicates
Phase 8:  Evaluate each constraint rule against index
Phase 9:  Compile behavioral obligations to TLA+
Phase 10: Invoke Apalache (if available)
Phase 11: Extract rationale + distillation to JSON
Phase 12: Report results
```

---

## 16. Extension Points

### 16.1 Adding a new constraint rule

1. Add variant to `ConstraintRule` enum in `ast.rs`
2. Add grammar rule in `intent.lalrpop`
3. Implement checker in `structural/checker/`
4. Add dispatch case in `structural/checker/mod.rs`
5. Add parser + checker tests

### 16.2 Adding a new top-level construct

1. Add to `TopLevel` enum in `ast.rs`
2. Add grammar rule in `intent.lalrpop`
3. Add handling in relevant compilation/verification pass
4. Update CLI if new command needed

### 16.3 Adding a language connector

Connector interface:
- `resolve_scope`: Map scope declarations to code entities
- `check_dependency`: Test whether entity A depends on entity B
- `find_pattern`: Find code matching a pattern
- `resolve_name`: Map architectural names to code entities

A TypeScript connector would use `ts-morph` or the TypeScript compiler API.

---

## 17. Relationship to Existing Tools

| Tool | Relationship to Intent |
|------|----------------------|
| **ArchUnit (Java)** | Similar structural goals. Intent adds hierarchy, behaviors, refinement, distillation. |
| **dependency-cruiser (JS)** | Dependency-only. Intent covers behaviors, rationale, lifecycle. |
| **cargo-deny** | Crate-level auditing. Intent operates at module/type level. |
| **TLA+/Apalache** | Intent generates obligations for Apalache, doesn't replace TLA+. |
| **Quint** | Potential alternative behavioral backend. |
| **ADR (Architecture Decision Records)** | Intent's rationale blocks are machine-readable ADRs. |

---

## 18. Current Status and Roadmap

### Implemented (v0.1)

- LR(1) parser with core constructs
- 6 structural constraint checkers
- Behavioral compilation (CircuitBreaker pattern)
- Apalache invocation scaffolding
- Rationale extraction and JSON reporting
- CLI with 5 subcommands

### Implemented (v0.2)

- Set algebra on scopes (union, intersection, difference, comprehension)
- Let bindings for named set expressions
- Universal and existential quantifiers (forall, exists)
- Implication (condition => consequence)
- Predicate definitions and calls
- State machines with invariants
- Plan mode validation

### Planned (v0.3)

- [ ] **System hierarchy** — `system` with `subsystems`, `parent`
- [ ] **Model declaration** — `fields`, `enum`, `derived`, model invariants
- [ ] **Interface declaration** — per-module contracts with `owner`, `operation` with `requires`/`ensures`, `protocol`
- [ ] **Adapter declaration** — connects interfaces with `mapping`, `transforms`, `error_handling`
- [ ] **Behavior enhancements** — temporal properties (`always`, `eventually`), `fairness`
- [ ] **Event-driven behaviors** — `subscribes`, `emits`, `effect` blocks, command handlers
- [ ] **Event sourcing** — `event_sourced`, derived state from events, stream identifiers
- [ ] **Patterns** — generic `pattern` construct with parameters, `applies` for instantiation, pattern composition
- [ ] **Maturity levels** — `sketch`, `draft`, `spec`, `final`
- [ ] **Explicit refinement** — `system X refines Y`, `refinement_map`, `action_map`
- [ ] **Progression** — `stage` definitions, `current_stage`, stage-scoped constraints
- [ ] **Non-functional constraints** — `category: non_functional`, latency/throughput/resource specs
- [ ] **Distillation** — `distilled pattern` (requires commit hash), `distilled from`, `insight`
- [ ] **Deployment** — `deployment` targets with resource mappings
- [ ] **Pipeline** — `pipeline` with stages and triggers
- [ ] **Tooling** — `tooling` block for tool choice documentation

### Future

- **Generic pattern compilation** — pattern-parameterized obligation generation
- **Incremental verification** — cache CrateIndex, re-analyze only changed files
- **Drift detection** — evaluate `revisit when` conditions via structural triggers
- **TypeScript connector** — frontend architectural constraints
- **Multi-language bridges** — `adapter` across language boundaries (e.g., Rust module ↔ TypeScript interface)

### Non-goals

- **Code generation** — Intent constrains; it does not generate
- **Runtime verification** — Intent operates at build time
- **Full temporal logic** — complex temporal properties belong in hand-written TLA+

---

## 19. Migration from v0.2

All v0.2 syntax is valid v0.3 syntax:

| v0.2 Construct | v0.3 Status |
|---------------|-------------|
| `concern C { ... }` | Unchanged |
| `scope`, `constraint`, `layer` | Unchanged |
| `statemachine` | Deprecated alias for `behavior` |
| `parameter`, `invariant` | Unchanged |
| `apply...refines` | Updated to `apply...formal` |
| `forall`, `exists`, `predicate` | Unchanged |

New constructs (`system`, `model`, `interface`, `progression`, `distilled`, etc.) are additive.
