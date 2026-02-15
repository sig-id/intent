# Intent Language Specification

**Version:** 0.2.0
**Status:** Implemented
**Updated:** 2026-02-15
**Grammar:** [`src/parser/intent.lalrpop`](src/parser/intent.lalrpop)

---

## 1. Overview

Intent is a domain-specific language for expressing machine-verifiable architectural design constraints. An Intent specification consists of one or more `.intent` files, each declaring a single **concern** -- a named architectural decision with its structural constraints, behavioral obligations, and design rationale.

Intent operates at the architectural level. It does not replace prose specifications, formal models (TLA+/Quint), or implementation-level contracts. It captures the machine-checkable subset of architectural intent that sits between prose and formal models.

---

## 2. Lexical Structure

### 2.1 Character Set

Intent source files are UTF-8 encoded. The grammar operates on ASCII keywords and identifiers; string literals may contain arbitrary UTF-8.

### 2.2 Whitespace and Comments

Whitespace (spaces, tabs, newlines) is insignificant except within string literals. Line comments begin with `//` and extend to the end of the line.

```intent
// This is a comment
concern Example {
    // Comments can appear anywhere whitespace is allowed
}
```

### 2.3 Identifiers

```
IDENT = [a-zA-Z_][a-zA-Z0-9_]*
```

Identifiers name concerns, scopes, constraints, layers, patterns, parameters, and code entities. They are case-sensitive.

### 2.4 Glob Patterns

```
PREFIX_GLOB = *[a-zA-Z0-9_]+     // e.g., *Client
SUFFIX_GLOB = [a-zA-Z_][a-zA-Z0-9_]*\*    // e.g., Dgraph*
```

Glob patterns match code entity names. `*Client` matches any entity ending in `Client`. `Dgraph*` matches any entity starting with `Dgraph`. Globs are expanded against the code index at verification time using regex matching.

### 2.5 Literals

| Literal | Syntax | Examples |
|---------|--------|----------|
| Integer | `[0-9]+` | `5`, `100`, `0` |
| Float | `[0-9]+\.[0-9]+` | `0.03`, `1.5` |
| Percent | `[0-9]+(\.[0-9]+)?%` | `5%`, `2.5%` |
| Duration | `[0-9]+[smhd]` | `30s`, `5m`, `2h`, `7d` |
| String | `"[^"]*"` | `"reason text"`, `"formal/tla/Spec.tla"` |

Duration literals support seconds (`s`), minutes (`m`), hours (`h`), and days (`d`).

### 2.6 Keywords

The following identifiers are reserved keywords:

```
concern    scope       constraint   layer       apply       to
refines    only        accesses     must_not    depend_on   reference
occur_only_in          must_depend_on           must_reference
must_implement         decided      because     rejected    alternatives
revisit    when        within       use
let        predicate   forall      exists      in          matches
depends_on references
```

---

## 3. Grammar

### 3.1 File Structure

A file contains zero or more concern declarations.

```
File = Concern*
```

### 3.2 Concern

The top-level unit of specification.

```
Concern = "concern" IDENT "{" ConcernItem* "}"
```

A concern groups related scopes, constraints, behavioral obligations, and rationale under a single name. Each concern is independently parseable and verifiable.

**Example:**

```intent
concern ResilientStorage {
    // scopes, constraints, applies, rationale...
}
```

### 3.3 Concern Items

Items that may appear inside a concern block:

```
ConcernItem = Scope
            | Constraint
            | Layer
            | Apply
            | DecidedBecause
            | RejectedAlternatives
            | RevisitWhen
            | UseScope
            | Let
            | Predicate
```

Items may appear in any order. There is no required ordering between scopes, constraints, and rationale blocks.

---

## 4. Scope Declarations

Scopes define named sets of code entities that constraints reference.

```
Scope = "scope" IDENT "{" ScopeBody ("within" IdentList)? "}"
```

### 4.1 Entity List Scope

Names a set of entities directly.

```
ScopeBody = IdentList
```

**Syntax:** `scope <name> { [<entity>, ...] }`

**Example:**

```intent
scope storage_backends {
    [DgraphClient, MilvusClient]
}
```

### 4.2 Access Boundary Scope

