# Intent Language Specification

**Version:** 0.1.3
**Status:** Living document
**Updated:** 2026-02-23

---

## 1. Overview

Intent is a domain-specific language for machine-verifiable architectural constraints. It provides two verification paths:

- **Structural constraints** are checked against source code using `syn`-based static analysis (Rust).
- **Behavioral specifications** are transpiled to TLA+ for formal model checking with Apalache or TLC.

```
                    ┌── structural ──► syn analysis ──► pass/fail
.intent files ──►───┤
                    └── behavioral ──► TLA+ ──► Apalache/TLC ──► pass/fail
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
contains    depends_only   parameters   default

// Logic
forall      exists         predicate    where        after
in          matches        all          let

// Imports
import      template       with         from         uses        applies
refines     implements     depends      references

// State machine
initial     terminal       emit

// Temporal (LTL-expressive; some operators require TLC backend)
always      eventually     next         until        releases
weak_until  strong_releases fairness    weak         strong

// Rationale (consolidated)
rationale   distilled      commit       observation
decided     because        rejected     revisit      when
discovered  source         recommendation

// Literals
true        false          description

// TLA+ expression primitives
choose      let_in         case         of          subset      union_all
domain_of   except         then         rec
tuple       set            fun          assume
```

### 2.4 Comments

```intent
// Line comment
/* Block comment */
```

### 2.5 Name Resolution and Grounding

#### Namespace Model

Component identifiers form a hierarchical namespace. Top-level components are global within a system. Nested components are referenced using dot notation:

```intent
system Payments {
    component Gateway {
        contains [Validator, Router]
    }
}

// Reference nested component as:
Gateway.Validator
Gateway.Router
```

#### Grounding Rules

Components can be grounded to implementation artifacts using `implements`:

```intent
component AuthService {
    implements "services/auth"        // Binds to codebase_root/services/auth
    contains [TokenManager, SessionStore]
}
```

- `implements "path"` – binds to the module/directory at that path relative to codebase root
- `contains [a, b]` – refers to sub-modules or nested items within the component's implementation path
- Components without `implements` are abstract/structural only (no direct code binding)

#### Scope Expressions

Scope expressions define sets of components for constraints, flows, and other declarations:

```intent
// Explicit list of named entities
flow handles: [Gateway, Processor, Settlement]

// All components in the current system
constraint audit_logging {
    scope all
    require logging_enabled == true
}

// Pattern matching on names (glob-style)
constraint internal_only {
    scope { x | x matches "*Internal" }
    require external_access == false
}
```

| Expression | Meaning |
|------------|---------|
| `[A, B, C]` | Explicit list of named entities |
| `all` | All components in the current system |
| `{ x \| x matches Pattern }` | Pattern matching on names (glob-style wildcards) |

#### Cross-System References

Reference components in other systems using fully-qualified names:

```intent
system Orders {
    component Checkout {
        // Reference component from another system
        uses Payments.Gateway
        uses Inventory.StockService
    }
}
```

### 2.6 Keywords and Identifiers

#### Lexical Rules

All keywords listed in section 2.3 are reserved and cannot be used as identifiers directly.

#### Escape Mechanism

Backticks allow reserved words to be used as identifiers:

```intent
component `import` {
    contains [`depends`, `from`]
}
```

This is valid but discouraged. Component names should use PascalCase to avoid collisions with keywords (which are all lowercase).

#### Naming Conventions

- Component names: PascalCase (e.g., `PaymentGateway`, `AuthService`)
- Keywords: all lowercase (e.g., `import`, `depends`, `component`)
- Avoid naming components after keywords even with escaping

---

## 3. Top-Level Declarations

```
File = { Import | System | Pattern | Insight | Distilled | Predicate }
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
        implements "crates/processing/src"
        depends_only [StorageAPI, EventQueue]

        // Component with behavior is behavioral (transpiles to TLA+)
        behavior TransactionLifecycle { ... }
    }

    component API {
        contains [routes, handlers]
        depends_only [Processing]
    }

    // Cross-cutting constraints
    constraint isolation {
        !Processing.depends(storage_backends)
        Processing.references([AppError])
    }

    // Layering constraints
    constraint layering {
        !Storage.depends([API, Processing])
    }

    // System properties (formerly deployment/tooling)
    platform: kubernetes
    ci: { stages: [lint, test, verify] }
}
```

### 4.1 Components

Components are structural by default. A component with `behavior` is behavioral and transpiles to TLA+.

**Structural components** are used for dependency constraints:

```intent
component API {
    contains [routes, handlers]
    depends_only [Processing]
}
```

**Behavioral components** define state machines that transpile to TLA+:

```intent
component Processing {
    implements "crates/processing/src"
    depends_only [StorageAPI, EventQueue]

    behavior TransactionLifecycle { ... }
}
```

Components can nest:

```intent
component API {
    component Validator {
        contains [schema_check, auth_check]
    }
}
```

### 4.2 Dependency Constraints

Express layering and isolation through explicit constraints:

```intent
constraint layering {
    !Storage.depends([API, Domain])
    forall s in [Domain, API]: s.depends([Infra])
}
```

---

## 5. Imports and Pattern Library

### 5.1 Import Patterns from GitHub

> **Note:** Remote imports are parsed but not yet resolved at runtime. Use the built-in standard library patterns (section 5.3) for now.

```intent
import pattern Saga from "github.com/org/intent-patterns@v1.2"
import pattern CircuitBreaker from "github.com/org/intent-patterns@v1.2"
```

### 5.2 Import Subsystem Templates

> **Note:** Template imports are parsed but not yet resolved at runtime.

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

### 5.5 Predicate Semantics

This section defines the precise meaning of each built-in predicate.

#### `A.depends(B)`

True if A has a **direct dependency** on B.

| Language   | What is detected                                  |
|------------|---------------------------------------------------|
| Rust       | `use` statements within the component's source files |
| TypeScript | `import`/`require` statements                     |

**Current Implementation Notes (Rust):**

The structural checker uses `syn` for AST analysis, which means:
- **Detected:** `use crate::module::Type;` statements
- **Not detected:**
  - Fully-qualified paths without `use` (e.g., `std::collections::HashMap::new()`)
  - Trait bounds on generic parameters
  - Macro-generated imports
  - `Cargo.toml` dependencies (separate from code-level imports)

