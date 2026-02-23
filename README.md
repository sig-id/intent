# Intent

[![Crates.io](https://img.shields.io/crates/v/intent.svg)](https://crates.io/crates/intent)
[![License](https://img.shields.io/crates/l/intent.svg)](https://crates.io/crates/intent)

**Architecture as code. Verification as a build step.**

Intent is a domain-specific language for expressing and verifying architectural constraints. It bridges the gap between design intent and implementation reality through three complementary approaches:

- **Static analysis** — verify dependency graphs, layering rules, and module boundaries against your actual codebase
- **Formal verification** — compile behavioral specifications to TLA+ for model checking with Apalache
- **Design rationale** — capture decisions, alternatives, and reasoning in machine-readable format

## Why Intent?

Architectural decisions are made once, then slowly eroded. Code reviews catch some violations, but many slip through:

```
"Services must not depend on storage"     → buried in ADRs, violated in PRs
"Payment state machine must be deadlock-free" → who checks this?
"We chose circuit breakers over retries"  → new hire adds retry logic anyway
```

Intent makes architecture **executable**. Write constraints, run verification in CI, get violations before merge.

## Quick Example

```intent
system PaymentPlatform {
    description "Payment processing with guaranteed settlement"

    components [Ingestion, Processing, Settlement]

    component Processing {
        implements "crates/processing/src"
        depends_only [StorageAPI, EventQueue]

        behavior TransactionLifecycle {
            states {
                pending   { initial: true }
                processing
                settled   { terminal: true }
                failed    { terminal: true }
            }

            transitions {
                pending -> processing on receive
                processing -> settled on confirm
                processing -> failed on timeout
            }

            property eventual_settlement {
                always(pending => eventually(settled | failed))
            }
        }
    }

    component API {
        contains [routes, handlers]
        depends_only [Processing]
    }

    // Layering: storage layer must not depend on business logic
    constraint layering {
        !Storage.depends([API, Processing])
        forall s in services: s.references([AppError])
    }

    // Why circuit breakers? Document it.
    rationale ResilienceStrategy {
        decided because {
            "Circuit breakers fail fast, preventing cascade failures."
            "Retries cause request pileup under load."
        }
        rejected {
            retry_only: "Amplifies load during outages."
        }
        revisit when {
            "Downstream services have guaranteed response times."
        }
    }
}
```

Verify against your codebase:

```bash
$ intent check intent/ --codebase src/

=== Phase 1: Structural verification ===
  [PASS] layering
  [PASS] error_handling

=== Phase 2: Behavioral compilation ===
  generated: formal/tla/obligations/Processing_TransactionLifecycle.tla

=== Phase 3: Obligation verification ===
  [PASS] EventSourced -> Processing

=== Phase 4: Rationale extraction ===
  written: intent/rationale.json

All checks passed.
```

## Features

**Structural constraints** (verified via static analysis):
- `A.depends(B)` — A imports/uses B
- `A.references(B)` — A mentions type B
- `A.implements(T)` — A implements trait T
- `A.contains(B)` — B is nested within A
- Logical operators: `!`, `&&`, `||`, `=>`, `<=>`
- Quantifiers: `forall`, `exists`
- Comparison expressions: `check x <= y`

**Behavioral specs** (compiled to TLA+ for Apalache verification):
- State machines with transitions, guards, and effects
- Temporal properties: `always`, `eventually`, `next`, `until`, `releases`
- Fairness constraints: `weak`, `strong`
- Cardinality properties: `count(state) <= N`
- Reusable patterns: Saga, CircuitBreaker, Retry, etc.

**TLA+ expression primitives** (for formal invariants):
- `choose(x, S, P)` — CHOOSE operator
- `let_in { x = e } in (body)` — LET-IN bindings
- `if/then/else`, `case` — conditionals
- `forall x in S: P`, `exists x in S: P` — quantifiers in expressions
- `subset`, `union_all`, `domain_of` — set operations
- `rec`, `tuple`, `set` — data structure literals
- `fun`, `except` — function literals and updates
- `assume` — model checking assumptions

**Linter**:
- Syntax error detection
- Semantic validation (unreachable states, invalid transitions)
- Style checks (naming conventions, missing descriptions)
- Dead code detection

**Pattern reuse**:
- Import patterns from GitHub with versioning
- Apply patterns to behaviors with parameters
- Standard library: EventSourced, CircuitBreaker, Saga, Retry, etc.

**Design rationale** (machine-readable annotations):
- `decided because { "reason" }`
- `rejected { alt: "reason" }`
- `revisit when { "condition" }`
- Structured decision records with confidence levels

## Installation

```bash
cargo install intent
```

## Usage

```bash
# Full verification (structural + behavioral + rationale)
intent check intent/ --codebase src/

# Structural constraint verification only
intent structural intent/ --codebase src/

# Lint intent files for syntax and style issues
intent lint intent/

# Lint with pedantic checks and hints
intent lint intent/ --pedantic --hints

# Compile to TLA+
intent compile intent/ --output formal/generated/

# Verify TLA+ obligations with Apalache
intent verify --obligations formal/generated/

# Extract rationale
intent rationale intent/ --output rationale.json

# Plan-mode validation (no codebase required)
intent plan intent/

# JSON output (works with all commands)
intent check intent/ --codebase src/ --format json
```

## Documentation

- [LANGUAGE.md](LANGUAGE.md) — Complete language specification
- [DESIGN.md](DESIGN.md) — Design rationale and architecture

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
