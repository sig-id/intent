# Intent Language Design Document

**Status:** Living document
**Version:** 0.2.0
**Updated:** 2026-02-21

---

## 1. Problem Statement

In spec-driven agentic coding workflows, AI agents write code guided by specifications and verified by formal models. The verification stack has a gap between prose specifications and executable formal models:

| Layer | Captures | Verified by |
|-------|----------|-------------|
| Prose specs (`spec/*.md`) | Requirements, architecture, rationale | Human review only |
| **Intent** | **Architectural constraints, behavioral specs** | **Static analysis + TLA+** |
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

1. **Refinement** – decomposing abstract intent into concrete subsystem specs
2. **Distillation** – capturing emergent and latent patterns from implementation

Intent bridges this gap with a minimal language that transpiles to TLA+ for formal verification while enabling structural analysis of codebases.

---

## 2. Design Principles

### 2.1 Minimal Core, Maximum Composition

The language has ~25 keywords. Expressiveness comes from:
- **Predicates** instead of specialized keywords (`A.depends(B)` vs `A must_depend_on B`)
- **Logical operators** for composition (`!`, `&&`, `||`, `=>`)
- **Parameterized patterns** for reuse

### 2.2 One Language, Three Backends

| Constraint Type | Backend | Output |
|-----------------|---------|--------|
| Structural | Static analysis | Violation reports |
| Behavioral | TLA+ compiler | `.tla` modules |
| Non-functional | Benchmark extraction | CI gates |

### 2.3 Generate Obligations, Not Models

Intent generates TLA+ **proof obligations** that hand-written specs must satisfy. Component-level models remain hand-written. This avoids the complexity of generating complete TLA+ from scratch.

### 2.4 Properties, Not Keywords

Deployment and tooling configuration uses system **properties** instead of dedicated keywords:
- `deployment` → `platform: kubernetes`
- `pipeline` → `ci: { stages: [...] }`
- `tooling` → `lang: rust, framework: axum`
- `maturity` → `status: spec`

### 2.5 Explicit Over Implicit

- No inferred conventions for file paths
- No implicit scope resolution
- Grounding from architectural concepts to code entities is declared

### 2.6 Pattern Reuse

Patterns are first-class:
- Import from GitHub with versioning
- Apply to behaviors with parameters
- Compose (e.g., `CircuitBreaker<Retry<Op>>`)

### 2.7 Explicit Variable Declarations

Variables in behaviors should be declared explicitly with types and initial values. Heuristic inference (e.g., inferring `Nat` from `*count` naming patterns) is provided for prototyping but should not be relied upon for production specifications.

---

## 3. Core Abstractions

| Abstraction | Purpose | Transpiles to |
|-------------|---------|---------------|
| **System** | Hierarchical container | Module structure |
| **Component** | Layer, subsystem, or module | Varies by kind |
| **Behavior** | State machine | TLA+ spec |
| **Pattern** | Reusable behavior | TLA+ template |
| **Constraint** | Structural rules | Static analysis |
| **Invariant** | State assertions | TLA+ predicates |

---

## 4. Architecture

```
+----------------------------------------------------------+
|  CLI (main.rs)                                           |
|  commands: check, structural, lint, compile, verify,     |
|            rationale, plan, skeleton                     |
+------------------+---------------------------------------+
                   |
      +------------+------------+------------+
      |            |            |            |
+-----v-----+ +----v----+ +-----v------+ +---v---+
| Structural| |Behavioral| |Non-Func   | |Linter |
| Verif.    | |Compile  | |Extract    | |       |
| (syn)     | |(TLA+)   | |(config)   | |       |
+-----+-----+ +----+----+ +-----+------+ +---+---+
      |            |            |            |
+-----v------------v------------v------------v-----+
|  Parser & AST (lalrpop)                          |
|  - Full language grammar                         |
|  - TLA+ expression primitives                    |
|  - Temporal properties with cardinality          |
+--------------------------------------------------+
```

### 4.1 Parser Layer

LR(1) parser generated by `lalrpop`. Grammar is self-documenting in `intent.lalrpop`.

### 4.2 Structural Verification

Single-pass constraint checking against a prebuilt code index using `syn`:
- Module tree from `mod` declarations
- Import and type reference maps
- Trait implementation index