**Does NOT include:**
- Trait bounds – use `A.implements(T)` instead
- Macro expansions – detected best-effort only
- Transitive dependencies – use explicit chains or composition

**Transitivity:** NO. Only direct dependencies are matched. For transitive analysis, write explicit constraints:

```intent
constraint transitive_isolation {
    !A.depends(C)
    !A.depends(B) || !B.depends(C)  // if A depends on B, B must not depend on C
}
```

#### `A.references(B)`

True if A **mentions** type or symbol B anywhere in source code.

**Includes:**
- Type annotations
- Function parameters and return types
- Struct/class fields
- Generic type arguments

**Does NOT require import** – catches fully-qualified references like `std::io::Error`.

#### `A.implements(T)`

True if A implements trait/interface T.

| Language   | Detection                                      |
|------------|-----------------------------------------------|
| Rust       | `impl Trait for Type` within A's scope        |
| TypeScript | `implements` clause on class                  |

**Note:** Derive macros (`#[derive(...)]`) are detected best-effort.

#### `A.contains(B)`

True if B is **lexically nested** within A.

Examples:
- Module contains submodule
- Component contains nested component
- Type contains inner type

#### `depends_only [X, Y]`

Equivalent to:
```
forall d in A.dependencies: d in [X, Y]
```

Enforces that **no other direct dependencies exist** beyond those listed.

#### Known Limitations

**Structural Analysis (`syn`-based):**
- No type resolution – `use` statements are tracked, but types are not resolved
- Macro bodies are partially visible; macro-generated imports are not detected
- Conditional compilation (`#[cfg(...)]`) yields multiple possible dependency graphs

**Recommendations:**
- Use explicit `use` statements rather than fully-qualified paths
- For `#[cfg(...)]` variations, use the `allow` suppression with documented rationale:
  ```intent
  constraint layering {
      !storage.depends(presentation) allow {
          exception: [LegacyStorage]
          reason: "Migration in progress; tracked in JIRA-1234"
          expires: "2026-06-01"
      }
  }
  ```
- Run checks in CI with `--fail-on-suppressed` to catch expired suppressions

#### User-Defined vs Built-in Predicates

**Built-in predicates** (`depends`, `references`, `implements`, `contains`) are evaluated by the structural checker using `syn`-based analysis. They have precise semantics defined above.

**User-defined predicates** (via the `predicate` keyword) are macro-like expansions:

```intent
predicate isolated(source, target) {
    !source.depends(target) && !source.references(target)
}

constraint boundaries {
    isolated(services, storage_backends)
}
```

User-defined predicates expand at parse time into their body constraint rules. They cannot define new structural analysis logic–only compose existing built-in predicates.

---

## 6. Behavior – State Machines

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

Intent supports LTL-expressive temporal operators. Note that some operators require the TLC backend (see §14.7 for backend compatibility):

| Operator | LTL | Meaning |
|----------|-----|---------|
| `always(P)` | □P | P holds in every state |
| `eventually(P)` | ◇P | P holds in some future state |
| `next(P)` | XP | P holds in the next state |
| `P until Q` | P U Q | P holds until Q becomes true |
| `P releases Q` | P R Q | Q holds until P becomes true |
| `P weak_until Q` | P W Q | Like until, but Q need not occur |
| `P strong_releases Q` | P M Q | Like release, but P must occur |
| `!P` | ¬P | Negation |
| `P <=> Q` | P ↔ Q | Biconditional (equivalence) |

```intent
behavior TransactionLifecycle {
    property eventual_completion {
        always(pending => eventually(settled | failed))
    }

    property failure_permanent {
        always(failed => always(failed))
    }

    property response_timing {
        // After every request, the next state must be response or timeout
        always(request => next(response | timeout))
    }

    property equivalence {
        // settled is equivalent to committed or acknowledged
        always(settled <=> (committed | acknowledged))
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

### 6.6 Behavior Semantics

This section defines the operational semantics of behaviors.

#### State

A behavior has exactly one current state variable. Initial state is marked `initial: true`. Terminal states marked `terminal: true` have no outgoing transitions.

#### Variables

Behaviors can reference data variables in guards (`where`) and effects:

**Explicit Declaration (Recommended)**

Variables should be declared explicitly with types and initial values:

```intent
behavior Payment {
    variables {
        balance: Nat = 0
        pending: Set(TxId) = {}
        retries: Nat = 0
    }
    // ...
}
```

**Type Inference (Prototyping Only)**

When variables are not explicitly declared but referenced in guards/effects, the transpiler infers types heuristically:

| Name Pattern | Inferred Type | Initial Value |
|--------------|---------------|---------------|
| `*count`, `*num`, `*size`, `*level`, `*retry` | `Nat` | `0` |
| `*enabled`, `*active`, `*valid`, `*done`, `*complete` | `Bool` | `FALSE` |
| `*list`, `*queue`, `*items`, `*seq` | `Seq(...)` | `<<>>` |
| `*set`, `*pool`, `*ids` | `Set(...)` | `{}` |
| `*id`, `*name`, `*key`, `*address`, `*token` | `Str` | `"<var_name>"` |
| (default) | `Nat` | `0` |

**Warning:** Heuristic inference is for prototyping only. Production specifications should declare all variables explicitly to ensure correct TLA+ generation and avoid unintended type assumptions.

**Bounded Domains for Model Checking**

For symbolic model checking with Apalache, variables should have bounded domains:

```intent
variables {
    counter: Nat = 0      \\* Will be bounded to 0..N during checking
    items: Set(ItemId) = {}  \\* ItemId should be a finite set
}
```

#### Events

- Events are synchronous and instantaneous
- An event triggers at most one transition (determinism required)
- If multiple transitions match, it's a static error (ambiguous behavior)
- Events can carry payload: `on receive(amount, sender)`

#### Effects Execution

- `emit EventName(args)` – emits an event (may trigger other behaviors)
- `variable = expr` – updates a variable (takes effect after transition completes)
- `if cond { ... } else { ... }` – conditional effects
- All effects in a transition execute atomically

#### Simultaneous Effect Semantics

Effect blocks use **simultaneous** (not sequential) semantics. All statements in an effect block describe the relationship between the current state and the next state, and the order of statements is irrelevant. This is consistent with TLA+'s next-state relation semantics.

```intent
// These two transitions are semantically identical:
s1 -> s2 on event_a
    effect {
        counter = counter + 1
        send Channel.Value(n: counter)
    }

