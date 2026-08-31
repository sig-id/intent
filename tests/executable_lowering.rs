//! Regression tests for how `executable` behaviors lower to TLA+.
//!
//! Each case here corresponds to a defect that produced a spec which either
//! SANY rejected or, worse, accepted with the wrong meaning.

use intent::parser;
use intent::parser::ast::TopLevel;
use intent::transpile::executable::generate_executable_v2;

/// Compile the single behavior in `source` and return its TLA+ module text.
fn lower(source: &str) -> String {
    let top = parser::parse(source).expect("spec parses");
    let system = top
        .into_iter()
        .find_map(|t| match t {
            TopLevel::System(s) => Some(s),
            _ => None,
        })
        .expect("a system declaration");
    let behavior = system.behaviors.first().expect("a behavior");
    generate_executable_v2(behavior, &system.name, false)
        .expect("behavior lowers")
        .content
}

const NEGATED_CONJUNCTION: &str = r#"
system T {
    behavior B executable {
        model {
            state pending { initial: true }
            state registered
            state done { terminal: true }
        }
        memory {
            a: Bool = false
            b: Bool = false
        }
        transition pending -> done on go {
            set memory.a = true
        }
        invariant negated_conjunction {
            always(!(a && b) || !(pending || registered))
        }
    }
}
"#;

#[test]
fn negation_applies_to_the_whole_parenthesised_operand() {
    let module = lower(NEGATED_CONJUNCTION);
    // Previously lowered to `~a /\ b \/ ...`, silently negating only `a`.
    assert!(
        module.contains("~(a /\\ b)"),
        "negation must cover the conjunction, got:\n{module}"
    );
    assert!(
        !module.contains("~a /\\ b"),
        "negation must not distribute onto the first operand only, got:\n{module}"
    );
}

#[test]
fn mixed_connectives_are_grouped() {
    let module = lower(NEGATED_CONJUNCTION);
    let line = module
        .lines()
        .find(|l| l.starts_with("NegatedConjunction =="))
        .expect("invariant is emitted");
    // TLA+ gives `/\` and `\/` equal precedence and SANY rejects them side by
    // side ungrouped, so every mixed operand must carry its own parentheses.
    let body = line.split_once("==").expect("definition body").1.trim();
    assert!(
        body.starts_with('(') && body.ends_with(')'),
        "compound invariant body must be delimited, got: {body}"
    );
}

#[test]
fn state_names_in_invariants_lower_to_state_predicates() {
    let module = lower(NEGATED_CONJUNCTION);
    // The state constants are `Str`; emitting a bare `pending` made Snowcat
    // reject `~pending` with a type error.
    assert!(
        module.contains("(state = pending)") && module.contains("(state = registered)"),
        "state names must lower to state predicates, got:\n{module}"
    );
}

#[test]
fn guard_referenced_vars_are_declared_alongside_memory() {
    let module = lower(
        r#"
system T {
    behavior B executable {
        model {
            state open { initial: true }
            state shut { terminal: true }
        }
        vars {
            frozen: Bool = false
            label: String = ""
        }
        memory {
            seen: Bool = false
        }
        transition open -> shut on close {
            where { !frozen }
            set memory.seen = true
        }
    }
}
"#,
    );
    let declarations = module
        .split_once("vars ==")
        .expect("a vars tuple")
        .0;
    // `memory` is the state carrier, but a `vars` entry read by a guard must
    // still be declared or the guard names an undeclared operator.
    assert!(
        declarations.contains("frozen"),
        "guard-referenced var must be declared, got:\n{module}"
    );
    assert!(
        declarations.contains("seen"),
        "memory field must remain declared, got:\n{module}"
    );
    // `label` is neither a guard atom nor of model type, so it stays out.
    assert!(
        !declarations.contains("label"),
        "unreferenced fixture var must not become model state, got:\n{module}"
    );
}