Declares that only listed entities may access a target scope.

```
ScopeBody = "only" IdentList "accesses" IDENT
```

**Syntax:** `scope <name> { only [<accessor>, ...] accesses <target> }`

**Example:**

```intent
scope storage_boundary {
    only [StorageCoordinator] accesses storage_backends
}
```

**Semantics:** This generates verification that no entity outside the accessor list has dependencies on entities in the target scope. The target may be a scope name or a literal entity name.

### 4.3 Module Restriction

The optional `within` clause restricts the scope to specific modules.

```intent
scope backends {
    [DgraphClient]
    within [storage, pipeline]
}
```

### 4.4 Scope References

Scopes can be referenced by name in constraint rules. When a bare identifier appears where an entity list is expected, it is resolved as a scope name first, then as a literal entity name.

```intent
scope processing {
    [services, pipeline, rag]
}

constraint no_leak {
    processing must_not depend_on storage_backends
    //  ^-- resolves to [services, pipeline, rag]
}
```

### 4.5 Cross-Concern References

Scopes from other concerns can be imported with `use`:

```
UseScope = "use" IDENT "." IDENT
```

**Example:**

```intent
concern ExtendedChecks {
    use ResilientStorage.storage_backends

    constraint extra {
        [chat] must_not depend_on storage_backends
    }
}
```

### 4.6 Set Expressions

Set expressions enable compositional scope definitions using set algebra operators.

```
ScopeExpr = "[" EntityName ("," EntityName)* "]"    // entity list
          | IDENT                                    // identifier (scope or entity)
          | PREFIX_GLOB                              // *Client
          | SUFFIX_GLOB                              // Dgraph*
          | ScopeExpr "|" ScopeExpr                  // union
          | ScopeExpr "&" ScopeExpr                  // intersection (higher precedence)
          | ScopeExpr "\" ScopeExpr                  // difference
          | "{" IDENT "|" IDENT "matches" Pattern "}"  // comprehension
          | "(" ScopeExpr ")"                        // parenthesized
```

**Operator precedence** (highest to lowest):
1. `&` (intersection)
2. `|` (union)
3. `\` (difference)

**Examples:**

```intent
let backends = [DgraphClient, MilvusClient]
let cache = [RedisClient]
let external = backends | cache  // union: all backends and cache
let core = services & pipeline   // intersection: entities in both
let safe = core \ test_helpers   // difference: core without test helpers
let clients = { e | e matches *Client }  // all entities ending in "Client"
```

### 4.7 Let Bindings

Let bindings define named scope expressions that can be referenced in constraints and other let bindings.

```
Let = "let" IDENT "=" ScopeExpr
```

Bindings are visible within the current concern, similar to scope declarations. The name is resolved in constraint rules before checking for scope names or entity names.

**Examples:**

```intent
let backends = [DgraphClient, MilvusClient]
let external = backends | cache
let core = services \ test_helpers
let clients = { e | e matches *Client }

constraint isolation {
    core must_not depend_on external
    clients occur_only_in [storage]
}
```

---

## 5. Constraint Declarations

Constraints assert properties of code structure. Each constraint contains one or more rules.

```
Constraint = "constraint" IDENT "{" ConstraintRule+ "}"
```

**Example:**

```intent
constraint no_direct_backend_access {
    [services, pipeline] must_not depend_on storage_backends
    AuthMiddleware occur_only_in [routes]
}
```

### 5.1 Entity References

Constraint rules reference code entities via entity refs:

```
EntityRef = "[" EntityName ("," EntityName)* "]"    // bracket list
          | IDENT                                    // bare name (scope ref or entity)
          | PREFIX_GLOB                              // *Client
          | SUFFIX_GLOB                              // Dgraph*

