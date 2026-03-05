# Nucleus System Spec: Intent Language Gap Analysis

## Context

Nucleus is a lightweight Docker alternative for agents, using Linux kernel
primitives (namespaces, cgroups, seccomp, Landlock, capabilities). Its formal
verification suite has 15 TLA+ specs. **14 are already auto-generated** from
Intent files in `nucleus/intent/`. The remaining one — `Nucleus_System.tla` — is
a 580-line hand-written integrated system model.

This report analyzes what prevents `Nucleus_System.tla` from being expressed in
the Intent language and transpiled.

## What Already Works

The subsystem behaviors map perfectly to Intent and are already transpiled:

| Intent behavior                | Generated TLA+ module                                          |
|--------------------------------|----------------------------------------------------------------|
| `NamespaceLifecycle`           | `Nucleus_Isolation_NamespaceLifecycle.tla`                     |
| `CgroupLifecycle`              | `Nucleus_Resources_CgroupLifecycle.tla`                        |
| `FilesystemLifecycle`          | `Nucleus_Filesystem_FilesystemLifecycle.tla`                   |
| `SecurityEnforcement`          | `Nucleus_Security_SecurityEnforcement.tla`                     |
| `CapabilityDropping`           | `NucleusSecurity_Capabilities_CapabilityDropping.tla`          |
| `SeccompEnforcement`           | `NucleusSecurity_Seccomp_SeccompEnforcement.tla`               |
| `NamespaceIsolation`           | `NucleusSecurity_Namespaces_NamespaceIsolation.tla`            |
| `ResourceLimiting`             | `NucleusSecurity_Cgroups_ResourceLimiting.tla`                 |
| `GVisorRuntime`                | `NucleusSecurity_GVisor_GVisorRuntime.tla`                     |
| `ContainerEscapePrevention`    | `NucleusSecurity_ContainerEscapePrevention.tla`                |
| `ResourceExhaustionPrevention` | `NucleusSecurity_ResourceExhaustionPrevention.tla`             |
| `ContainerLifecycleTest`       | `NucleusVerification_IntegrationTests_ContainerLifecycleTest.tla` |
| `NamespaceIsolationSpec`       | `NucleusVerification_FormalVerification_NamespaceIsolationSpec.tla` |
| `ResourceLimitSpec`            | `NucleusVerification_FormalVerification_ResourceLimitSpec.tla`  |

These are simple linear or branching state machines with a single `state`
variable — exactly what Intent's behavior construct was designed for.

## The Gap: `Nucleus_System.tla`

The integrated system model composes all subsystems into a multi-container
orchestration spec with authorization, adversarial modeling, and cross-subsystem
invariants. It uses TLA+ features that have no direct Intent equivalent.

### Gap 1: Multi-Entity Record State

**TLA+ pattern:**
```tla
VARIABLES containers   \* [Containers -> Record]

Init == containers = [c \in Containers |-> ContainerInitRecord]

CreateContainer(c) ==
    /\ containers' = [containers EXCEPT
          ![c].exists = TRUE,
          ![c].owner = caller,
          ![c].lifecycle = LC_created,
          \* ... 17 more fields
       ]
```

**Problem:** Intent behaviors have a single implicit `state` variable plus flat
user-declared variables. There is no way to declare a function from an entity set
to a record type, nor to update individual fields of an indexed record.

**What would be needed:** A `Map(K, Record)` variable type with field-level
update syntax in effects, e.g.:
```intent
variables {
    containers: Map(ContainerId, ContainerRecord) = [c -> default_record]
}
```

### Gap 2: Quantified Transitions Over Entity Sets

**TLA+ pattern:**
```tla
Next ==
    \/ \E c \in Containers : CreateContainer(c)
    \/ \E c \in Containers : SetupNamespaces(c)
    \/ \E c \in Containers, p \in AllowedMountPaths : BindMountPath(c, p)
    \/ \E c \in Containers, s \in Signals : StopOrKillOrAttach(c, "stop", s)
```

**Problem:** Intent transitions operate on the behavior's own `state` variable.
There is no way to parameterize a transition over an entity set so that it
generates `\E c \in Set : Action(c)` wrappers.

**Partial support:** Intent has `nodes: replicas` for distributed behaviors, but
this generates replicated state variables (one per node), not a function from
entities to records. It also doesn't support parameterizing transitions over
multiple sets simultaneously (`\E c \in Containers, s \in Signals`).

