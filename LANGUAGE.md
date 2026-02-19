# Intent Language Specification

**Version:** 0.2.0
**Status:** Living document
**Updated:** 2026-02-17

---

## 1. Overview

Intent is a domain-specific language for machine-verifiable architectural constraints. It captures behavioral specifications that transpile to TLA+ for formal verification.

```
Specification ──► Intent ──► TLA+ ──► Apalache ──► Implementation
      ▲                                              │
      └────────────── Distillation ◄─────────────────┘
```

**Core principle:** Minimal syntax, maximum expressiveness through composition.

---

## 2. Lexical Structure

### 2.1 Identifiers

```
IDENT = [a-zA-Z_][a-zA-Z0-9_]*
```

### 2.2 Literals

| Type | Syntax | Examples |
|------|--------|----------|
| Int | `[0-9]+(_[0-9]+)*` | `5`, `1_000_000` |
| Float | `[0-9]+\.[0-9]+` | `0.03`, `1.5` |
| Duration | `[0-9]+[μsmhd]` | `100ms`, `30s`, `5m` |
| String | `"[^"]*"` | `"reason text"` |

### 2.3 Keywords

```
// Declarations
system      component      components   behavior     pattern     constraint

// Structure
states      transitions    on           effect       property    invariant
kind        contains       depends_only layer       subsystem   module
order       parameters     default

// Logic
forall      exists         predicate    where        after
in          matches        all          let

// Imports
import      template       with         from         uses        applies
refines     implements     depends      references

// State machine
initial     terminal       emit

// Temporal (LTL-complete)
always      eventually     next         until        releases
weak_until  strong_releases fairness    weak         strong

// Rationale (consolidated)
rationale   distilled      commit       observation
decided     because        rejected     revisit      when
discovered  source         recommendation

// Literals
true        false          description
```

### 2.4 Comments

```intent
// Line comment
/* Block comment */
```

---

## 3. Top-Level Declarations

```
File = { Import | System | Pattern | Insight }
```

---

## 4. System Declaration

The system is the primary container. All other constructs live within systems.

```intent
system PaymentPlatform {
    description "Multi-tenant payment processing"

    components [Ingestion, Processing, Settlement]

    // Component definitions
    component Processing {
        kind: subsystem
        implements "crates/processing/src"

        behavior TransactionLifecycle { ... }
    }

    component API {
        kind: layer
        contains [routes, handlers]
        depends_only [Processing]
    }

    // Cross-cutting constraints
    constraint isolation {
        !Processing.depends(storage_backends)
        Processing.references([AppError])
    }

    // System properties (formerly deployment/tooling)
    platform: kubernetes
    ci: { stages: [lint, test, verify] }
}
```

### 4.1 Component Kinds

| Kind | Purpose | Generates |
|------|---------|-----------|
| `layer` | Architectural stratum | Implicit dependency constraints |
| `subsystem` | Bounded context | TLA+ module |
| `module` | Code module grouping | Static analysis scope |

### 4.2 Component Declaration

```intent
component Processing {
    kind: subsystem

    // Optional: maps to code path
    implements "crates/processing/src"

    // Optional: restrict dependencies
    depends_only [StorageAPI, EventQueue]

    // Components can nest
    component Validator {
        kind: module
        contains [schema_check, auth_check]
    }
}
```

### 4.3 Layer Ordering

```intent
component API { kind: layer, order: 1 }
component Domain { kind: layer, order: 2 }
component Infra { kind: layer, order: 3 }

// Implicit: layer N cannot depend on layer < N
```

---

## 5. Imports and Pattern Library

### 5.1 Import Patterns from GitHub

```intent
import pattern Saga from "github.com/org/intent-patterns@v1.2"
import pattern CircuitBreaker from "github.com/org/intent-patterns@v1.2"
```

### 5.2 Import Subsystem Templates

```intent
import template Auth from "github.com/org/templates/auth@main"
    with { provider: oauth2, mfa: true }

system MySystem {
    uses Auth  // Instantiates Auth template
}
```

### 5.3 Standard Library

Built-in patterns (no import needed):