### 4.3 Behavioral Compilation

Compiles behaviors to TLA+:
- `states` → `VARIABLES` + `Init`
- `transitions` → `Next` actions
- `property` → temporal formulas
- `applies Pattern` → module instantiation

### 4.4 Non-Functional Extraction

Extracts performance constraints to:
- Benchmark configurations
- CI gate thresholds
- Resource limit specifications

### 4.5 Linter

The linter provides comprehensive syntax and semantic checking:
- **Parse errors**: Invalid syntax, unexpected tokens
- **Semantic validation**: Undefined identifiers, invalid transitions, unreachable states
- **State machine checks**: Missing initial/terminal states, multiple initial states, terminal state transitions
- **Style checks**: Naming conventions (PascalCase for systems/components, snake_case for states/constraints)
- **Dead code detection**: Unused components, unreferenced entities

The linter is exposed via `intent lint` and is also run as part of `intent check`.

---

## 5. TLA+ Transpilation

See [LANGUAGE.md §15](LANGUAGE.md#15-tla-transpilation) for the complete mapping table.

### 5.1 What Transpiles

| Intent Construct | TLA+ Output |
|------------------|-------------|
| `behavior` states/transitions | Module with `Init`, `Next` |
| `property always(...)` | Temporal formulas |
| `invariant` | `TypeOK` predicates |
| `refines` | Refinement theorems |
| `applies Pattern` | Module instantiation |
| `choose(x, S, P)` | `CHOOSE x \in S : P` |
| `let_in { x = e } in (body)` | `LET x == e IN body` |
| `forall x in S: P` | `\A x \in S : P` |
| `exists x in S: P` | `\E x \in S : P` |
| `subset(S)` | `SUBSET S` |
| `rec { a: 1 }` | `[a |-> 1]` |
| `tuple(a, b)` | `<<a, b>>` |
| `fun(x, S, body)` | `[x \in S |-> body]` |
| `except(f, [i], v)` | `[f EXCEPT ![i] = v]` |

### 5.2 What Doesn't Transpile

| Intent Construct | Verification |
|------------------|--------------|
| `A.depends(B)` | Import graph analysis |
| `A.references(B)` | Type reference scan |
| `A.implements(T)` | Trait impl lookup |
| `p99(op) < X` | Benchmark assertion |

### 5.3 What Requires Hand-Written TLA+

- Complex liveness properties with `until`/`releases` operators
- Properties requiring TLC (not Apalache)
- Probabilistic/Bayesian properties
- Real-time constraints with hard deadlines
- Unbounded data structures

---

## 6. Implementation Decisions

### 6.1 syn for AST Analysis

- No compilation required – operates on source text
- Fast – ~1.5s for ~50k lines
- No type resolution – acceptable for architectural constraints

### 6.2 lalrpop for Parsing

- Grammar is authoritative syntax reference
- Built-in error recovery and location tracking
- LR(1) ensures unambiguous grammar

### 6.3 No Data Models

Intent does not include inline data model definitions. Data schemas should use:
- JSON Schema / Protobuf / GraphQL for definitions
- Intent references external schemas: `uses schema "schemas/transaction.json"`

**Rationale:** Data validation is a solved problem. Intent focuses on behavioral verification.

### 6.4 Component Unification

Components are unified without explicit `kind`:
- **Structural by default** – used for dependency constraints
- **Behavioral with `behavior`** – defines state machines that transpile to TLA+

The `order` property has been removed. Layering is expressed through explicit dependency constraints:

```intent
constraint layering {
    !Storage.depends([API, Domain])
    forall s in [Domain, API]: s.depends([Infra])
}
```

This is more flexible and explicit than implicit ordering.

### 6.6 Operator-Based Constraints

Constraints use predicates with logical operators:

| Old keyword syntax | Current predicate syntax |
|-------------------|--------------------------|
| `A must_not depend_on B` | `!A.depends(B)` |
| `A must_depend_on B` | `A.depends(B)` |
| `A must_reference B` | `A.references(B)` |
| `A must_implement T` | `A.implements(T)` |
| `X occur_only_in Y` | `forall x in X: Y.contains(x)` |

### 6.7 Predicate Semantics

Predicates have precisely defined semantics (see [LANGUAGE.md §5.5](LANGUAGE.md#55-predicate-definition)). Known limitations are documented, including scope restrictions and evaluation order constraints.

---

## 7. Extension Points

### 7.1 Adding New Predicates

User-defined predicates via the `predicate` keyword are macros that expand to constraint rules at parse time. Compiler predicates (built-in) are registered in Rust:

1. Add predicate function to `structural/predicates.rs`
2. Register in predicate dispatch table
3. Add tests

### 7.2 Adding New Patterns

1. Create pattern definition file or import from GitHub
2. Add TLA+ template if needed
3. Patterns are just parameterized behaviors – no compiler changes

### 7.3 Adding Language Connectors

Connector interface:
- `resolve_scope`: Map scope names to code entities
- `check_dependency`: Test entity A depends on entity B
- `find_references`: Find all references to an entity

TypeScript connector would use `ts-morph` or TypeScript compiler API.

---

## 8. Pattern Library

Standard patterns are built-in. Custom patterns can be:

1. **Defined locally:** `pattern Retry<Op> { ... }`
2. **Imported from GitHub:** `import pattern Saga from "github.com/org/patterns@v1.2"`
3. **Templates with implementation:** `import template Auth from "..." with { ... }`

Pattern versioning uses git refs (tags, branches, commits).

### 8.1 Import Security

Pattern imports from GitHub are a supply-chain surface. Guardrails:

**Integrity verification:**
- Imports are pinned to exact git refs (tags, commits, branches)
- Content hash is computed on first import and stored in lockfile (`intent.lock`)
- Subsequent imports verify hash matches; mismatch is an error

**Lockfile format:**
```toml
[[patterns]]
name = "Saga"
source = "github.com/org/intent-patterns@v1.2"
commit = "a1b2c3d4e5f6..."
sha256 = "abc123..."
fetched = "2026-02-15T10:30:00Z"
```

**Trust model:**
- No automatic updates; explicit `intent update` required
- `--frozen` flag in CI fails on any unlocked import
- Signature verification (planned): check GPG/sigstore signatures on releases

**Auditing:**
- `intent audit` shows all external dependencies and their sources
- Diff between pattern versions: `intent diff pattern Saga v1.1 v1.2`

---

## 9. Relationship to Existing Tools

| Tool | Relationship |
|------|--------------|
| **ArchUnit** | Similar structural goals. Intent adds behaviors, TLA+, distillation. |
| **dependency-cruiser** | Dependency-only. Intent covers behaviors and rationale. |
| **TLA+/Apalache** | Intent generates obligations for Apalache, doesn't replace TLA+. |
| **ADR** | Intent's rationale blocks are machine-readable ADRs. |

---

## 10. Current Status

### Implemented

- **Parser**: LR(1) parser with full language support (LALRPOP)
- **Structural verification**: Constraint checkers for `depends`, `references`, `implements`, `contains`
- **Behavioral compilation**: TLA+ generation from state machines
- **TLA+ expression primitives**: `choose`, `let_in`, `if/then/else`, `case`, `forall x in S: P`, `exists x in S: P`, `subset`, `union_all`, `domain_of`, `rec`, `tuple`, `set`, `fun`, `except`, `assume`
- **Temporal properties**: `always`, `eventually`, `next`, `until`, `releases`, `weak_until`, `strong_releases`
- **Fairness constraints**: Weak and strong fairness with alternatives
- **Cardinality properties**: `count(state)` for distributed system specs
- **Linter**: Syntax checking, semantic validation, style checks, dead code detection
- **Pattern library**: EventSourced, CircuitBreaker, Saga, Retry, Timeout, RateLimiter, Bulkhead, etc.
- **Rationale extraction**: Machine-readable decision records
- **Plan mode**: Validation without codebase

### Planned

- GitHub pattern/template import
- Import lockfile and integrity verification
- Distillation with confidence scoring
- Drift detection
- TypeScript connector
- Skeleton code generation

### Non-goals

- **Code generation** – Intent constrains; it does not generate
- **Runtime verification** – Intent operates at build time
- **Full temporal logic** – Complex properties belong in hand-written TLA+
- **Data validation** – Use JSON Schema, Protobuf, etc.