**What would be needed:** Entity-parameterized behaviors:
```intent
behavior ContainerOrchestration for c in Containers {
    transitions {
        nonexistent -> created on create
            where { !containers[c].exists }
            effect { containers[c].exists = true }
    }
}
```

### Gap 3: System-Level Variables

**TLA+ pattern:**
```tla
VARIABLES
    containers,   \* per-entity state
    caller,       \* active control-plane user
    now,          \* logical clock
    events        \* audit trail
```

**Problem:** Intent variables are scoped to a behavior. There is no way to
declare variables that exist at the system level and are shared/updated across
multiple behaviors or transitions. `caller`, `now`, and `events` are not part of
any single subsystem.

**What would be needed:** System-scoped variable declarations:
```intent
system Nucleus {
    variables {
        caller: String = "root" where { in: Users }
        now: Nat = 0
        events: Seq(EventRecord) = <<>>
    }
}
```

### Gap 4: Conditional Branching Within Transitions

**TLA+ pattern:**
```tla
ApplySeccomp(c) ==
    /\ containers[c].sec_state = SS_caps_dropped
    /\ IF containers[c].seccomp_supported THEN
          containers' = [containers EXCEPT ![c].sec_state = SS_seccomp_applied, ![c].seccomp_on = TRUE]
       ELSE IF containers[c].allow_degraded THEN
          containers' = [containers EXCEPT ![c].sec_state = SS_degraded]
       ELSE
          containers' = [containers EXCEPT ![c].lifecycle = LC_failed]
```

**Problem:** Intent's `where` guards are preconditions that enable/disable a
transition. They cannot express "if condition A, go to state X; else if condition
B, go to state Y; else go to state Z" within a single logical action.

**Workaround:** Split into 3 separate transitions with mutually exclusive guards:
```intent
transitions {
    created -> created on apply_seccomp
        where { sec_state == "caps_dropped" && seccomp_supported }
        effect { sec_state = "seccomp_applied"; seccomp_on = true }
    created -> created on apply_seccomp_degraded
        where { sec_state == "caps_dropped" && !seccomp_supported && allow_degraded }
        effect { sec_state = "degraded" }
    created -> failed on apply_seccomp_fail
        where { sec_state == "caps_dropped" && !seccomp_supported && !allow_degraded }
}
```

This works but changes the event names (3 distinct events instead of 1) and is
verbose. The hand-written TLA+ treats these as a single atomic action with
internal branching.

### Gap 5: Authorization and Access Control

**TLA+ pattern:**
```tla
AccessAllowed(u, c) == (u = RootUser) \/ (containers[c].owner = u)

StopOrKillOrAttach(c, op, sig) ==
    /\ IF AccessAllowed(caller, c) /\ PidFresh(c) THEN
          /\ events' = AppendEvent(op, c, "granted")
          /\ \* ... modify container state
       ELSE
          /\ events' = AppendEvent(op, c, "denied")
          /\ UNCHANGED containers
```

**Problem:** Intent has no concept of a "caller" or role-based access control.
Transitions either happen or don't; they cannot produce different outcomes
(granted/denied) based on who invokes them. The authorization check is
interleaved with the transition logic.

**What would be needed:** Actor-parameterized transitions with outcome branching,
or at minimum support for helper predicates that reference system variables.

### Gap 6: Adversarial / Environment Actions

**TLA+ pattern:**
```tla
SeccompUnsupported(c) == ...   \* kernel changes support flags
SyscallFailure(c) == ...       \* runtime syscall fails
PidReuseRace(c) == ...         \* OS reuses PID under us
RefreshPid(c) == ...           \* we detect and fix stale PID
```

**Problem:** Intent transitions model the system's own actions. There is no
semantic distinction for environment/adversary actions — transitions that
represent things happening *to* the system rather than *by* the system.