s1 -> s2 on event_b
    effect {
        send Channel.Value(n: counter)   // same result – order doesn't matter
        counter = counter + 1
    }
```

Variable reads reference the **current** state; variable writes define the **next** state. Since all effects execute simultaneously, there is no "read-after-write" within a single effect block.

For a detailed explanation and formal semantics, see `docs/EFFECT_SEMANTICS.md`.

#### Composition Semantics (`composes [A, B]`)

- **States**: union of all states; same-named states unify (must have compatible flags)
- **Variables**: union; same-named variables must have identical types/initial values
- **Transitions**: merged; conflict if same (source, event) pair with different targets
- **Properties**: conjoined (all must hold)

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

## 8. Constraint – Structural Rules

### 8.1 Predicates (Method-Style Syntax)

**Built-in predicates** are evaluated by the structural checker using `syn`-based analysis:

| Predicate | Meaning |
|-----------|---------|
| `A.depends(B)` | A imports/uses B (via `use` statements) |
| `A.references(B)` | A mentions type B anywhere in source |
| `A.implements(T)` | A implements trait T |
| `A.contains(B)` | A is parent of B (lexical nesting) |

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

### 8.3 User-Defined Predicates

User-defined predicates are **macro-like expansions** that compose built-in predicates. They are expanded at parse time:

```intent
predicate isolated(source, target) {
    !source.depends(target) && !source.references(target)
}

constraint boundaries {
    isolated(services, storage_backends)
    isolated(pipeline, auth)
}
```

**Important:** User-defined predicates cannot define new structural analysis logic–they only compose existing built-in predicates (`depends`, `references`, `implements`, `contains`).

**Adding New Structural Predicates:** To add a genuinely new predicate that requires code analysis:

1. Implement the predicate in `src/structural/checker/`
2. Register it in the predicate dispatch table
3. Add corresponding grammar support in `intent.lalrpop`

This is a compiler extension point, not a user-facing feature.

### 8.4 Comparison Expressions

General comparison expressions in constraints require the `check` keyword to disambiguate from predicates:

```intent
constraint data_constraints {
    forall c in [ContractA, ContractB]: check c.fee <= c.budget
    forall op in [Operation]: check op.latency < 100
}
```

The `check` keyword is required for LR(1) parsing to distinguish `x.field <= y` from potential predicate calls.

### 8.5 Non-Functional Constraints

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

### 8.6 Non-Functional Constraint Semantics

#### Metrics

| Metric | Unit | Description |
|--------|------|-------------|
| `p50(op)`, `p95(op)`, `p99(op)` | Duration | Latency percentiles |
| `throughput(scope)` | `N/s`, `N/m`, `N/h` | Operations per time unit |
| `memory` | `MB`, `GB` | Peak memory usage |
| `cpu` | cores (float) | CPU utilization |

#### Operation Binding

Operations (`op`) bind to code via:
- Behavior transitions: `p99(processing -> settled) < 100ms`
- Named operations in `implements` scope: `p99(validate) < 10ms`
- System-wide: `throughput(system) > 10_000/s`

#### Verification

Non-functional constraints are verified by **benchmark extraction**, not model checking:

1. **Extraction**: Constraints generate benchmark configuration
2. **Execution**: CI runs benchmarks with specified parameters
3. **Assertion**: Results compared against thresholds

```bash
# Extract benchmark config
intent extract-benchmarks intent/ --output bench/config.json

# Example generated config
{
  "benchmarks": [
    { "name": "settle", "metric": "p99", "threshold_ms": 100 },
    { "name": "validate", "metric": "p99", "threshold_ms": 10 }
  ]
}
```

#### Statistical Requirements

- Minimum sample size: 100 iterations (configurable: `min_samples: N`)
- Warmup: 10% of samples discarded by default
- Environment: constraints apply to CI environment unless tagged

#### Limitations

- Results are environment-dependent (not portable)
- No formal guarantees (empirical measurement only)
- Flaky results should use `tolerance: 10%` or similar

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

## 10. TLA+ Expression Primitives

Intent supports TLA+-style expression primitives for formal specification. These enable mathematical precision in invariants and can be transpiled to TLA+ for model checking.

### 10.1 CHOOSE – Select Element Satisfying Predicate

```intent
invariant select_available_worker {
    choose(worker, Workers, worker.status == "healthy")
}
```

TLA+ equivalent: `CHOOSE worker \in Workers : worker.status = "healthy"`

### 10.2 LET-IN – Local Definitions

```intent
invariant price_calculation {
    let_in {
        base_price = 100,
        discount = 10,
        tax_rate = 5
    } in (base_price - discount + tax_rate)
}
```

TLA+ equivalent: `LET base_price == 100 discount == 10 tax_rate == 5 IN base_price - discount + tax_rate`

### 10.3 IF-THEN-ELSE – Conditional Expression

```intent
invariant shipping_cost {
    if order_total > 100 then 0 else 10
}

invariant complex_condition {
    if is_prime && order_total > 50 then 0 else 5
}
```

TLA+ equivalent: `IF order_total > 100 THEN 0 ELSE 10`

### 10.4 CASE – Multi-way Conditional

```intent
invariant priority_assignment {
    case {
        tier == "platinum" => "high",
        tier == "gold" => "medium",
        tier == "silver" => "low",
        default: "lowest"
    }
}
```

TLA+ equivalent: `CASE tier = "platinum" -> "high" [] tier = "gold" -> "medium" [] tier = "silver" -> "low" [] OTHER -> "lowest"`

### 10.5 Set Operations

```intent
invariant power_set {
    subset(Regions)  // All possible subsets of Regions
}

invariant big_union {
    union_all(CustomerAddresses)  // Union of all elements
}

invariant domain_example {
    domain_of(ConfigMap)  // All keys in the map
}
```

TLA+ equivalents: `SUBSET Regions`, `UNION CustomerAddresses`, `DOMAIN ConfigMap`

### 10.6 Data Structures

```intent
invariant record_example {
    rec { id: "order-123", amount: 99, status: "pending" }
}

invariant tuple_example {
    tuple(latitude, longitude, timestamp)
}