| Pattern | Purpose |
|---------|---------|
| `EventSourced` | Declare event subscriptions and emissions |
| `Timeout` | Enforce deadline with fallback state |
| `Scoped` | Restrict access to resources |
| `Retry<Op>` | Retry with configurable backoff |
| `CircuitBreaker` | Fail fast when downstream unhealthy |
| `Saga` | Distributed transaction with compensation |
| `ProcessManager<W>` | Long-running workflow coordinator |
| `RateLimiter` | Limit operations per time window |
| `Bulkhead` | Isolate resources with concurrency limits |

### 5.4 Built-in Predicates

These are implemented by the structural checker, not as library code:

| Predicate | Purpose |
|-----------|---------|
| `A.depends(B)` | A imports/uses B |
| `A.references(B)` | A mentions type B |
| `A.implements(T)` | A implements trait T |
| `A.contains(B)` | B is nested within A |

Use them directly in constraints:

```intent
constraint isolation {
    !services.depends(storage_backends)
    services.references([AppError])
}
```

---

## 6. Behavior — State Machines

### 6.1 States and Transitions

```intent
behavior TransactionLifecycle {
    states {
        pending   { initial: true }
        validating
        processing
        settled   { terminal: true }
        failed    { terminal: true }
    }

    transitions {
        pending -> validating    on receive
        validating -> processing on valid      where { amount <= limit }
        validating -> failed     on invalid
        processing -> settled    on confirmed
        processing -> failed     on timeout
    }
}
```

### 6.2 Effects (Event-Driven)

Use `applies EventSourced` to declare event subscriptions and emissions:

```intent
behavior OrderProcessor {
    applies EventSourced {
        subscribes: [OrderCreated, PaymentCompleted]
        emits: [ReserveInventory, ShipOrder]
    }

    states { idle, reserving, charging, completed }

    transitions {
        idle -> reserving on OrderCreated
            effect { emit ReserveInventory(order_id, items) }

        reserving -> charging on InventoryReserved
            effect { emit ProcessPayment(order_id, total) }
    }
}
```

### 6.3 Temporal Properties

```intent
behavior TransactionLifecycle {
    property eventual_completion {
        always(pending => eventually(settled | failed))
    }

    property failure_permanent {
        always(failed => always(failed))
    }

    fairness {
        weak(validating -> processing | failed)
        strong(processing -> settled | failed)
    }
}
```

### 6.4 Applying Patterns

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

### 6.5 Composed Behaviors

```intent
behavior SystemFlow composes [Ingestion.Flow, Processing.Flow] {
    invariant ordering {
        Ingestion.received(t1) < Ingestion.received(t2)
            => Processing.started(t1) <= Processing.started(t2)
    }
}
```

---

## 7. Pattern Declaration

Patterns are reusable, parameterized behaviors.

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

            waiting(n) -> attempting(n + 1)
                after { initial_delay * backoff^(n-1) }
                effect { emit Op }
        }
    }
}
```

---

## 8. Constraint — Structural Rules

### 8.1 Predicates (Method-Style Syntax)

| Predicate | Meaning |
|-----------|---------|
| `A.depends(B)` | A imports/uses B |
| `A.references(B)` | A mentions type B |
| `A.implements(T)` | A implements trait T |
| `A.contains(B)` | A is parent of B |

Multiple arguments are supported: `A.depends(B, C, D)`.

### 8.2 Constraint Examples

```intent
constraint architecture {
    // Negation
    !services.depends(storage_backends)

    // Conjunction
    services.references([AppError]) && !services.references([RawError])

    // Implication
    m.depends(cache) => m.depends(cache_invalidation)

    // Quantifiers
    forall s in services: s.references([AppError])

    exists s in services: s.depends(logging)

    // Pattern matching
    forall c in { x | x matches *Client }:
        storage.contains(c)
}
```

### 8.3 Predicate Definitions

```intent
predicate isolated(source, target) {
    !source.depends(target) && !source.references(target)
}

constraint boundaries {
    isolated(services, storage_backends)
    isolated(pipeline, auth)
}
```

### 8.4 Non-Functional Constraints

```intent
constraint performance {
    // Latency assertions
    p99(settle) < 100ms
    p99(validate) < 10ms

    // Throughput
    throughput(system) > 10_000 / s

    // Resources
    memory < 4GB
    cpu < 2
}
```

---

## 9. Invariants

Invariants can appear in systems, behaviors, or standalone.

```intent
// In a model
invariant positive_balance { balance >= 0 }