EntityName = IDENT | PREFIX_GLOB | SUFFIX_GLOB
```

A bare `IDENT` is resolved as a scope name first (expanding to the scope's entity list), then as a literal module/type name. Glob patterns are expanded against the code entity index.

### 5.2 Constraint Rules

#### `must_not depend_on`

```
EntityRef "must_not" "depend_on" EntityName
```

Asserts that no entity in the `from` set has import/use dependencies on the target. The target is resolved as a scope name or entity name.

**Semantics:** For each module `m` in `from`, verify that `m` does not import, use, or reference any entity in the target set.

```intent
[services, pipeline] must_not depend_on storage_backends
```

#### `must_not reference`

```
EntityRef "must_not" "reference" EntityRef
```

Asserts that no entity in the `from` set references (by type or call) any entity in the target set.

**Semantics:** For each module `m` in `from`, verify that `m` does not contain type paths or call expressions naming any entity in targets.

```intent
[services, storage] must_not reference [AuthMiddleware, SessionCookie]
```

#### `must_depend_on`

```
EntityRef "must_depend_on" EntityName
```

Asserts that at least one entity in the `from` set depends on the target. Used to verify required dependencies exist.

```intent
[services] must_depend_on storage
```

#### `must_reference`

```
EntityRef "must_reference" EntityRef
```

Asserts that at least one entity in the `from` set references at least one entity in the target set.

```intent
[services] must_reference [AppError, Result]
```

#### `occur_only_in`

```
EntityName "occur_only_in" EntityRef
```

Asserts that a code entity (type, function, or glob pattern) appears only in the listed modules. If found elsewhere, a violation is reported.

```intent
AuthMiddleware occur_only_in [routes]
*Client occur_only_in [storage]
```

#### `must_implement`

```
IDENT "must_implement" IDENT
```

Asserts that a type implements a specific trait. Verified by checking for `impl TraitName for TypeName` in the code index.

```intent
DgraphClient must_implement GraphStore
MilvusClient must_implement VectorStore
```

### 5.3 Quantifiers (forall / exists)

Quantifiers allow constraints to range over sets of entities.

```
"forall" IDENT "in" ScopeExpr ":" ConstraintRule
"forall" IDENT "in" ScopeExpr "{" ConstraintRule+ "}"
"exists" IDENT "in" ScopeExpr ":" ConstraintRule
"exists" IDENT "in" ScopeExpr "{" ConstraintRule+ "}"
```

**Semantics:**
- `forall x in S: P(x)` — For all entities `x` in set `S`, constraint `P(x)` must hold
- `exists x in S: P(x)` — At least one entity `x` in set `S` must satisfy constraint `P(x)`

The quantified variable can be used in constraint rules where an entity reference is expected.

**Examples:**

```intent
// Every service must reference AppError
forall s in services: s must_reference [AppError]

// At least one service must depend on logging
exists s in services: s must_depend_on logging

// Multiple constraints per quantifier (block form)
forall m in [services, pipeline] {
    m must_not depend_on external
    m must_reference [Result]
}
```

### 5.4 Implication (=>)

Implication constraints express conditional obligations: if a condition holds, then a constraint must be satisfied.

```
IDENT "depends_on" EntityName "=>" ConstraintRule
IDENT "references" EntityName "=>" ConstraintRule
```

**Semantics:**
- `x depends_on B => P(x)` — If entity `x` depends on `B`, then constraint `P(x)` must hold
- `x references B => P(x)` — If entity `x` references `B`, then constraint `P(x)` must hold

Typically used inside quantifiers to express conditional constraints over sets.

**Example:**

```intent
// If a module depends on cache, it must also depend on cache_invalidation
forall m in services:
    m depends_on cache => m must_depend_on cache_invalidation