invariant set_literal {
    set { "pending", "processing", "completed", "cancelled" }
}
```

### 10.7 Function Literal

```intent
invariant square_function {
    fun(x, Numbers, x * x)
}

invariant transform_function {
    fun(id, CustomerIds, lookup_name(id))
}
```

TLA+ equivalent: `[x \in Numbers |-> x * x]`

### 10.8 EXCEPT – Record/Function Updates

```intent
invariant record_update {
    except(rec { a: 1, b: 2, c: 3 }, [b], 10)
}

invariant nested_update {
    except(Config, [timeout, value], 5000)
}
```

TLA+ equivalent: `[rec EXCEPT !.b = 10]`

### 10.9 Quantifiers in Expressions

```intent
invariant all_orders_valid {
    forall order in Orders: (order.amount > 0)
}

invariant exists_high_priority {
    exists order in Orders: (order.priority == "high")
}

invariant nested_quantifier {
    forall customer in Customers: (exists order in Orders: (order.customer_id == customer.id))
}
```

The `forall x in S: P` and `exists x in S: P` syntax is used uniformly in both constraints and expressions. Domain and body use primary expressions; use parentheses for complex bodies.

TLA+ equivalents: `\A order \in Orders : order.amount > 0`, `\E order \in Orders : order.priority = "high"`

### 10.10 ASSUME – Model Checking Assumptions

```intent
invariant assume_positive_balance {
    assume(InitialBalance > 0)
}

invariant assume_bounded_workers {
    assume(WorkerCount >= 1 && WorkerCount <= 10)
}
```

TLA+ equivalent: `ASSUME InitialBalance > 0`

### 10.11 Transpilation Table

| Intent | TLA+ |
|--------|------|
| `choose(x, S, P)` | `CHOOSE x \in S : P` |
| `let_in { a = e1 } in (body)` | `LET a == e1 IN body` |
| `if c then e1 else e2` | `IF c THEN e1 ELSE e2` |
| `case { c1 => e1, default: e }` | `CASE c1 -> e1 [] OTHER -> e` |
| `subset(S)` | `SUBSET S` |
| `union_all(S)` | `UNION S` |
| `domain_of(f)` | `DOMAIN f` |
| `rec { a: 1 }` | `[a |-> 1]` |
| `tuple(a, b)` | `<<a, b>>` |
| `set { a, b }` | `{a, b}` |
| `fun(x, S, body)` | `[x \in S |-> body]` |
| `except(f, [i], v)` | `[f EXCEPT ![i] = v]` |
| `forall x in S: P` | `\A x \in S : P` |
| `exists x in S: P` | `\E x \in S : P` |
| `assume(P)` | `ASSUME P` |

---

## 11. Refinement

### 11.1 System Refinement

```intent
system Concrete refines Abstract {
    map {
        Abstract.pending -> [Concrete.queued, Concrete.validating]
        Abstract.done -> [Concrete.settled]
    }

    strengthens Abstract.safety with local_safety
}
```

### 11.2 Behavior Refinement

> **Note:** External TLA+ refinement checking (`refines "path.tla"`) is parsed but not yet verified at runtime. Behavior-to-behavior refinement (section 11.1) is validated.

```intent
behavior OrderLifecycle {
    refines "formal/tla/OrderFlow.tla"
}
```

---

## 12. Versioning (No New Keywords)

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

## 13. Distillation

> **Note:** The `distilled` keyword is parsed for forward compatibility but the Intent CLI does not yet include a built-in extraction engine. Distilled blocks currently serve as structured documentation.
>
> Distillation is available as an external tool: [intent-distill](https://github.com/wiggum-cc/chief-wiggum/blob/main/skills/intent-distill/SKILL.md). It analyzes source code to identify patterns, constraints, and rationale, then generates `.intent` files validated with `intent lint`.

Distillation extracts patterns and constraints from implementation code, feeding them back into specifications.

### 13.1 Input Sources

Distillation analyzes:
- **AST patterns**: Repeated code structures across files
- **Git history**: Evolution of implementations over time
- **Runtime traces**: (Future) Behavioral patterns from telemetry

### 13.2 Output Types

| Output | Description |
|--------|-------------|
| `distilled pattern` | Reusable behavior extracted from code |
| `distilled constraint` | Structural rule observed in codebase |
| `observation` | Human-readable insight requiring review |

### 13.3 Distilled Pattern Syntax

```intent
distilled pattern RetryWithBackoff {
    source: "crates/client/src/*.rs"
    commit: "a1b2c3d"
    extracted: "2026-02-15"
    confidence: 0.85  // 0.0-1.0, how certain the extraction is
    
    observation {
        "Exponential backoff pattern found in 5 client implementations."
    }
    
    parameters {
        max_attempts: Int { default: 3 }
        backoff_factor: Float { default: 2.0 }
    }
    
    behavior {
        states [idle, attempting, waiting, succeeded, failed]
        // ... state machine definition
    }
    
    applies_to { *Client.call }  // suggested scope
}
```

### 13.4 Distilled Constraint Syntax

```intent
distilled constraint ObservedLayering {
    source: "src/**/*.rs"
    commit: "b2c3d4e"
    confidence: 0.92
    
    observation {
        "All service modules avoid direct storage dependencies."
    }
    
    constraint {
        !services.depends(storage_backends)
    }
}
```

### 13.5 Soundness and Trust

Distilled artifacts are **advisory by default**:
- `confidence` indicates extraction certainty (based on pattern frequency, consistency)
- Must be explicitly promoted to enforced: `promoted: true`
- Promoted distillations become regular constraints/patterns

### 13.6 CLI Usage

```bash
# Extract patterns from codebase
intent distill src/ --output distilled/

# Review extracted patterns
intent distill src/ --dry-run --format json