// In a behavior
invariant single_settlement {
    forall t in Transaction: t.settled_count <= 1
}

// In a system
invariant total_balance {
    sum(accounts.balance) == sum(transactions.settled_amount)
}
```

---

## 10. Refinement

### 10.1 System Refinement

```intent
system Concrete refines Abstract {
    map {
        Abstract.pending -> [Concrete.queued, Concrete.validating]
        Abstract.done -> [Concrete.settled]
    }

    strengthens Abstract.safety with local_safety
}
```

### 10.2 Behavior Refinement

```intent
behavior OrderLifecycle {
    refines "formal/tla/OrderFlow.tla"
}
```

---

## 11. Versioning (No New Keywords)

Versioning is expressed through existing constructs:

```intent
behavior TransactionMigrations {
    states { v1, v2, v3 }

    transitions {
        v1 -> v2 on upgrade
            effect { v2.metadata = default }

        v2 -> v3 on upgrade
            effect { v3.new_field = compute(v2) }
    }

    invariant version_order {
        forall t in history: t.version < 3 => t.metadata == null
    }
}
```

---

## 12. Distillation

### 12.1 Distilled Patterns

```intent
distilled pattern RetryWithBackoff {
    source: "crates/client/src/*.rs"
    commit: "a1b2c3d"  // required
    extracted: "2026-02-15"

    observation {
        "Exponential backoff emerged in all client implementations."
    }

    parameters { ... }
    behavior { ... }

    applies_to { *Client.call }
}
```

---

## 13. Rationale

Rationale consolidates design decisions, insights, and architectural rationale:

```intent
rationale CircuitBreakerDecision {
    discovered: "2026-02-10"
    source: "Code review"

    observation {
        "Services A and B use Cache but invalidate inconsistently."
    }

    recommendation {
        constraint cache_discipline {
            [ServiceA, ServiceB].depends([CacheInvalidator])
        }
    }

    decided because {
        "Circuit breakers prevent cascading failures."
    }

    rejected {
        retry_only: "Retries cause request pileup."
    }

    revisit when {
        "Dgraph runs in replicated HA"
    }
}
```

---

## 14. TLA+ Transpilation

### 14.1 Mapping Table

| Intent | TLA+ | LTL |
|--------|------|-----|
| `behavior { states }` | `VARIABLES` + `Init` | — |
| `transition A -> B on E` | `A_to_B == /\ state = "A"` | — |
| `property always(P)` | `[] P` | G P |
| `property eventually(P)` | `<> P` | F P |
| `property next(P)` | `P'` | X P |
| `P until Q` | `P \U Q` (TLC module) | P U Q |
| `P releases Q` | `~(~P \U ~Q)` | P R Q |
| `P weak_until Q` | `(P \U Q) \/ []P` | P W Q |
| `P strong_releases Q` | `(P releases Q) /\ <>P` | P M Q |
| `always(P => eventually(Q))` | `[](P => <>Q)` | G(P → F Q) |
| `fairness { weak }` | `WF_vars(Next)` | — |
| `invariant I` | `TypeOK == /\ I` | — |
| `refines Abstract` | `THEOREM Concrete => Abstract` | — |
| `forall x in S: P(x)` | `\A x \in S: P(x)` | — |
| `exists x in S: P(x)` | `\E x \in S: P(x)` | — |

### 14.2 Not Transpiled (Static Analysis Only)

| Intent | Verification |
|--------|--------------|
| `A.depends(B)` | Import graph analysis |
| `A.references(B)` | Type reference scan |
| `A.implements(T)` | Trait impl lookup |
| `p99(op) < Xms` | Benchmark assertions |

### 14.3 Automatic Variable Extraction

The transpiler automatically extracts data variables referenced in guards and effects:

| Variable Pattern | TLA+ Type | Initial Value |
|------------------|-----------|---------------|
| `*count`, `*num`, `*size` | `Nat` | `0` |
| `*enabled`, `*active`, `*valid` | `BOOLEAN` | `FALSE` |
| `*list`, `*queue`, `*items` | `Seq(...)` | `<<>>` |
| `*set`, `*pool` | `Set(...)` | `{}` |
| Other | Symbolic | `CHOOSE x \in {} : TRUE` |

All extracted variables are included in `UNCHANGED` clauses for transitions that don't modify them.