```

### 5.5 Predicate Definitions

Predicates define reusable constraint patterns that can be invoked with arguments.

```
Predicate = "predicate" IDENT "(" IDENT ("," IDENT)* ")" "{" ConstraintRule+ "}"
```

**Semantics:** A predicate defines a named constraint pattern with formal parameters. When called, the parameters are substituted with actual scope expressions.

**Example:**

```intent
predicate isolated(source, target) {
    source must_not depend_on target
    source must_not reference target
}
```

### 5.6 Predicate Calls

Predicates are invoked within constraints by name with actual arguments.

```
IDENT "(" ScopeExpr ("," ScopeExpr)* ")"
```

**Semantics:** The predicate body is instantiated with actual arguments substituted for formal parameters, generating the corresponding constraint rules.

**Example:**

```intent
constraint boundaries {
    isolated(services, storage_backends)
    isolated(pipeline | rag, auth)
}
```

This expands to:
```intent
constraint boundaries {
    services must_not depend_on storage_backends
    services must_not reference storage_backends
    (pipeline | rag) must_not depend_on auth
    (pipeline | rag) must_not reference auth
}
```

---

## 6. Layer Declarations

Layers declare ordered architecture layers. The declaration order defines the dependency direction: layers listed first are higher (presentation), layers listed last are lower (infrastructure).

```
Layer = "layer" IDENT "{" EntityRef "}"
```

**Example:**

```intent
layer presentation { [routes] }
layer application { [services] }
layer processing { [pipeline, segmentation, rag, community, knowledge] }
layer infrastructure { [storage] }
```

**Implicit constraints:** Layers generate `must_not depend_on` constraints between all pairs where a lower layer would depend on a higher layer. Given layers L1, L2, ..., Ln (top to bottom):

- For every pair (Li, Lj) where i < j: `Lj.entities must_not depend_on Li.entities`

In the example above, this generates:
- `[services] must_not depend_on [routes]`
- `[pipeline, segmentation, rag, community, knowledge] must_not depend_on [routes]`
- `[pipeline, segmentation, rag, community, knowledge] must_not depend_on [services]`
- `[storage] must_not depend_on [routes]`
- `[storage] must_not depend_on [services]`
- `[storage] must_not depend_on [pipeline, segmentation, rag, community, knowledge]`

---

## 7. Pattern Applications

Pattern applications declare that a component implements a known architectural pattern with specific parameters, and that the pattern's behavioral properties are verified against a formal model.

```
Apply = "apply" IDENT Params "to" DottedName ("{" "refines" STRING "}")?
Params = "(" Param ("," Param)* ")"
Param = IDENT ":" Value
Value = INT | DURATION | STRING
DottedName = IDENT ("." IDENT)*
```

**Example:**

```intent
apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
    to StorageCoordinator.dgraph_circuit_breaker {
        refines "formal/tla/CircuitBreaker.tla"
    }
```

### 7.1 Parameters

| Type | Syntax | Example |
|------|--------|---------|
| Integer | `N` | `threshold: 5` |
| Duration | `Ns` | `timeout: 30s` |
| String | `"..."` | `name: "dgraph"` |

### 7.2 Target

The target is a dotted name identifying the component and field/method that implements the pattern. For example, `StorageCoordinator.dgraph_circuit_breaker` identifies the `dgraph_circuit_breaker` field on the `StorageCoordinator` type.

### 7.3 Refines Clause

The optional `refines` clause references a TLA+ specification file. The behavioral compiler generates a TLA+ obligation module that `EXTENDS` the referenced spec and asserts the pattern's invariants with the given parameter values.

If the `refines` clause is omitted, no behavioral obligation is generated -- only structural verification that the target component exists.

### 7.4 Compilation

A pattern application with `refines` compiles to a TLA+ obligation module:

```tla
---- MODULE Obligation_ResilientStorage ----
EXTENDS CircuitBreaker

VARIABLES cb_state, failure_count, consecutive_successes, elapsed

ConstInit ==
    /\ cb_state = "Closed"
    /\ failure_count = 0
    /\ consecutive_successes = 0
    /\ elapsed = 0

INSTANCE CircuitBreaker WITH
    FAILURE_THRESHOLD <- 5,
    RECOVERY_TIMEOUT <- 0,
    PROBE_SUCCESS_LIMIT <- 2

PatternInv_OpenRequiresThreshold ==
    cb_state = "Open" => failure_count >= 5

PatternInv_OpenRejects ==
    cb_state = "Open" => TRUE

PatternInv_ClosedBelowThreshold ==
    cb_state = "Closed" => failure_count < 5

PatternObligation ==
    /\ PatternInv_OpenRequiresThreshold
    /\ PatternInv_OpenRejects
    /\ PatternInv_ClosedBelowThreshold
