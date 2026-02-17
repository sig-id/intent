//! Integration tests for Intent v0.2.0 API

use intent::parser;
use intent::parser::ast::*;
use intent::structural;

#[test]
fn parse_full_system_roundtrip() {
    let source = r#"
system Example {
    description "Example system"

    scope backends { [FooClient, BarClient] }
    scope boundary { only [storage] accesses backends }

    component presentation {
        kind: layer
        contains [routes]
        order: 1
    }
    component application {
        kind: layer
        contains [services]
        order: 2
    }
    component infrastructure {
        kind: layer
        contains [storage]
        order: 3
    }

    constraint no_leak {
        !application.depends(backends)
    }

    applies CircuitBreaker {
        threshold: 5
        timeout: 30
    }

    decided because {
        "reason one"
        "reason two"
    }

    rejected {
        alt_a: "bad because X"
        alt_b: "bad because Y"
    }

    revisit when {
        "condition changes"
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    let system = match &top_levels[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    assert_eq!(system.name, "Example");

    assert_eq!(system.scopes.len(), 2);
    assert_eq!(system.constraints.len(), 1);
    assert_eq!(system.components.len(), 3);
    assert_eq!(system.applies.len(), 1);
}

#[test]
fn structural_check_with_tempdir() {
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(
        tmp.path().join("lib.rs"),
        "mod routes;\nmod services;\nmod storage;\n",
    )
    .unwrap();

    for dir in &["routes", "services", "storage"] {
        std::fs::create_dir(tmp.path().join(dir)).unwrap();
    }

    std::fs::write(tmp.path().join("routes/mod.rs"), "pub struct Router;\n").unwrap();
    std::fs::write(tmp.path().join("services/mod.rs"), "pub fn init() {}\n").unwrap();
    std::fs::write(tmp.path().join("storage/mod.rs"), "pub struct DbClient;\n").unwrap();

    let source = r#"
system Layered {
    component presentation {
        kind: layer
        contains [routes]
        order: 1
    }
    component application {
        kind: layer
        contains [services]
        order: 2
    }
    component infrastructure {
        kind: layer
        contains [storage]
        order: 3
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    let systems: Vec<_> = top_levels
        .iter()
        .filter_map(|t| match t {
            TopLevel::System(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    let results = structural::check(&systems, tmp.path()).unwrap();

    // 3 layers -> C(3,2) = 3 implicit must_not_depend_on constraints
    assert_eq!(results.len(), 3);
    assert!(
        results.iter().all(|r| r.passed),
        "clean codebase should pass all layer constraints"
    );
}

#[test]
fn structural_detects_layer_violation() {
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(
        tmp.path().join("lib.rs"),
        "mod routes;\nmod storage;\n",
    )
    .unwrap();

    std::fs::create_dir(tmp.path().join("routes")).unwrap();
    std::fs::create_dir(tmp.path().join("storage")).unwrap();

    std::fs::write(tmp.path().join("routes/mod.rs"), "pub struct Router;\n").unwrap();
    // storage depends on routes — violation
    std::fs::write(
        tmp.path().join("storage/mod.rs"),
        "use crate::routes::Router;\npub struct StorageClient;\n",
    )
    .unwrap();

    let source = r#"
system Layered {
    component presentation {
        kind: layer
        contains [routes]
        order: 1
    }
    component infrastructure {
        kind: layer
        contains [storage]
        order: 2
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    let systems: Vec<_> = top_levels
        .iter()
        .filter_map(|t| match t {
            TopLevel::System(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    let results = structural::check(&systems, tmp.path()).unwrap();

    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(
        !failed.is_empty(),
        "storage depending on routes should be a violation"
    );
}

#[test]
fn parse_behavior_with_transitions() {
    let source = r#"
system ContractLifecycle {
    component Engine {
        kind: subsystem

        behavior Lifecycle {
            states {
                PUBLISH { initial: true }
                CLAIM
                EXECUTE
                DELIVER
                VERIFY
                SETTLE { terminal: true }
                CANCEL { terminal: true }
            }

            transitions {
                PUBLISH -> CLAIM on submit
                CLAIM -> EXECUTE on accept
                CLAIM -> CANCEL on reject
                EXECUTE -> DELIVER on complete
                DELIVER -> VERIFY on submit_work
                VERIFY -> SETTLE on approve
            }
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    let system = match &top_levels[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    assert_eq!(system.name, "ContractLifecycle");
    assert_eq!(system.components.len(), 1);

    let component = &system.components[0];
    assert_eq!(component.kind, ComponentKind::Subsystem);
    assert_eq!(component.behaviors.len(), 1);

    let behavior = &component.behaviors[0];
    assert_eq!(behavior.name, "Lifecycle");
    assert_eq!(behavior.states.len(), 7);
    assert_eq!(behavior.transitions.len(), 6);

    // Check initial state
    let initial: Vec<_> = behavior.states.iter().filter(|s| s.initial).collect();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].name, "PUBLISH");

    // Check terminal states
    let terminal: Vec<_> = behavior.states.iter().filter(|s| s.terminal).collect();
    assert_eq!(terminal.len(), 2);
}

#[test]
fn parse_temporal_expr_with_operators() {
    let source = r#"
system TemporalOps {
    behavior Workflow {
        states {
            pending { initial: true }
            settled { terminal: true }
            failed { terminal: true }
            processing
        }

        transitions {
            pending -> processing on start
            processing -> settled on complete
            processing -> failed on error
        }

        property eventual_completion {
            always(pending => eventually(settled | failed))
        }

        property conjunction_test {
            always(processing => eventually(settled & pending))
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    let system = match &top_levels[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };

    let empty_behaviors = vec![];
    let behavior = system.components.get(0).map(|c| &c.behaviors).unwrap_or(&empty_behaviors);
    if behavior.is_empty() {
        // Behavior might be at system level
        assert_eq!(system.behaviors.len(), 1);
        let beh = &system.behaviors[0];
        assert_eq!(beh.properties.len(), 2);

        // Check eventual_completion
        let prop = &beh.properties[0];
        assert_eq!(prop.name, "eventual_completion");
        match &prop.expr {
            TemporalExpr::Always(inner) => {
                match inner.as_ref() {
                    TemporalExpr::BinOp { op, .. } => {
                        assert_eq!(*op, TemporalOp::Implies);
                    }
                    _ => panic!("expected BinOp with Implies"),
                }
            }
            _ => panic!("expected Always"),
        }
    }
}

#[test]
fn parse_import_and_apply_pattern() {
    let source = r#"
import pattern Saga from "github.com/org/patterns@v1.2"
import template Auth from "github.com/org/auth@main" with {
    mfa: true
}

system Payment {
    uses Auth

    applies Saga {
        timeout: 30
    }

    decided because {
        "Saga pattern handles distributed transactions."
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 3); // 2 imports + 1 system

    // Check imports
    match &top_levels[0] {
        TopLevel::Import(i) => {
            assert_eq!(i.kind, ImportKind::Pattern);
            assert_eq!(i.name, "Saga");
            assert_eq!(i.source, "github.com/org/patterns@v1.2");
        }
        _ => panic!("expected Import"),
    }

    match &top_levels[1] {
        TopLevel::Import(i) => {
            assert_eq!(i.kind, ImportKind::Template);
            assert_eq!(i.name, "Auth");
            assert!(i.with_params.iter().any(|(k, v)| k == "mfa" && *v == ParamValue::Bool(true)));
        }
        _ => panic!("expected Import"),
    }

    // Check system
    match &top_levels[2] {
        TopLevel::System(s) => {
            assert_eq!(s.uses, vec!["Auth"]);
            assert_eq!(s.applies.len(), 1);
            assert_eq!(s.applies[0].pattern, "Saga");
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_pattern_with_multiple_parameters() {
    let source = r#"
pattern Retry<Op> {
    parameters {
        max_attempts: Int { default: 3 }
        delay_ms: Int { default: 100 }
        backoff_factor: Int { default: 2 }
    }

    behavior RetryLoop {
        states {
            idle { initial: true }
            attempting
            succeeded { terminal: true }
            failed { terminal: true }
        }

        transitions {
            idle -> attempting on start
            attempting -> succeeded on success
            attempting -> idle on retry
            attempting -> failed on exhausted
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    match &top_levels[0] {
        TopLevel::Pattern(p) => {
            assert_eq!(p.name, "Retry");
            assert_eq!(p.type_params, vec!["Op"]);
            assert_eq!(p.parameters.len(), 3, "should have 3 parameters");
            assert_eq!(p.parameters[0].name, "max_attempts");
            assert_eq!(p.parameters[1].name, "delay_ms");
            assert_eq!(p.parameters[2].name, "backoff_factor");
            assert!(p.behavior.is_some());
        }
        _ => panic!("expected Pattern"),
    }
}

#[test]
fn parse_insight() {
    let source = r#"
insight LatentCoupling {
    discovered: "2026-02-10"
    source: "Code review"
    observation { "Inconsistent cache invalidation between services." }
    recommendation {
        constraint cache_discipline {
            [ServiceA, ServiceB].depends([CacheInvalidator])
        }
    }
    status: proposed
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    match &top_levels[0] {
        TopLevel::Insight(i) => {
            assert_eq!(i.name, "LatentCoupling");
            assert_eq!(i.discovered.as_deref(), Some("2026-02-10"));
            assert_eq!(i.status, InsightStatus::Proposed);
            assert_eq!(i.recommendation.len(), 1);
        }
        _ => panic!("expected Insight"),
    }
}

#[test]
fn parse_constraint_with_quantifiers() {
    let source = r#"
system X {
    constraint universal {
        forall svc in [ServiceA, ServiceB, ServiceC]: !svc.depends(DirectStorage)
    }

    constraint existential {
        exists handler in [Handler1, Handler2]: handler.implements(Validator)
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.constraints.len(), 2);

            // Check universal constraint
            let c = &s.constraints[0];
            assert_eq!(c.name, "universal");
            match &c.rules[0] {
                ConstraintRule::Forall { var, domain, body } => {
                    assert_eq!(var, "svc");
                    matches!(domain, ScopeExpr::EntityList(v) if v.len() == 3);
                    matches!(body.as_ref(), ConstraintRule::Not(_));
                }
                _ => panic!("expected Forall"),
            }

            // Check existential constraint
            let c = &s.constraints[1];
            assert_eq!(c.name, "existential");
            match &c.rules[0] {
                ConstraintRule::Exists { var, domain, body } => {
                    assert_eq!(var, "handler");
                    matches!(domain, ScopeExpr::EntityList(v) if v.len() == 2);
                    matches!(body.as_ref(), ConstraintRule::Predicate(PredicateCall::Implements { .. }));
                }
                _ => panic!("expected Exists"),
            }
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_transition_with_guard_and_effect() {
    let source = r#"
system X {
    behavior Payment {
        states {
            pending { initial: true }
            processing
            completed { terminal: true }
        }

        transitions {
            pending -> processing on validate
                where { amount <= limit }
                effect { emit PaymentStarted(order_id) }
                within { 30s }
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            let behavior = &s.behaviors[0];
            let t = &behavior.transitions[0];

            assert_eq!(t.from, "pending");
            assert_eq!(t.to, "processing");
            assert_eq!(t.on_event, "validate");
            assert!(t.guard.is_some(), "should have guard");
            assert!(!t.effects.is_empty(), "should have effects");
            assert!(t.timing.is_some(), "should have timing");

            match &t.effects[0].kind {
                EffectKind::Emit { name, args } => {
                    assert_eq!(name, "PaymentStarted");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!("expected Emit"),
            }

            match &t.timing {
                Some(TransitionTiming::Within(_)) => {}
                _ => panic!("expected Within timing"),
            }
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_distilled_from() {
    let source = r#"
system X {
    distilled from "crates/client/src/*.rs" {
        commit: "a1b2c3d"
        observation { "Exponential backoff emerged in all client implementations." }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.distilled.len(), 1);
            let d = &s.distilled[0];
            assert_eq!(d.source, "crates/client/src/*.rs");
            assert_eq!(d.commit, "a1b2c3d");
            assert!(d.observation.is_some());
        }
        _ => panic!("expected System"),
    }
}
