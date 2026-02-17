# Intent

[![Crates.io](https://img.shields.io/crates/v/intent.svg)](https://crates.io/crates/intent)
[![License](https://img.shields.io/crates/l/intent.svg)](https://crates.io/crates/intent)

Machine-verifiable architectural design constraints for codebases.

Intent is a minimal domain-specific language that transpiles to TLA+ for formal verification while enabling structural analysis of codebases.

## The Problem

Architectural constraints live in documentation or engineers' heads. When code changes, nothing verifies that architectural invariants still hold:

- "Services must not access storage backends directly" — no test catches a violation
- "Auth middleware only belongs in the routes layer" — nothing enforces this
- "We chose circuit breakers over retries for resilience" — the reasoning is lost

## The Solution

Write constraints in `.intent` files:

```intent
system PaymentPlatform {
    components [Ingestion, Processing, Settlement]

    component Processing {
        kind: subsystem
        behavior TransactionLifecycle { ... }
    }

    component API {
        kind: layer
        contains [routes, handlers]
    }

    constraint isolation {
        !depends(Processing, storage_backends)
        references(Processing, [AppError])
    }
}
```

Then verify against your codebase:

```bash
intent check intent/ --codebase src/
```

## Features

**Structural constraints** (verified via static analysis):
- `depends(A, B)` — A imports/uses B
- `references(A, B)` — A mentions type B
- `implements(A, T)` — A implements trait T
- Logical operators: `!`, `&&`, `||`, `=>`
- Quantifiers: `forall`, `exists`

**Behavioral specs** (compiled to TLA+ for Apalache verification):
- State machines with transitions and effects
- Temporal properties: `always`, `eventually`
- Reusable patterns: Saga, CircuitBreaker, Retry

**Pattern reuse**:
- Import patterns from GitHub with versioning
- Apply patterns to behaviors with parameters

**Design rationale** (machine-readable annotations):
- `decided because { "reason" }`
- `rejected { alt: "reason" }`
- `revisit when { "condition" }`

## Installation

```bash
cargo install intent
```

## Usage

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

## Documentation

- [LANGUAGE.md](LANGUAGE.md) — Complete language specification
- [DESIGN.md](DESIGN.md) — Design rationale and architecture

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