====
```

This obligation is verified by invoking:

```
apalache-mc check --cinit=ConstInit --inv=PatternObligation Obligation_ResilientStorage.tla
```

---

## 8. Rationale Annotations

Rationale blocks attach design decisions and reasoning to a concern. They are not mechanically verified but are machine-readable for agent consumption.

### 8.1 Decided Because

```
DecidedBecause = "decided" "because" "{" STRING+ "}"
```

Natural-language reasons for the design decision. Each string is one reason.

```intent
decided because {
    "Dgraph and Milvus are external dependencies with independent failure modes."
    "Circuit breakers prevent cascading failures."
}
```

### 8.2 Rejected Alternatives

```
RejectedAlternatives = "rejected" "alternatives" "{" (IDENT ":" STRING)+ "}"
```

Documents what was considered and rejected, with rationale. Prevents agents from re-proposing already-rejected approaches.

```intent
rejected alternatives {
    retry_only: "Retries without circuit breaking cause request pileup during outages."
    failover_to_replica: "Neither Dgraph nor Milvus runs replicas in current deployment."
}
```

### 8.3 Revisit When

```
RevisitWhen = "revisit" "when" "{" STRING+ "}"
```

Invalidation triggers. If codebase changes match a trigger condition, the concern is flagged for review.

```intent
revisit when {
    "Dgraph or Milvus runs in a replicated HA configuration"
    "A third storage backend is added"
}
```

---

## 9. Formal Grammar (EBNF)

```ebnf
File           = { Concern } ;

Concern        = "concern" IDENT "{" { ConcernItem } "}" ;

ConcernItem    = Scope
               | Constraint
               | Layer
               | Apply
               | DecidedBecause
               | RejectedAlternatives
               | RevisitWhen
               | UseScope
               | Let
               | Predicate ;

Scope          = "scope" IDENT "{" ScopeBody [ "within" IdentList ] "}" ;
ScopeBody      = "only" IdentList "accesses" IDENT
               | IdentList ;

Layer          = "layer" IDENT "{" EntityRef "}" ;

Let            = "let" IDENT "=" ScopeExpr ;

Predicate      = "predicate" IDENT "(" IDENT { "," IDENT } ")" "{"
                 ConstraintRule { ConstraintRule } "}" ;

Constraint     = "constraint" IDENT "{" ConstraintRule { ConstraintRule } "}" ;

ConstraintRule = EntityRef "must_not" "depend_on" EntityName
               | EntityRef "must_not" "reference" EntityRef
               | EntityRef "must_depend_on" EntityName
               | EntityRef "must_reference" EntityRef
               | EntityName "occur_only_in" EntityRef
               | IDENT "must_implement" IDENT
               | "forall" IDENT "in" ScopeExpr ":" ConstraintRule
               | "forall" IDENT "in" ScopeExpr "{" ConstraintRule { ConstraintRule } "}"
               | "exists" IDENT "in" ScopeExpr ":" ConstraintRule
               | "exists" IDENT "in" ScopeExpr "{" ConstraintRule { ConstraintRule } "}"
               | IDENT "depends_on" EntityName "=>" ConstraintRule
               | IDENT "references" EntityName "=>" ConstraintRule
               | IDENT "(" ScopeExpr { "," ScopeExpr } ")" ;

Apply          = "apply" IDENT Params "to" DottedName
                 [ "{" "refines" STRING "}" ] ;

DecidedBecause = "decided" "because" "{" STRING { STRING } "}" ;

RejectedAlternatives = "rejected" "alternatives" "{"
                       IDENT ":" STRING { IDENT ":" STRING }
                       "}" ;

RevisitWhen    = "revisit" "when" "{" STRING { STRING } "}" ;

UseScope       = "use" IDENT "." IDENT ;

(* Helpers *)