#[test]
fn memory_only_behaviors_are_unchanged() {
    // Guards that read only memory must lower exactly as before.
    let module = lower(
        r#"
system T {
    behavior B executable {
        model {
            state open { initial: true }
            state shut { terminal: true }
        }
        memory {
            ready: Bool = false
        }
        transition open -> shut on close {
            where { ready }
        }
    }
}
"#,
    );
    let declarations = module.split_once("vars ==").expect("a vars tuple").0;
    assert!(declarations.contains("ready"));
    assert!(module.contains("/\\ ready"), "guard is emitted:\n{module}");
}

#[test]
fn next_lowers_to_a_primed_action_formula() {
    // TLA+ has no LTL `X`; SANY rejects it as an unknown operator. A next-state
    // assertion is a primed expression, and a temporal formula containing one
    // must be an action formula (`[][...]_vars`).
    let module = lower(
        r#"
system T {
    behavior B executable {
        model {
            state open { initial: true }
            state shut { terminal: true }
        }
        memory {
            seen: Bool = false
        }
        transition open -> shut on close {
            set memory.seen = true
        }
        property single_use {
            always(shut => !next(shut))
        }
    }
}
"#,
    );
    assert!(
        !module.contains("X("),
        "must not emit an LTL X operator, got:\n{module}"
    );
    assert!(
        module.contains("[][") && module.contains("]_domain_vars"),
        "a next-bearing property must be an action formula, got:\n{module}"
    );
    assert!(module.contains(")'"), "next must prime its operand, got:\n{module}");
}

#[test]
fn property_referenced_vars_are_declared() {
    // Same class as the guard case: a `vars` entry a temporal property reads
    // must be declared, or it lowers to an undeclared operator.
    let module = lower(
        r#"
system T {
    behavior B executable {
        model {
            state open { initial: true }
            state shut { terminal: true }
        }
        vars {
            generation: Int = 0
        }
        memory {
            seen: Bool = false
        }
        transition open -> shut on close {
            set memory.seen = true
        }
        property no_rotation {
            always(shut => generation == 0)
        }
    }
}
"#,
    );
    let declarations = module.split_once("vars ==").expect("a vars tuple").0;
    assert!(
        declarations.contains("generation"),
        "property-referenced var must be declared, got:\n{module}"
    );
}

#[test]
fn action_properties_are_subscripted_on_domain_state() {
    // The generated `Stutter` action rewrites `action_taken`, so it is not
    // `UNCHANGED vars`. Subscripting an action-formula property on `vars` made
    // every such property fail on a stutter step, for reasons unrelated to what
    // it asserts.
    let module = lower(
        r#"
system T {
    behavior B executable {
        model {
            state open { initial: true }
            state shut { terminal: true }
        }
        memory {
            seen: Bool = false
        }
        transition open -> shut on close {
            set memory.seen = true
        }
        property single_use {
            always(shut => !next(shut))
        }
    }
}
"#,
    );
    assert!(
        module.contains("domain_vars == <<state, seen>>"),
        "domain state tuple must exclude trace bookkeeping, got:\n{module}"
    );
    let prop = module
        .lines()
        .find(|l| l.starts_with("Prop_single_use =="))
        .expect("property is emitted");
    assert!(
        prop.ends_with("]_domain_vars"),
        "action property must be subscripted on domain state, got: {prop}"
    );
}

#[test]
fn tlc_config_drops_the_trace_magnet() {
    // `NotTerminated` is the device the MBT driver violates on purpose so
    // Apalache emits traces reaching a terminal state. Left in a TLC config it
    // makes every run of a behavior with a terminal state report a violation,
    // burying any real one.
    let cfg = "SPECIFICATION Spec\n\nINVARIANTS\n    TypeOK\n    NotTerminated\n\nPROPERTIES\n    Prop_x\n";
    let stripped = intent::behavioral::strip_trace_magnet_for_test(cfg);
    assert!(!stripped.contains("NotTerminated"), "got:\n{stripped}");
    assert!(stripped.contains("TypeOK"), "other invariants survive:\n{stripped}");
    assert!(stripped.contains("Prop_x"), "properties survive:\n{stripped}");
}