# Promote a distilled pattern to enforced
intent promote distilled/RetryWithBackoff.intent
```

---

## 14. Rationale

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

## 15. TLA+ Transpilation

### 15.1 Mapping Table

| Intent | TLA+ | LTL |
|--------|------|-----|
| `behavior { states }` | `VARIABLES` + `Init` | – |
| `transition A -> B on E` | `A_to_B == /\ state = "A"` | – |
| `property always(P)` | `[] P` | G P |
| `property eventually(P)` | `<> P` | F P |
| `property next(P)` | `P'` | X P |
| `!P` | `~P` | ¬P |
| `P <=> Q` | `P <=> Q` | P ↔ Q |
| `P until Q` | `P \U Q` (TLC module) | P U Q |
| `P releases Q` | `~(~P \U ~Q)` | P R Q |
| `P weak_until Q` | `(P \U Q) \/ []P` | P W Q |
| `P strong_releases Q` | `(P releases Q) /\ <>P` | P M Q |
| `always(P => eventually(Q))` | `[](P => <>Q)` | G(P → F Q) |
| `fairness { weak }` | `WF_vars(Next)` | – |
| `weak(A -> B)` | `WF_vars(A_to_B)` | – |
| `strong(A -> B)` | `SF_vars(A_to_B)` | – |
| `invariant I` | `Inv_<Name> == I` | – |
| `refines Abstract` | `THEOREM Concrete => Abstract` | – |
| `forall x in S: P(x)` | `\A x \in S: P(x)` | – |
| `exists x in S: P(x)` | `\E x \in S: P(x)` | – |

### 15.2 Not Transpiled (Static Analysis Only)

| Intent | Verification |
|--------|--------------|
| `A.depends(B)` | Import graph analysis |
| `A.references(B)` | Type reference scan |
| `A.implements(T)` | Trait impl lookup |
| `p99(op) < Xms` | Benchmark assertions |

### 15.3 Variable Declarations

Variables must be declared explicitly using the `variables` block:

```intent
behavior Payment {
    variables {
        balance: Nat = 0
        pending: Set(TxId) = {}
        retries: Nat = 0
    }
    // ...
}
```

**Inference rules** (when type/initial value omitted):

| Variable Pattern | Inferred Type | Inferred Initial |
|------------------|---------------|------------------|
| `*count`, `*num`, `*size` | `Nat` | `0` |
| `*enabled`, `*active`, `*valid` | `BOOLEAN` | `FALSE` |
| `*list`, `*queue`, `*items` | `Seq(...)` | `<<>>` |
| `*set`, `*pool` | `Set(...)` | `{}` |

**Undeclared variables are an error.** If a guard or effect references an undeclared variable, the transpiler emits:

```
error[E0401]: undeclared variable `foo` in transition `A -> B`
  --> payment.intent:15:12
   |
15 |     where { foo > 0 }
   |             ^^^ declare in `variables { foo: Type = initial }`
```

All declared variables are included in `UNCHANGED` clauses for transitions that don't modify them.

### 15.4 Behavior Composition

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

### 15.5 Apalache Type Annotations

For symbolic model checking, use `generate_for_apalache()` to produce:

```tla
\* @typeAlias: STATE = Str;
\* @typeAlias: EVENT = [type: Str, args: Seq(Int)];
\* @type: STATE;
VARIABLE state
```

### 15.6 Requires Hand-Written TLA+

- Probabilistic properties
- Real-time constraints (deadlines)
- Complex data invariants

### 15.7 Backend Limitations

The Intent transpiler targets **Apalache** by default, which has limited temporal support compared to TLC.

| Operator | Apalache | TLC | Notes |
|----------|----------|-----|-------|
| `always(P)` | ✓ Bounded | ✓ | Safety, works well |
| `eventually(P)` | ✓ Bounded | ✓ | Bounded model checking only |
| `next(P)` | ✓ | ✓ | – |
| `until` | ✗ | ✓ | Requires TLC or hand-written |
| `releases` | ✗ | ✓ | Requires TLC or hand-written |
| `weak_until` | ✗ | ✓ | Requires TLC or hand-written |
| `strong_releases` | ✗ | ✓ | Requires TLC or hand-written |
| `WF_vars(Action)` | ✓ | ✓ | Fairness supported |
| `SF_vars(Action)` | ✓ | ✓ | Fairness supported |
| Complex liveness | △ | ✓ | Manual review recommended |

**Operator categories:**

- **Apalache-safe**: `always`, `eventually`, `next`, `!`, `&&`, `||`, `=>`, `<=>`
- **TLC-only**: `until`, `releases`, `weak_until`, `strong_releases`
- **Hand-written**: Complex liveness combining fairness with nested temporal operators

When using TLC-only operators, the transpiler emits:

```
warning[W0501]: operator `until` requires TLC backend
  --> payment.intent:25:5
   |
25 |     property progress { pending until settled }
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   = note: use `--backend tlc` or write TLA+ manually
```

### 15.8 Generated Module Contract

Generated TLA+ follows a predictable structure for composition with hand-written specs.

**Module naming**: `<BehaviorName>_Intent`

**Variable prefixing**: All variables are prefixed with behavior name to avoid collisions:
```tla
VARIABLES Payment_state, Payment_balance, Payment_pending
```

**Predicates**:
| Generated Predicate | Purpose |
|---------------------|---------|
| `<Name>_Init` | Initial state predicate |
| `<Name>_Next` | Next-state relation |
| `<Name>_TypeOK` | Type invariant (auto-generated from variable declarations) |
| `Inv_<InvName>` | Each declared invariant |
| `<Name>_Safety` | `TypeOK /\ Inv_A /\ Inv_B /\ ...` |
| `<Name>_vars` | Tuple of all variables |

**Example generated module**:
```tla
---- MODULE Payment_Intent ----
EXTENDS Integers, Sequences, TLC

VARIABLES Payment_state, Payment_balance, Payment_retries

Payment_vars == <<Payment_state, Payment_balance, Payment_retries>>

Payment_TypeOK ==
    /\ Payment_state \in {"pending", "settled", "failed"}
    /\ Payment_balance \in Nat
    /\ Payment_retries \in Nat

Payment_Init ==
    /\ Payment_state = "pending"
    /\ Payment_balance = 0
    /\ Payment_retries = 0

Pending_to_Settled == ...
Pending_to_Failed == ...

Payment_Next ==
    \/ Pending_to_Settled
    \/ Pending_to_Failed

Inv_PositiveBalance == Payment_balance >= 0

Payment_Safety == Payment_TypeOK /\ Inv_PositiveBalance
====
```

**Hand-written extension**:
```tla
---- MODULE PaymentWithConstraints ----
EXTENDS Payment_Intent

\* Add additional constraints
StricterInvariant == Payment_balance < 1000000

\* Compose with other modules
VARIABLES ExternalAudit_state
...
====
```

---

## 16. Formal Grammar (EBNF)