ScopeExpr      = SetUnion ;
SetUnion       = SetIntersect { "|" SetIntersect } ;
SetIntersect   = SetDiff { "&" SetDiff } ;
SetDiff        = SetPrimary { "\" SetPrimary } ;
SetPrimary     = "[" EntityName { "," EntityName } "]"
               | "{" IDENT "|" IDENT "matches" Pattern "}"
               | IDENT
               | PREFIX_GLOB
               | SUFFIX_GLOB
               | "(" ScopeExpr ")" ;

Pattern        = IDENT | PREFIX_GLOB | SUFFIX_GLOB ;

EntityRef      = "[" EntityName { "," EntityName } "]"
               | IDENT
               | PREFIX_GLOB
               | SUFFIX_GLOB ;

EntityName     = IDENT | PREFIX_GLOB | SUFFIX_GLOB ;
IdentList      = "[" IDENT { "," IDENT } "]" ;
Params         = "(" Param { "," Param } ")" ;
Param          = IDENT ":" Value ;
Value          = INT | FLOAT | PERCENT | DURATION | STRING ;
DottedName     = IDENT { "." IDENT } ;

(* Terminals *)

IDENT          = /[a-zA-Z_][a-zA-Z0-9_]*/ ;
PREFIX_GLOB    = /\*[a-zA-Z0-9_]+/ ;
SUFFIX_GLOB    = /[a-zA-Z_][a-zA-Z0-9_]*\*/ ;
INT            = /[0-9]+/ ;
FLOAT          = /[0-9]+\.[0-9]+/ ;
PERCENT        = /[0-9]+(\.[0-9]+)?%/ ;
DURATION       = /[0-9]+[smhd]/ ;
STRING         = /"[^"]*"/ ;
COMMENT        = /\/\/[^\n]*/ ;  (* discarded *)
```

---

## 10. Semantic Rules

### 10.1 Name Resolution

1. **Scope names** are resolved within the current concern's scope declarations, plus any `use`-imported scopes from other concerns.

2. **Entity names** in constraint rules are resolved in two phases:
   - First, check if the name matches a declared scope. If so, expand to the scope's entity list.
   - Otherwise, treat as a literal code entity name (module, type, or function).

3. **Glob patterns** are expanded against the code entity index using regex matching:
   - `*Foo` becomes regex `^.*Foo$`
   - `Foo*` becomes regex `^Foo.*$`

### 10.2 Layer Ordering

Layers are ordered by declaration order (first declared = highest). For layers L_1 through L_n, for every pair (i, j) where i < j, an implicit constraint is generated:

```
L_j.entities must_not depend_on L_i.entities
```

### 10.3 Scope Visibility

- Scopes declared within a concern are visible to all items within that concern.
- Scopes imported via `use Concern.scope` are visible within the importing concern.
- Scopes are not visible across concern boundaries without explicit `use`.

### 10.4 Constraint Independence

Each constraint rule is evaluated independently. There is no global fixpoint computation and no ordering dependency between constraints.

---

## 11. Verification Semantics

### 11.1 Structural Verification

Let `G = (V, E)` be the code dependency graph where `V` is the set of code entities and `E` represents dependencies.

| Rule | Formal semantics |
|------|-----------------|
| `A must_not depend_on B` | There is no path from any `a in A` to any `b in B` in `G` |
| `only A accesses B` | For all `v in V \ A`, there is no path from `v` to any `b in B` |
| `P occur_only_in M` | For all `v in V` matching pattern `P`: `v in M` |
| `A must_not reference B` | No entity in `A` contains a type path or call expression naming any entity in `B` |
| `A must_depend_on B` | At least one entity in `A` has a dependency on some entity in `B` |
| `A must_reference B` | At least one entity in `A` contains a reference to some entity in `B` |
| `T must_implement Tr` | An `impl Tr for T` block exists in the codebase |
| `forall x in S: P(x)` | For all `x in S`: `P(x)` holds |
| `exists x in S: P(x)` | There exists `x in S` such that `P(x)` holds |
| `A depends_on B => C` | If `A` depends on `B`, then `C` must hold |
| `pred(args)` | Expand predicate `pred` with `args` substituted |

### 11.2 Behavioral Verification

Let `S` be a TLA+ specification and `O` be a generated obligation formula.

- `S satisfies O` iff Apalache verifies `O` as an invariant of `S`.
- Pattern application `apply P(args) to C refines S` generates obligations that all of `P`'s invariants hold in `S` with parameter values substituted from `args`.

---

## 12. Complete Examples

### 12.1 Storage Resilience

```intent
concern ResilientStorage {
    scope storage_backends {
        [DgraphClient, MilvusClient]
    }

    scope storage_boundary {
        only [storage] accesses storage_backends
    }

    scope processing {
        [services, pipeline, rag, community, knowledge]
    }

    constraint no_direct_backend_access {
        processing must_not depend_on storage_backends
    }

    apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
        to StorageCoordinator.dgraph_circuit_breaker {
            refines "formal/tla/CircuitBreaker.tla"
        }

    apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
        to StorageCoordinator.milvus_circuit_breaker {
            refines "formal/tla/CircuitBreaker.tla"
        }

    decided because {
        "Dgraph and Milvus are external dependencies with independent failure modes."
        "Circuit breakers prevent cascading failures."
    }

    rejected alternatives {
        retry_only: "Retries without circuit breaking cause request pileup during outages."
        failover_to_replica: "Neither Dgraph nor Milvus runs replicas in current deployment."
    }

    revisit when {
        "Dgraph or Milvus runs in a replicated HA configuration"
        "A third storage backend is added"
    }
}
```

### 12.2 Layered Architecture

```intent
concern LayeredArchitecture {
    layer presentation { [routes] }
    layer application { [services] }
    layer processing { [pipeline, segmentation, rag, community, knowledge] }
    layer infrastructure { [storage] }

    constraint auth_boundary {
        [services, storage, pipeline] must_not reference [AuthMiddleware]
    }

    decided because {
        "Layered architecture ensures each layer depends only on layers below it."
        "Auth enforcement at the route layer provides a single enforcement point."
    }

    rejected alternatives {
        flat_architecture: "No dependency direction leads to circular dependencies."
        hexagonal_ports: "Overkill for a monolithic codebase with a single deployment unit."
    }

    revisit when {
        "Services are extracted into independently deployable microservices"
        "A second client type (CLI, gRPC) is added beyond HTTP"
    }
}
```

### 12.3 Trait Conformance

```intent
concern StorageContracts {
    constraint backend_traits {
        DgraphClient must_implement GraphStore
        MilvusClient must_implement VectorStore
    }

    constraint client_locality {
        *Client occur_only_in [storage]
    }

    decided because {
        "Trait-based storage abstraction enables in-memory test doubles."
        "Client types confined to storage module prevents leaking infrastructure."
    }
}
```

### 12.4 Cross-Concern References

```intent
concern ExtendedStorageChecks {
    use ResilientStorage.storage_backends

    constraint chat_isolation {
        [chat] must_not depend_on storage_backends
    }
}
```

### 12.5 Advanced Features (v0.2)

This example demonstrates all v0.2 features: let bindings, set expressions, quantifiers, implication, and predicates.

```intent
concern AdvancedArchitecture {
    // Let bindings with set expressions
    let backends = [DgraphClient, MilvusClient]
    let cache = [RedisClient]
    let external = backends | cache
    let core = [services, pipeline, rag] \ [test_helpers]
    let clients = { e | e matches *Client }

    // Predicate definition: reusable constraint pattern
    predicate isolated(src, target) {
        src must_not depend_on target
        src must_not reference target
    }

    // Predicate call
    constraint boundaries {
        isolated(core, external)
    }

    // Quantified constraints
    constraint error_handling {
        forall s in core: s must_reference [AppError]
        exists s in core: s must_depend_on logging
    }

    // Implication (conditional constraints)
    constraint caching_discipline {
        forall m in core:
            m depends_on cache => m must_depend_on cache_invalidation
    }

    decided because {
        "Set algebra enables compositional scope definitions."
        "Quantifiers make constraint semantics explicit."
        "Predicates enable reusable constraint patterns."
    }
}
```

---

## 13. CLI Usage

```
intent-check <COMMAND>

Commands:
  check       Full verification pipeline (structural + behavioral + rationale)
  structural  Structural verification only
  compile     Generate TLA+ obligation modules
  verify      Run Apalache on existing obligation files
  rationale   Extract rationale to JSON

Options:
  --format <text|json>    Output format (default: text)
  --quiet                 Suppress non-error output
```

**Running structural checks:**

```bash
intent-check structural \
    --intent formal/intent/ \
    --codebase crates/nxbrain-core/src
```

**Running the full pipeline:**

```bash
intent-check check \
    --intent formal/intent/ \
    --codebase crates/nxbrain-core/src
```

**Running with --ignored tests (integration):**

```bash
cargo test -p intent-check --test integration -- --nocapture
```