### 14.4 Behavior Composition

When a behavior uses `composes [A, B]`:

1. States from all source behaviors are merged (shared names unify)
2. Transitions are combined (conflicts are detected)
3. Properties and fairness specs are merged
4. Reachability validation ensures all states are reachable

```intent
behavior Combined {
    composes [FlowA, FlowB]
    
    // Additional states/transitions extend the composed base
    transitions {
        done -> archived on archive
    }
}
```

### 14.5 Apalache Type Annotations

For symbolic model checking, use `generate_for_apalache()` to produce:

```tla
\* @typeAlias: STATE = Str;
\* @typeAlias: EVENT = [type: Str, args: Seq(Int)];
\* @type: STATE;
VARIABLE state
```

### 14.6 Requires Hand-Written TLA+

- Probabilistic properties
- Real-time constraints (deadlines)
- Complex data invariants

---

## 15. Formal Grammar (EBNF)

```ebnf
(* TOP LEVEL *)
File          = { Import | System | Pattern | Rationale } ;

Import        = "import" ( "pattern" | "template" ) IDENT
                "from" STRING [ "with" "{" { IDENT ":" Value } "}" ] ;

(* SYSTEM *)
System        = "system" IDENT [ "refines" IDENT ] "{" { SystemItem } "}" ;
SystemItem    = Description | ComponentsDecl | Component | Behavior
              | Constraint | Invariant | RationaleBlock | Uses | Property ;

Description   = "description" STRING ;
ComponentsDecl = "components" "[" IDENT { "," IDENT } "]" ;
Uses          = "uses" IDENT ;

Property      = IDENT ":" ( Value | ObjectLiteral | ArrayLiteral ) ;

(* COMPONENT *)
Component     = "component" IDENT "{" { ComponentItem } "}" ;
ComponentItem = Kind | Implements | Contains | DependsOnly | Behavior ;

Kind          = "kind" ":" ( "layer" | "subsystem" | "module" ) ;
Implements    = "implements" STRING ;
Contains      = "contains" "[" IDENT { "," IDENT } "]" ;
DependsOnly   = "depends_only" "[" IDENT { "," IDENT } "]" ;

(* BEHAVIOR *)
Behavior      = "behavior" IDENT [ "composes" IdentList ] "{" { BehaviorItem } "}" ;
BehaviorItem  = StatesDecl | TransitionsDecl
              | Property | Fairness | Invariant | AppliesPattern | RefinesClause ;

StatesDecl    = "states" ( "{" { StateDecl } "}" | "[" StateList "]" ) ;
StateDecl     = IDENT [ "{" { "initial" ":" "true" | "terminal" ":" "true" } "}" ] ;
TransitionsDecl = "transitions" "{" { TransitionDecl } "}" ;
TransitionDecl = IDENT "->" IDENT "on" IDENT
                [ "where" "{" Expr "}" ]
                [ "effect" "{" { EffectStmt } "}" ]
                [ "after" "{" Expr "}" ] ;
EffectStmt    = "emit" IDENT [ "(" [ Expr { "," Expr } ] ")" ]
              | "if" Expr "{" { EffectStmt } "}" [ "else" "{" { EffectStmt } "}" ] ;

Property      = "property" IDENT "{" TemporalExpr "}" ;
TemporalExpr  = TemporalExpr "=>" TemporalExpr     (* implication *)
              | TemporalExpr "&" TemporalExpr      (* conjunction *)
              | TemporalExpr "|" TemporalExpr      (* disjunction *)
              | TemporalExpr "until" TemporalExpr  (* strong until: φ U ψ *)
              | TemporalExpr "releases" TemporalExpr (* release: φ R ψ *)
              | TemporalExpr "weak_until" TemporalExpr (* weak until: φ W ψ *)
              | TemporalExpr "strong_releases" TemporalExpr (* strong release: φ M ψ *)
              | "always" "(" TemporalExpr ")"      (* globally: G φ *)
              | "eventually" "(" TemporalExpr ")"  (* finally: F φ *)
              | "next" "(" TemporalExpr ")"        (* next: X φ *)
              | IDENT                              (* atomic proposition *)
              | "(" TemporalExpr ")" ;
Fairness      = "fairness" "{" { ( "weak" | "strong" ) "(" IDENT "->" IDENT ")" } "}" ;

AppliesPattern = "applies" IDENT "{" { IDENT ":" Value } "}" ;
RefinesClause = "refines" STRING ;

(* PATTERN *)
Pattern       = "pattern" IDENT [ TypeParams ] "{" { PatternItem } "}" ;
PatternItem   = Parameters | Behavior ;
Parameters    = "parameters" "{" { ParamDecl } "}" ;
ParamDecl     = IDENT ":" TypeExpr [ "{" { FieldConstraint } "}" ] ;

(* CONSTRAINT *)
Constraint    = "constraint" IDENT "{" { ConstraintRule } "}" ;
ConstraintRule = "!" ConstraintRule
               | ConstraintRule "&&" ConstraintRule
               | ConstraintRule "||" ConstraintRule
               | ConstraintRule "=>" ConstraintRule
               | "forall" IDENT "in" ScopeExpr ":" ConstraintRule
               | "exists" IDENT "in" ScopeExpr ":" ConstraintRule
               | PredicateCall
               | ComparisonExpr ;

PredicateCall = IDENT "(" ScopeExpr { "," ScopeExpr } ")" ;
ComparisonExpr = Expr CompOp Expr ;

(* PREDICATE DEFINITION *)
Predicate     = "predicate" IDENT "(" IDENT { "," IDENT } ")" "{" { ConstraintRule } "}" ;

(* INVARIANT *)
Invariant     = "invariant" IDENT "{" Expr "}" ;

(* DISTILLATION *)
Distilled     = "distilled" "pattern" IDENT "{" { DistilledItem } "}" ;
DistilledItem = "source" ":" STRING | "commit" ":" STRING | "extracted" ":" STRING
              | "observation" "{" STRING "}" | Parameters | Behavior | "applies_to" "{" GlobPattern "}" ;

(* RATIONALE - consolidated *)
Rationale     = "rationale" IDENT "{" { RationaleItem } "}" ;
RationaleBlock = Rationale ;  // inline in systems
RationaleItem = "discovered" ":" STRING
              | "source" ":" STRING
              | "observation" "{" STRING "}"
              | "recommendation" "{" { Constraint | Invariant } "}"
              | "decided" "because" "{" { STRING } "}"
              | "rejected" "{" { IDENT ":" STRING } "}"
              | "revisit" "when" "{" { STRING } "}" ;

(* REFINEMENT *)
Map           = "map" "{" { DottedName "->" ( IDENT | IdentList ) "}" ;
Strengthens   = "strengthens" DottedName "with" IDENT ;

(* EXPRESSIONS *)
Expr          = OrExpr ;
OrExpr        = AndExpr { "||" AndExpr } ;
AndExpr       = CompExpr { "&&" CompExpr } ;
CompExpr      = AddExpr [ CompOp AddExpr ] ;
CompOp        = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
AddExpr       = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr       = UnaryExpr { ( "*" | "/" ) UnaryExpr } ;
UnaryExpr     = "!" UnaryExpr | "-" UnaryExpr | Primary ;
Primary       = Value | "(" Expr ")" | DottedName | IDENT "(" [ Expr { "," Expr } ] ")" ;

ScopeExpr     = "[" IDENT { "," IDENT } "]"
              | "{" IDENT "|" IDENT "matches" Pattern "}"
              | IDENT | "all" ;

(* UTILITIES *)
IdentList     = "[" IDENT { "," IDENT } "]" ;
DottedName    = IDENT { "." IDENT } ;
TypeExpr      = IDENT [ "?" ] ;
TypeParams    = "<" IDENT { "," IDENT } ">" ;
Value         = INT | FLOAT | DURATION | STRING | "true" | "false" ;
ObjectLiteral = "{" { IDENT ":" Value } "}" ;
ArrayLiteral  = "[" Value { "," Value } "]" ;
GlobPattern   = DottedName [ "." ( "*" | IDENT ) ] ;
```

---

## 16. CLI Usage

```bash
# Full verification
intent check intent/ --codebase src/

# Compile to TLA+
intent compile intent/ --output formal/generated/

# Verify with Apalache
intent verify --tla formal/generated/

# Extract rationale
intent rationale intent/ --output rationale.json

# JSON output
intent check intent/ --codebase src/ --format json
```