```ebnf
(* TOP LEVEL *)
File          = { Import | System | Pattern | Rationale | Distilled | Predicate | EventDecl } ;

Import        = "import" ( "pattern" | "template" ) IDENT
                "from" STRING [ "with" "{" { IDENT ":" Value } "}" ] ;

(* EVENT DECLARATION *)
EventDecl     = "event" IDENT [ ":" TypeAnnotation ] ;

(* SYSTEM *)
System        = "system" IDENT [ "refines" IDENT ] "{" { SystemItem } "}" ;
SystemItem    = Description | ComponentsDecl | Component | Behavior
              | Constraint | Invariant | Rationale | Uses | Property
              | Applies | Pattern | Predicate | Let | Distilled | EventDecl ;

Description   = "description" STRING ;
ComponentsDecl = "components" "[" IDENT { "," IDENT } "]" ;
Uses          = "uses" IDENT ;
Let           = "let" IDENT "=" ScopeExpr ;

Property      = IDENT ":" ( Value | ObjectLiteral | ArrayLiteral ) ;

(* COMPONENT *)
Component     = "component" IDENT "{" { ComponentItem } "}" ;
ComponentItem = Implements | Contains | DependsOnly | Behavior
              | Component | Binds ;

Implements    = "implements" STRING ;
Contains      = "contains" "[" IDENT { "," IDENT } "]" ;
DependsOnly   = "depends_only" "[" IDENT { "," IDENT } "]" ;
Binds         = "binds" QualifiedName [ "as" IDENT ] [ "with" "{" { IDENT ":" Value } "}" ] ;

(* BEHAVIOR *)
Behavior      = "behavior" IDENT [ "composes" IdentList ] "{" { BehaviorItem } "}" ;
BehaviorItem  = NodesDecl | VariablesDecl | StatesDecl | TransitionsDecl
              | Property | Fairness | Invariant | Applies | RefinesClause
              | MapDecl | StrengthensDecl ;

NodesDecl     = "nodes" ":" IDENT ;
VariablesDecl = "variables" "{" { VariableDecl } "}" ;
VariableDecl  = IDENT ":" TypeExpr [ "=" Expr ] ;

StatesDecl    = "states" "{" { StateDecl } "}" ;
StateDecl     = IDENT [ "{" { "initial" ":" "true" | "terminal" ":" "true" } "}" ] ;
TransitionsDecl = "transitions" "{" { TransitionDecl } "}" ;
TransitionDecl = TransitionSource "->" TransitionTarget "on" IDENT
                [ "where" "{" Expr "}" ]
                [ "effect" "{" { EffectStmt } "}" ]
                [ "after" "{" Expr "}" ] ;
TransitionSource = IDENT                    (* single state *)
                 | "*"                      (* wildcard: any state *)
                 | "[" IDENT { "," IDENT } "]" ;  (* multiple states *)
TransitionTarget = IDENT                    (* single state *)
                 | "self"                   (* self-transition *)
                 | "[" IDENT { "," IDENT } "]" ;  (* multiple targets *)
EffectStmt    = "emit" IDENT [ "(" [ NamedArg { "," NamedArg } ] ")" ]
              | "if" Expr "{" { EffectStmt } "}" [ "else" "{" { EffectStmt } "}" ]
              | IDENT "=" Expr ;
NamedArg      = [ IDENT ":" ] Expr ;        (* named or positional *)

Property      = "property" IDENT "{" TemporalExpr "}" ;
TemporalExpr  = TemporalExpr "<=>" TemporalImplExpr   (* biconditional: φ ↔ ψ *)
              | TemporalImplExpr ;
TemporalImplExpr = TemporalImplExpr "=>" TemporalAndExpr (* implication *)
              | TemporalAndExpr ;
TemporalAndExpr = TemporalAndExpr "&" TemporalOrExpr  (* conjunction *)
              | TemporalOrExpr ;
TemporalOrExpr = TemporalOrExpr "|" TemporalBinaryExpr (* disjunction *)
              | TemporalBinaryExpr ;
TemporalBinaryExpr = TemporalAtom "until" TemporalBinaryExpr (* strong until: φ U ψ *)
              | TemporalAtom "releases" TemporalBinaryExpr (* release: φ R ψ *)
              | TemporalAtom "weak_until" TemporalBinaryExpr (* weak until: φ W ψ *)
              | TemporalAtom "strong_releases" TemporalBinaryExpr (* strong release: φ M ψ *)
              | TemporalAtom ;
TemporalAtom  = "always" "(" TemporalExpr ")"      (* globally: G φ *)
              | "eventually" "(" TemporalExpr ")"  (* finally: F φ *)
              | "next" "(" TemporalExpr ")"        (* next: X φ *)
              | "!" TemporalAtom                   (* negation: ¬φ *)
              | "count" "(" IDENT ")"              (* cardinality of nodes in state *)
              | INT                                (* integer literal *)
              | IDENT                              (* atomic proposition *)
              | "(" TemporalExpr ")" ;
Fairness      = "fairness" "{" { FairnessSpec } "}" ;
FairnessSpec  = ( "weak" | "strong" ) "(" IDENT "->" IDENT [ "|" IDENT { "|" IDENT } ] ")" ;

Applies       = "applies" IDENT [ TypeArgs ] "{" { IDENT ":" Value } "}" ;
RefinesClause = "refines" STRING ;
MapDecl       = "map" "{" { DottedName "->" ( IDENT | IdentList ) } "}" ;
StrengthensDecl = "strengthens" DottedName "with" IDENT ;

(* PATTERN *)
Pattern       = "pattern" IDENT [ TypeParams ] "{" { PatternItem } "}" ;
PatternItem   = Parameters | Behavior ;
Parameters    = "parameters" "{" { ParamDecl } "}" ;
ParamDecl     = IDENT ":" TypeExpr [ "{" { FieldConstraint } "}" ] ;
FieldConstraint = "default" ":" Value ;

(* CONSTRAINT *)
Constraint    = "constraint" IDENT "{" { ConstraintRule } "}" ;
ConstraintRule = ConstraintExpr ;
ConstraintExpr = ConstraintExpr "<=>" ConstraintImplExpr   (* biconditional *)
               | ConstraintImplExpr ;
ConstraintImplExpr = ConstraintImplExpr "=>" ConstraintOrExpr  (* implication *)
                   | ConstraintOrExpr ;
ConstraintOrExpr = ConstraintOrExpr "||" ConstraintAndExpr     (* disjunction *)
                  | ConstraintAndExpr ;
ConstraintAndExpr = ConstraintAndExpr "&&" ConstraintAtom      (* conjunction *)
                   | ConstraintAtom ;
ConstraintAtom = "!" ConstraintAtom                             (* negation *)
               | "forall" IDENT "in" ScopeExpr ":" ConstraintAtom
               | "exists" IDENT "in" ScopeExpr ":" ConstraintAtom
               | SuppressibleAtom ;
SuppressibleAtom = SimpleConstraint [ "allow" "{" { SuppressionItem } "}" ] ;
SimpleConstraint = PredicateCall
                 | ComparisonExpr
                 | NFConstraint
                 | "(" ConstraintRule ")" ;
PredicateCall = ScopeExpr "." BuiltinPredicate "(" [ ScopeExpr { "," ScopeExpr } ] ")"
              | ScopeExpr "." IDENT "(" [ ScopeExpr { "," ScopeExpr } ] ")" ;
BuiltinPredicate = "depends" | "references" | "implements" | "contains" ;
ComparisonExpr = "check" ConstraintOperand CompOp ConstraintOperand ;
ConstraintOperand = DottedName | "count" "(" IDENT ")" | IDENT | INT | FLOAT | DURATION | STRING | "true" | "false" ;
SuppressionItem = "exception" ":" IdentList
                | "reason" ":" STRING
                | "expires" ":" STRING
                | "tracking" ":" STRING ;

(* PREDICATE DEFINITION *)
Predicate     = "predicate" IDENT "(" IDENT { "," IDENT } ")" "{" { ConstraintRule } "}" ;

(* INVARIANT *)
Invariant     = "invariant" IDENT "{" Expr "}" ;

(* NON-FUNCTIONAL CONSTRAINTS *)
NFConstraint  = NFMetric "(" IDENT ")" CompOp ConstraintOperand
              | "memory" CompOp ConstraintOperand
              | "cpu" CompOp ConstraintOperand ;
NFMetric      = "p50" | "p90" | "p95" | "p99" | "throughput" ;

(* DISTILLATION *)
Distilled     = "distilled" "pattern" IDENT "{" { DistilledItem } "}" ;
DistilledItem = "source" ":" STRING
              | "commit" ":" STRING
              | "extracted" ":" STRING
              | "confidence" ":" FLOAT
              | "observation" "{" { STRING } "}"
              | Parameters
              | Behavior
              | "applies_to" "{" GlobPattern "}" ;

(* RATIONALE *)
Rationale     = "rationale" IDENT "{" { RationaleItem } "}" ;
RationaleItem = "discovered" ":" STRING
              | "source" ":" STRING
              | "observation" "{" { STRING } "}"
              | "recommendation" "{" ( Constraint | Invariant ) "}"
              | "decided" "because" "{" { STRING } "}"
              | "rejected" "{" { IDENT ":" STRING } "}"
              | "revisit" "when" "{" { STRING } "}" ;

(* TYPE ANNOTATION *)
TypeAnnotation = TypeBody ;
TypeBody      = "{" TypeField { "," TypeField } "}"              (* record *)
              | IDENT "<" TypeAnnotation { "," TypeAnnotation } ">"  (* generic *)
              | DottedName                                         (* qualified name *)
              | TypeAnnotation "?"                                  (* optional *)
              | IDENT ;                                             (* simple type *)
TypeField     = IDENT ":" TypeAnnotation ;

(* EXPRESSIONS *)
Expr          = OrExpr ;
OrExpr        = AndExpr { "||" AndExpr } ;
AndExpr       = CompExpr { "&&" CompExpr } ;
CompExpr      = AddExpr [ CompOp AddExpr ] ;
CompOp        = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
AddExpr       = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr       = UnaryExpr { ( "*" | "/" ) UnaryExpr } ;
UnaryExpr     = "!" UnaryExpr | "-" UnaryExpr | Primary ;
Primary       = Value
              | "(" Expr ")"
              | DottedName
              | IDENT "(" [ Expr { "," Expr } ] ")"
              | "count" "(" IDENT ")"
              (* TLA+ expression primitives *)
              | "choose" "(" IDENT "," Expr "," Expr ")"
              | "let_in" "{" LetBinding { "," LetBinding } "}" "in" "(" Expr ")"
              | "if" Expr "then" Expr "else" Primary
              | "case" "{" CaseArm { "," CaseArm } [ "default" ":" Expr ] "}"
              | "subset" "(" Expr ")"
              | "union_all" "(" Expr ")"
              | "domain_of" "(" Expr ")"
              | "rec" "{" RecordField { "," RecordField } "}"
              | "tuple" "(" Expr { "," Expr } ")"
              | "set" "{" Expr { "," Expr } "}"
              | "fun" "(" IDENT "," Expr "," Expr ")"
              | "except" "(" Expr "," "[" Expr { "," Expr } "]" "," Expr ")"
              | "forall" IDENT "in" Primary ":" Primary
              | "exists" IDENT "in" Primary ":" Primary
              | "assume" "(" Expr ")" ;

(* TLA+ expression helpers *)
LetBinding    = IDENT "=" Expr ;
CaseArm       = Expr "=>" Expr ;
RecordField   = IDENT ":" Expr ;

ScopeExpr     = "[" EntityName { "," EntityName } "]"
              | "{" IDENT "|" IDENT "matches" Pattern "}"
              | "{" IDENT "|" Expr "}"
              | ScopeExpr "|" ScopeExpr      (* union *)
              | ScopeExpr "&" ScopeExpr      (* intersection *)
              | QualifiedName
              | Glob
              | "all"
              | "(" ScopeExpr ")" ;
EntityName    = IDENT ;
QualifiedName = IDENT { "." IDENT } ;
Glob          = "*" IDENT                  (* prefix glob: *Client *)
              | IDENT "*" ;                (* suffix glob: Client* )
Pattern       = IDENT | STRING ;

(* UTILITIES *)
IdentList     = "[" IDENT { "," IDENT } "]" ;
DottedName    = IDENT { "." IDENT } ;
TypeExpr      = IDENT [ "?" ] ;
TypeParams    = "<" IDENT { "," IDENT } ">" ;
TypeArgs      = "<" IDENT { "," IDENT } ">" ;
Value         = INT | FLOAT | DURATION | STRING | "true" | "false" ;
ObjectLiteral = "{" { IDENT ":" Value } "}" ;
ArrayLiteral  = "[" Value { "," Value } "]" ;
GlobPattern   = Glob | STRING ;
```

