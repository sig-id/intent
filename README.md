# Intent

[![Crates.io](https://img.shields.io/crates/v/intent.svg)](https://crates.io/crates/intent)
[![License](https://img.shields.io/crates/l/intent.svg)](https://crates.io/crates/intent)

**Architecture as code. Verification as a build step.**

Intent is a language and CLI tool for writing architectural constraints that are checked automatically – against your actual codebase with static analysis, and against state machine models with TLA+ formal verification.

## The problem

Architectural decisions rot. They start as a sentence in a design doc, get restated in a code review, then slowly erode as the team grows:

```
"Services must not depend on storage"     → violated three PRs later
"Payment flow must reach settlement"      → nobody model-checks this
"We chose circuit breakers over retries"  → new hire adds retry logic
```

Linters catch style. Tests catch behavior. Nothing catches architecture – unless you make it executable.

## What Intent does

You write `.intent` files that describe your system's structure and behavior. Intent verifies them:

1. **Structural constraints** are checked against your source code using static analysis (`syn` for Rust and regex-based analysis for TypeScript/JavaScript). These enforce layering, dependency boundaries, and module containment rules.

2. **Behavioral specifications** are compiled to TLA+ and model-checked with [Apalache](https://apalache-mc.org/) or TLC. These verify state machine properties like deadlock freedom, liveness, and safety invariants.

3. **Design rationale** is captured alongside constraints in a machine-readable format, producing structured decision records you can query and audit.

## Quick start

```bash
cargo install intent
```

Write a spec (`system.intent`):

```intent
system PaymentPlatform {
    description "Payment processing with guaranteed settlement"

    component Processing {
        implements "crates/processing/src"
        depends_only [StorageAPI, EventQueue]

        behavior TransactionLifecycle {
            states {
                pending    { initial: true }
                processing
                settled    { terminal: true }
                failed     { terminal: true }
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

    constraint layering {
        !Storage.depends([API, Processing])
    }
}
```

Run it:

```bash
# Lint for syntax/semantic errors (no codebase needed)
intent lint system.intent

# Check structural constraints against source
intent structural system.intent --codebase src/

# Compile behaviors to TLA+ and verify with Apalache
intent compile system.intent --output out/
intent verify --obligations out/
```

Or run everything at once:

```bash
intent check system.intent --codebase src/
```

## CLI commands

| Command | Purpose |
|---------|---------|
| `intent check` | Run all phases: structural, compile, verify, rationale |
| `intent structural` | Structural constraint verification against source code |
| `intent lint` | Syntax checking, semantic validation, style lints |
| `intent compile` | Generate TLA+ modules from behavioral specs |
| `intent verify` | Model-check generated TLA+ with Apalache or TLC |
| `intent rationale` | Extract decision records as JSON |
| `intent plan` | Validate specs without a codebase (design phase) |
| `intent extract-benchmarks` | Export non-functional constraints as benchmark config |

All commands support `--format json` for CI integration.

## Structural constraints

Constraints are checked against your codebase using `syn`-based static analysis:

```intent
constraint architecture {
    // Dependency rules
    !Storage.depends([API, Domain])
    Processing.depends_only([StorageAPI, EventQueue])

    // Type reference rules
    forall s in [ServiceA, ServiceB]: s.references([AppError])

    // Trait implementation checks
    forall repo in [UserRepo, OrderRepo]: repo.implements([Repository])

    // Module containment
    API.contains([routes, handlers])
}
```

Built-in predicates: `depends`, `depends_transitively`, `references`, `implements`, `contains` – plus negated forms. Combine with `forall`, `exists`, `&&`, `||`, `=>`, `<=>`, and `!`.

## Behavioral specifications

State machines compile to TLA+ for formal verification:

```intent
behavior OrderSaga {
    variables {
        retries: Nat = 0
    }

    states {
        created   { initial: true }
        reserved
        charged
        completed { terminal: true }
        cancelled { terminal: true }
    }

    transitions {
        created -> reserved on reserve_inventory
        reserved -> charged on process_payment
            where { retries < 3 }
            effect { retries = retries + 1 }
        charged -> completed on confirm
        * -> cancelled on cancel
    }

    property safety {
        always(completed => !eventually(cancelled))
    }

    property liveness {
        always(created => eventually(completed | cancelled))
    }

    fairness {
        weak(reserved -> charged | cancelled)
        strong(charged -> completed | cancelled)
    }
}
```

Temporal operators: `always`, `eventually`, `next`, `until`, `releases`, `weak_until`, `strong_releases`. The transpiler targets Apalache by default; `until`/`releases` variants require TLC (`--mode exhaustive`).

## Refinement bridge (grounding)

A compiled behavior captures a state-machine **shape**, but its guards are uninterpreted booleans. A `grounding` block links that shape *down* to a hand-written **detailed** TLA+ module that carries the real logic, and checks the detailed model refines the shape:

```intent
behavior LoginFlow {
    variables { password_valid: Bool = false  account_active: Bool = false }
    states { idle { initial: true }  verifying  authenticated { terminal: true }  denied { terminal: true } }
    transitions {
        idle -> verifying on submit
        verifying -> authenticated on ok   where { password_valid && account_active }
        verifying -> denied on deny         where { !password_valid || !account_active }
    }

    grounding "AuthDetailed" from "AuthDetailed.tla" {
        state          -> "AbsLoginState"   // abstraction function
        password_valid -> "pw_ok"           // grounds a guard atom
        account_active -> "acct_active"
    }
}
```

The optional `from "<path>"` points at the hand-written detailed module (it can live anywhere under the spec tree); `intent compile` co-locates it with the harness so `intent verify` runs the refinement check automatically. Compiling emits a refinement harness (`LoginFlow_Refinement.tla`) that `EXTENDS` the detailed module and asserts — as an **Apalache action invariant** (`Inv_Refinement`) — that every detailed step projects onto an abstract FSM step or a stutter, plus an obligations manifest (`LoginFlow.obligations.json`) tracking which guard atoms are grounded. `intent verify` reports obligation coverage and fails on any unmet obligation — an ungrounded guard would make the check vacuous. The check runs under Apalache and emits **ITF** counterexamples (`apalache-mc check --inv=Inv_Refinement --output-traces ...`), so a refinement violation can be replayed against the implementation; the temporal/liveness form (which would require TLC and forgo ITF) is left opt-in. See [`examples/refinement/`](examples/refinement/) for a complete, model-checkable example, and [LANGUAGE.md §11.3](LANGUAGE.md#113-grounding--linking-an-abstract-shape-to-a-detailed-model) for details.

## Pattern library

Built-in patterns can be applied to behaviors without imports:

```intent
behavior OrderProcessor {
    applies EventSourced {
        subscribes: [OrderCreated, PaymentCompleted]
        emits: [ReserveInventory, ShipOrder]
    }

    applies Timeout {
        deadline: 30m
        fallback_state: "cancelled"
    }
}
```

Available patterns: `EventSourced`, `Timeout`, `Scoped`, `Retry`, `CircuitBreaker`, `Saga`, `ProcessManager`, `RateLimiter`, `Bulkhead`, `CompensatingTransaction`, `OptimisticLocking`.

## Design rationale

Capture architectural decisions alongside the constraints they justify:

```intent
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
```

Extract as JSON with `intent rationale system.intent --output decisions.json`.

## TLA+ expression primitives

For formal invariants, Intent supports TLA+-style expressions that transpile directly:

```intent
invariant worker_assignment {
    choose(worker, Workers, worker.status == "healthy")
}

invariant price_calculation {
    let_in { base = 100, discount = 10 } in (base - discount)
}

invariant valid_orders {
    forall order in Orders: (order.amount > 0)
}
```

Full set: `choose`, `let_in`, `if/then/else`, `case`, `forall`, `exists`, `subset`, `union_all`, `domain_of`, `rec`, `tuple`, `set`, `fun`, `except`, `assume`.

## Verification modes

```bash
# Bounded model checking with Apalache (default, fast)
intent verify --obligations out/

# Exhaustive state space with TLC
intent verify --obligations out/ --mode exhaustive

# Both
intent verify --obligations out/ --mode both

# Temporal property checking (requires TLC)
intent verify --obligations out/ --temporal --mode exhaustive
```

## Project layout

A typical project using Intent:

```
my-project/
  src/                    # Your Rust source code
  intent/
    system.intent         # System spec with constraints and behaviors
  out/                    # Generated TLA+ (from intent compile)
```

## Current status

Intent is under active development. What works today:

- Structural constraint checking for Rust codebases (`syn`-based analysis)
- Structural constraint checking for TypeScript/JavaScript codebases (import/reference/interface checks)
- Behavioral spec compilation to TLA+ with Apalache/TLC verification
- Comprehensive linter (21 rules covering syntax, semantics, style, and dead code)
- Pattern application from the built-in standard library
- Design rationale extraction
- Hindley-Milner type inference for pattern parameters

Not yet implemented in the CLI:

- Remote pattern imports (`import pattern X from "github.com/..."` parses but does not resolve)
- Built-in distillation engine (the `distilled` keyword is parsed for forward compatibility)

**Distillation** – extracting Intent specs from existing codebases – is available as an external tool: [intent-distill](https://github.com/wiggum-cc/chief-wiggum/blob/main/skills/intent-distill/SKILL.md). It analyzes source code to identify architectural patterns, dependency constraints, and behavioral state machines, then generates `.intent` files validated with `intent lint`.

See [LANGUAGE.md](LANGUAGE.md) for the full language specification and [DESIGN.md](DESIGN.md) for architecture decisions.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