**Workaround:** These can be modeled as regular transitions (the TLA+ doesn't
distinguish them either — it's just a comment convention). But it would be
cleaner to have an `adversary` or `environment` block.

### Gap 7: Audit Trail / Event Log

**TLA+ pattern:**
```tla
VARIABLE events   \* Seq(Record)

AppendEvent(op, target, result) ==
    Append(events, [op |-> op, actor |-> caller, target |-> target, ...])
```

Every transition appends to the events sequence, enabling invariants like
`AuthorizationGrantedOnlyForOwnerOrRoot` that inspect the last event.

**Problem:** Intent has no sequence-append effect. The `emit` effect exists but
maps to a different semantic (event emission for composition, not an in-spec
audit log).

### Gap 8: INSTANCE Module Composition

**TLA+ pattern:**
```tla
INSTANCE Nucleus_Isolation_NamespaceLifecycle AS Iso
INSTANCE Nucleus_Filesystem_FilesystemLifecycle AS Fs
```

**Problem:** Intent's `composes [A, B]` merges behaviors into a single state
machine. TLA+'s `INSTANCE` imports definitions from another module without
merging — it's used here to reference the subsystem state constants and ensure
consistency between the integrated model and subsystem specs.

### Gap 9: Helper Operators and Ranking Functions

**TLA+ pattern:**
```tla
LifecycleRank(s) ==
    IF s = LC_nonexistent THEN 0
    ELSE IF s = LC_created THEN 1
    ...

NoBackwardLifecycle ==
    []\A c \in Containers : LifecycleRank(containers'[c].lifecycle) >= LifecycleRank(containers[c].lifecycle)
```

**Problem:** Intent has no way to define helper functions/operators that map
state values to integers. The `predicate` construct exists but is for structural
analysis (syn-based), not behavioral specs.

**What would be needed:** User-defined TLA+ operators within behaviors:
```intent
function lifecycle_rank(s: String) -> Nat {
    case {
        s == "nonexistent" => 0,
        s == "created" => 1,
        ...
    }
}
```

### Gap 10: Cross-Subsystem Invariants

**TLA+ pattern:**
```tla
RunningRequiresIsolation ==
    \A c \in Containers :
      containers[c].lifecycle = LC_running =>
        /\ containers[c].ns_state = NS_entered
        /\ containers[c].fs_state = FS_pivoted
        /\ containers[c].res_state = RS_attached
```

**Problem:** Intent invariants are scoped to a single behavior. There is no way
to write an invariant that spans multiple subsystem states (lifecycle +
namespace + filesystem + resource) and quantifies over an entity set.

## Summary Table

| Feature | Intent support | Gap severity |
|---------|---------------|-------------|
| Simple state machines | Full | — |
| Flat variables with guards | Full | — |
| Temporal properties (LTL) | Full | — |
| Fairness specifications | Full | — |
| Multi-entity record state | None | **Critical** |
| Quantified transitions (`\E c`) | Partial (`nodes:`) | **Critical** |
| System-level variables | None | **High** |
| Conditional branching in effects | Partial (split transitions) | Medium |
| Authorization / access control | None | **High** |
| Adversary / environment actions | None (cosmetic) | Low |
| Audit trail (sequence append) | None | Medium |
| `INSTANCE` module references | None | Medium |
| Helper operators / ranking fns | None | Medium |
| Cross-subsystem invariants | None | **High** |

## Recommendations

### Short-term: `tla!()` escape hatch

The existing subsystem specs stay as Intent. For the system model, use a hybrid
approach with `tla!()` for the parts that don't map:

```intent
behavior ContainerOrchestration {
    // Use tla!() for record state, quantified transitions, etc.
    tla!("VARIABLES containers, caller, now, events")
    tla!("Init == containers = [c \\in Containers |-> ContainerInitRecord]")
}
```

This preserves the intent file as documentation but delegates the heavy lifting
to inline TLA+. Not ideal — it's essentially embedding the hand-written spec.

### Medium-term: Language extensions

Priority order for maximum coverage:

1. **Entity parameterization** — `for c in Set { ... }` on behaviors
2. **Record/Map variable types** — `Map(K, V)` with field-level effects
3. **System-level variables** — variables outside any behavior
4. **User-defined operators** — pure functions usable in guards/invariants
5. **Cross-behavior invariants** — invariants at system scope

Extensions 1-3 would cover ~80% of `Nucleus_System.tla`. Adding 4-5 would close
the remaining gaps except for the audit trail pattern.

### Long-term: Composition model

A richer composition model where `composes` generates something closer to TLA+'s
`INSTANCE` + interleaved `Next` rather than state merging would make the system
spec expressible without escape hatches.