---

## 17. CLI Usage

```bash
# Full verification (structural + compile + verify + rationale)
intent check intent/ --codebase src/

# Structural constraint verification only
intent structural intent/ --codebase src/

# Lint intent files for syntax and style issues
intent lint intent/ --pedantic --hints

# Compile behaviors to TLA+
intent compile intent/ --output out/

# Verify with Apalache (bounded) or TLC (exhaustive)
intent verify --obligations out/
intent verify --obligations out/ --mode exhaustive

# Extract rationale as JSON
intent rationale intent/ --output rationale.json

# Plan-mode validation (no codebase required)
intent plan intent/

# Extract non-functional constraints as benchmark config
intent extract-benchmarks intent/ --output benchmarks.json

# JSON output (works with all commands)
intent check intent/ --codebase src/ --format json
```

---

## 18. Error Handling and Diagnostics

### 18.1 Constraint Violations

When a structural constraint fails, diagnostics include:

- **File path and line number** of the violating entity
- **Dependency path** showing how A depends on B (chain of imports)
- **Suggested fixes** or suppression syntax

Example output:

```
error[E0301]: constraint violation in `layering`
  --> src/storage/cache.rs:15:1
   |
15 | use crate::api::handlers::AuthHandler;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = Storage depends on API, violating: !Storage.depends([API])
   = dependency path: Storage -> cache.rs -> AuthHandler -> API
   |
   = help: move shared types to a common module
   = help: or add suppression with `allow { exception: [Storage] }`
```

### 18.2 Suppression

Constraints can be suppressed with tracking metadata:

```intent
constraint architecture {
    !services.depends(storage) allow {
        exception: [LegacyService]
        reason: "Migration in progress"
        expires: 2026-06-01
        tracking: "JIRA-1234"
    }
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `exception` | Yes | List of entities exempt from this constraint |
| `reason` | Yes | Human-readable justification |
| `expires` | No | Date after which suppression should be reviewed |
| `tracking` | No | Issue tracker reference |

Expired suppressions emit warnings during verification.

### 18.3 Counterexample Traces

When Apalache finds a property violation, Intent renders the counterexample as a state sequence:

```
error[E0401]: property `eventual_settlement` violated
  --> payment.intent:45:5
   |
45 |     property eventual_settlement { pending => eventually(settled) }
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Counterexample trace (3 states):

  State 0 (initial):
    Payment_state = "pending"
    Payment_retries = 0
    --> payment.intent:12:5 (initial state)

  State 1 (after: retry_exhausted):
    Payment_state = "pending"
    Payment_retries = 3
    --> payment.intent:28:9 (transition retry)

  State 2 (after: timeout):
    Payment_state = "failed"
    Payment_retries = 3
    --> payment.intent:35:9 (transition timeout)

  = note: system reached terminal state "failed" without passing through "settled"
```

Each state shows:
- Current state values
- Triggering event/transition
- Mapped Intent source location where possible

### 18.4 Verification Levels

Output indicates the verification level for each check:

| Level | Description |
|-------|-------------|
| `sound` | Formally verified (TLA+/Apalache) |
| `checked` | Statically analyzed (structural predicates) |
| `advisory` | Best-effort, may have false positives/negatives |
| `benchmark` | Empirical measurement, environment-dependent |

Example output:

```
Verification results:
  [sound]     property eventual_settlement     PASS
  [sound]     invariant positive_balance       PASS
  [checked]   constraint layering              PASS
  [checked]   constraint no_cycles             PASS
  [advisory]  constraint naming_convention     PASS (2 warnings)
  [benchmark] p99(Gateway) < 100ms             PASS (measured: 47ms)
```

---

## Appendix A. Implementation Status

This section tracks which language features are fully implemented in the `intent` CLI (v0.1.3).

| Feature | Status | Notes |
|---------|--------|-------|
| Structural predicates (`depends`, `references`, `implements`, `contains`) | Implemented | Rust codebases via `syn` |
| Transitive dependency checking (`depends_transitively`) | Implemented | |
| Negated predicates | Implemented | |
| Quantifiers in constraints (`forall`, `exists`) | Implemented | |
| Logical operators (`&&`, `\|\|`, `!`, `=>`, `<=>`) | Implemented | |
| Scope expressions (entity lists, globs, `all`) | Implemented | Filtered scopes not yet active |
| Comparison constraints (`check x <= y`) | Parsed | Not yet evaluated by structural checker |
| Non-functional constraints (`p99`, `throughput`) | Parsed | Benchmark extraction only |
| Behavior state machines | Implemented | Full TLA+ transpilation |
| Variables with explicit types | Implemented | |
| Heuristic variable inference | Implemented | Emits warnings |
| Guards (`where`) and effects | Implemented | |
| Temporal properties (`always`, `eventually`, `next`) | Implemented | Apalache and TLC |
| Temporal operators (`until`, `releases`, `weak_until`, `strong_releases`) | Implemented | Requires TLC backend |
| Fairness constraints | Implemented | Weak and strong |
| TLA+ expression primitives | Implemented | All operators in section 10 |
| Pattern application (`applies`) | Implemented | Built-in stdlib patterns |
| Behavior composition (`composes`) | Implemented | Conflict detection included |
| Behavior refinement (behavior-to-behavior) | Implemented | Mapping validation included |
| External TLA+ refinement (`refines "path.tla"`) | Parsed | Not yet verified |
| Remote imports (`import from "github.com/..."`) | Parsed | Resolution not implemented |
| Distillation (`distilled pattern`) | Parsed | Built-in engine not yet active; available via [intent-distill](https://github.com/wiggum-cc/chief-wiggum/blob/main/skills/intent-distill/SKILL.md) |
| Rationale extraction | Implemented | JSON output |
| Linter | Implemented | 21 rules |
| TypeScript structural analysis | Implemented | Import/reference/interface checks for `.ts`/`.tsx`/`.js`/`.jsx` |
