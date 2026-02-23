//! Integration tests for Intent v0.2.0 API

use intent::transpile::tla;
use intent::parser;
use intent::parser::ast::*;
use intent::structural;
use std::path::Path;

/// Helper function to create a StateDecl with default new fields
fn make_state(name: &str, initial: bool, terminal: bool) -> StateDecl {
    StateDecl {
        name: name.to_string(),
        initial,
        terminal,
        parent: None,
        substates: Vec::new(),
        entry_actions: Vec::new(),
        exit_actions: Vec::new(),
    }
}

#[test]
fn parse_full_system_roundtrip() {
    let source = r#"
system Example {
    description "Example system"

    component presentation {
        contains [routes]
    }
    component application {
        contains [services]
        depends_only [presentation]
    }
    component infrastructure {
        contains [storage]
        depends_only [application]
    }

    constraint no_leak {
        !application.depends([FooClient, BarClient])
    }

    constraint layering {
        !infrastructure.depends([presentation])
    }

    applies CircuitBreaker {
        threshold: 5
        timeout: 30
    }

    rationale LayeringDecision {
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
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    let system = match &top_levels[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    assert_eq!(system.name, "Example");

    assert_eq!(system.constraints.len(), 2);
    assert_eq!(system.components.len(), 3);
    assert_eq!(system.applies.len(), 1);
    assert_eq!(system.rationales.len(), 1);
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
        contains [routes]
    }
    component application {
        contains [services]
        depends_only [presentation]
    }
    component infrastructure {
        contains [storage]
        depends_only [application]
    }

    constraint layering {
        !infrastructure.depends([presentation])
        !application.depends([infrastructure])
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

    // 2 explicit layering constraints
    assert_eq!(results.len(), 2);
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
        contains [routes]
    }
    component infrastructure {
        contains [storage]
    }

    constraint layering {
        !infrastructure.depends([presentation])
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
fn parse_ltl_temporal_operators() {
    let source = r#"
system LTLComplete {
    behavior StateMachine {
        states {
            idle { initial: true }
            active
            done { terminal: true }
        }

        transitions {
            idle -> active on start
            active -> done on finish
        }

        // X (next) - idle must hold in the next state
        property next_test {
            next(idle)
        }

        // U (until) - active holds until done
        property until_test {
            active until done
        }

        // R (release) - done releases active
        property release_test {
            done releases active
        }

        // W (weak until) - active holds until done, or forever
        property weak_until_test {
            active weak_until done
        }

        // M (strong release) - done strongly releases active
        property strong_release_test {
            done strong_releases active
        }

        // Combined: always(idle => next(active))
        property combined_test {
            always(idle => next(active))
        }

        // Nested: always(active until eventually(done))
        property nested_test {
            always(active until eventually(done))
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

    let beh = &system.behaviors[0];
    assert_eq!(beh.properties.len(), 7);

    // Check next_test
    let prop = &beh.properties[0];
    assert_eq!(prop.name, "next_test");
    assert!(matches!(&prop.expr, TemporalExpr::Next(_)));

    // Check until_test
    let prop = &beh.properties[1];
    assert_eq!(prop.name, "until_test");
    assert!(matches!(&prop.expr, TemporalExpr::Until { .. }));

    // Check release_test
    let prop = &beh.properties[2];
    assert_eq!(prop.name, "release_test");
    assert!(matches!(&prop.expr, TemporalExpr::Release { .. }));

    // Check weak_until_test
    let prop = &beh.properties[3];
    assert_eq!(prop.name, "weak_until_test");
    assert!(matches!(&prop.expr, TemporalExpr::WeakUntil { .. }));

    // Check strong_release_test
    let prop = &beh.properties[4];
    assert_eq!(prop.name, "strong_release_test");
    assert!(matches!(&prop.expr, TemporalExpr::StrongRelease { .. }));

    // Check combined_test: always(idle => next(active))
    let prop = &beh.properties[5];
    assert_eq!(prop.name, "combined_test");
    match &prop.expr {
        TemporalExpr::Always(inner) => {
            match inner.as_ref() {
                TemporalExpr::BinOp { op: TemporalOp::Implies, rhs, .. } => {
                    assert!(matches!(rhs.as_ref(), TemporalExpr::Next(_)));
                }
                _ => panic!("expected Implies"),
            }
        }
        _ => panic!("expected Always"),
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

    rationale SagaDecision {
        decided because {
            "Saga pattern handles distributed transactions."
        }
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
            assert_eq!(s.applies[0].pattern.name(), "Saga");
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
            assert_eq!(p.type_params.len(), 1);
            assert_eq!(p.type_params[0].name, "Op");
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
fn parse_rationale() {
    let source = r#"
rationale LatentCoupling {
    discovered: "2026-02-10"
    source: "Code review"
    observation { "Inconsistent cache invalidation between services." }
    recommendation {
        constraint cache_discipline {
            [ServiceA, ServiceB].depends([CacheInvalidator])
        }
    }
    decided because { "Use centralized cache invalidator." }
}
"#;
    let top_levels = parser::parse(source).unwrap();
    assert_eq!(top_levels.len(), 1);

    match &top_levels[0] {
        TopLevel::Rationale(r) => {
            assert_eq!(r.name, "LatentCoupling");
            assert_eq!(r.discovered.as_deref(), Some("2026-02-10"));
            assert_eq!(r.decided_because.len(), 1);
            assert_eq!(r.recommendation.len(), 1);
        }
        _ => panic!("expected Rationale"),
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

    constraint complex_body {
        forall x in [A, B]: (x.depends(C) => x.references(D))
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.constraints.len(), 3);

            // Check universal constraint
            let c = &s.constraints[0];
            assert_eq!(c.name, "universal");
            match &c.rules[0] {
                ConstraintRule::Forall { var, domain, body, .. } => {
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
                ConstraintRule::Exists { var, domain, body, .. } => {
                    assert_eq!(var, "handler");
                    matches!(domain, ScopeExpr::EntityList(v) if v.len() == 2);
                    matches!(body.as_ref(), ConstraintRule::Predicate(PredicateCall::Implements { .. }));
                }
                _ => panic!("expected Exists"),
            }

            // Check complex body constraint (forall with parenthesized implication)
            let c = &s.constraints[2];
            assert_eq!(c.name, "complex_body");
            match &c.rules[0] {
                ConstraintRule::Forall { var, body, .. } => {
                    assert_eq!(var, "x");
                    // Body should be an Implies (wrapped in parens)
                    matches!(body.as_ref(), ConstraintRule::Implies(_, _));
                }
                _ => panic!("expected Forall with complex body"),
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
                after { 30s }
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            let behavior = &s.behaviors[0];
            let t = &behavior.transitions[0];

            assert_eq!(t.from.as_state(), Some("pending"));
            assert_eq!(t.to.as_state(), Some("processing"));
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
                Some(TransitionTiming::After(_)) => {}
                _ => panic!("expected After timing"),
            }
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_distilled_pattern() {
    let source = r#"
system X {
    distilled pattern RetryWithBackoff {
        source: "crates/client/src/*.rs"
        commit: "a1b2c3d"
        extracted: "2026-02-15"
        observation { "Exponential backoff emerged in all client implementations." }
        applies_to { *Client }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.distilled.len(), 1);
            let d = &s.distilled[0];
            assert_eq!(d.name, "RetryWithBackoff");
            assert_eq!(d.source, "crates/client/src/*.rs");
            assert_eq!(d.commit, "a1b2c3d");
            assert_eq!(d.extracted, Some("2026-02-15".to_string()));
            assert!(d.observation.is_some());
            assert!(d.applies_to.is_some());
            assert_eq!(d.applies_to.as_ref().unwrap().path, "*Client");
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_emit_with_named_params() {
    let source = r#"
system X {
    behavior Flow {
        states {
            pending { initial: true }
            completed { terminal: true }
        }

        transitions {
            pending -> completed on success
                effect {
                    emit CredentialGranted(level: 1, domain: "general")
                    emit SimpleEvent(42)
                    emit NamedOnly(name: "test")
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

    let behavior = &system.behaviors[0];
    let transition = &behavior.transitions[0];

    // Check that we have 3 effects
    assert_eq!(transition.effects.len(), 3);

    // Named params are syntactic sugar - args are extracted in order
    match &transition.effects[0].kind {
        EffectKind::Emit { name, args } => {
            assert_eq!(name, "CredentialGranted");
            assert_eq!(args.len(), 2);
        }
        _ => panic!("expected Emit"),
    }

    match &transition.effects[1].kind {
        EffectKind::Emit { name, args } => {
            assert_eq!(name, "SimpleEvent");
            assert_eq!(args.len(), 1);
        }
        _ => panic!("expected Emit"),
    }

    match &transition.effects[2].kind {
        EffectKind::Emit { name, args } => {
            assert_eq!(name, "NamedOnly");
            assert_eq!(args.len(), 1);
        }
        _ => panic!("expected Emit"),
    }
}

#[test]
fn transpile_ltl_to_tla() {
    let source = r#"
system PaymentSystem {
    behavior TransactionLifecycle {
        states {
            pending { initial: true }
            validating
            processing
            settled { terminal: true }
            failed { terminal: true }
        }

        transitions {
            pending -> validating on payment_received
            validating -> processing on valid
            validating -> failed on invalid
            processing -> settled on confirmed
            processing -> failed on timeout
        }

        // LTL properties
        property eventual_settlement {
            always(pending => eventually(settled | failed))
        }

        property active_until_done {
            validating until (processing | failed)
        }

        property next_state {
            pending => next(validating | pending)
        }

        property weak_persistence {
            processing weak_until settled
        }

        property release_constraint {
            failed releases processing
        }

        fairness {
            weak(pending -> validating)
            strong(processing -> settled)
        }
    }
}
"#;

    let top_levels = parser::parse(source).unwrap();
    let system = match &top_levels[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };

    let behavior = &system.behaviors[0];
    let result = tla::generate(behavior, "PaymentSystem", Path::new(".")).unwrap();

    // Check module was generated
    assert_eq!(result.module_name, "PaymentSystem_TransactionLifecycle");
    assert!(!result.content.is_empty());

    // Check TLA+ structure
    assert!(result.content.contains("MODULE PaymentSystem_TransactionLifecycle"));
    assert!(result.content.contains("EXTENDS Naturals"));
    assert!(result.content.contains("VARIABLES"));
    assert!(result.content.contains("state"));
    assert!(result.content.contains("Init =="));
    assert!(result.content.contains("Next =="));
    assert!(result.content.contains("TypeOK =="));

    // Check states
    assert!(result.content.contains("pending"));
    assert!(result.content.contains("settled"));
    assert!(result.content.contains("failed"));

    // Check LTL properties are transpiled
    assert!(result.content.contains("Prop_eventual_settlement"));
    assert!(result.content.contains("[]")); // always
    assert!(result.content.contains("<>")); // eventually
    assert!(result.content.contains("\\U")); // until

    // Check fairness
    assert!(result.content.contains("WF")); // weak fairness
    assert!(result.content.contains("SF")); // strong fairness

    // Verify property count
    assert_eq!(result.properties.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// EXTENDED QUANTIFICATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_constraint_with_comparison() {
    let source = r#"
system FeeSystem {
    constraint fee_bounded {
        forall c in [ContractA, ContractB]: check c.fee <= c.budget
    }

    constraint latency_sla {
        forall op in [Operation1]: check op.latency < 100
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.constraints.len(), 2);

            // Check fee_bounded constraint
            let c = &s.constraints[0];
            assert_eq!(c.name, "fee_bounded");
            match &c.rules[0] {
                ConstraintRule::Forall { var, body, .. } => {
                    assert_eq!(var, "c");
                    match body.as_ref() {
                        ConstraintRule::Comparison { lhs, op, rhs } => {
                            assert_eq!(*op, ComparisonOp::Le);
                            // Check lhs is c.fee
                            match lhs {
                                Expr::DottedName(name) => assert_eq!(name, "c.fee"),
                                _ => panic!("expected DottedName for lhs"),
                            }
                            // Check rhs is c.budget
                            match rhs {
                                Expr::DottedName(name) => assert_eq!(name, "c.budget"),
                                _ => panic!("expected DottedName for rhs"),
                            }
                        }
                        _ => panic!("expected Comparison"),
                    }
                }
                _ => panic!("expected Forall"),
            }

            // Check latency_sla constraint
            let c = &s.constraints[1];
            assert_eq!(c.name, "latency_sla");
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_constraint_with_numeric_comparison() {
    let source = r#"
system BudgetSystem {
    constraint count_bounded {
        forall c in [Contract]: check c.total > 10
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.constraints.len(), 1);
            let c = &s.constraints[0];
            assert_eq!(c.name, "count_bounded");

            match &c.rules[0] {
                ConstraintRule::Forall { body, .. } => {
                    match body.as_ref() {
                        ConstraintRule::Comparison { lhs: _, op, rhs } => {
                            assert_eq!(*op, ComparisonOp::Gt);
                            // RHS should be an integer
                            match rhs {
                                Expr::Int(n) => assert_eq!(*n, 10),
                                _ => panic!("expected Int for rhs"),
                            }
                        }
                        _ => panic!("expected Comparison"),
                    }
                }
                _ => panic!("expected Forall"),
            }
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn parse_all_comparison_operators() {
    // Test all six comparison operators with check keyword
    let operators = [
        ("<=", ComparisonOp::Le),
        (">=", ComparisonOp::Ge),
        ("<", ComparisonOp::Lt),
        (">", ComparisonOp::Gt),
        ("==", ComparisonOp::Eq),
        ("!=", ComparisonOp::Ne),
    ];

    for (op_str, expected_op) in operators {
        let source = format!(
            r#"
system Test {{
    constraint check_op {{
        forall x in [A]: check a {} b
    }}
}}
"#,
            op_str
        );
        let top_levels = parser::parse(&source).unwrap();

        match &top_levels[0] {
            TopLevel::System(s) => {
                let c = &s.constraints[0];
                match &c.rules[0] {
                    ConstraintRule::Forall { body, .. } => {
                        match body.as_ref() {
                            ConstraintRule::Comparison { op, .. } => {
                                assert_eq!(*op, expected_op, "Failed for operator {}", op_str);
                            }
                            _ => panic!("expected Comparison for operator {}", op_str),
                        }
                    }
                    _ => panic!("expected Forall for operator {}", op_str),
                }
            }
            _ => panic!("expected System"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BEHAVIOR COMPOSITION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_behavior_with_composes() {
    let source = r#"
system OrderSystem {
    behavior CombinedFlow composes [PaymentFlow, ShippingFlow] {
        states {
            initiated { initial: true }
            completed { terminal: true }
        }
    }
}
"#;
    let top_levels = parser::parse(source).unwrap();

    match &top_levels[0] {
        TopLevel::System(s) => {
            assert_eq!(s.behaviors.len(), 1);
            let b = &s.behaviors[0];
            assert_eq!(b.name, "CombinedFlow");
            assert_eq!(b.composes, vec!["PaymentFlow", "ShippingFlow"]);
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn test_behavior_composition() {
    use intent::behavioral::{compose_behaviors, CompositionConfig};
    use intent::parser::ast::{StateDecl, TransitionDecl};

    // Create two simple behaviors
    let b1 = BehaviorDecl {
        name: "Flow1".to_string(),
        states: vec![
            make_state("a1", true, false),
            make_state("a2", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("a1".to_string()),
            to: intent::parser::ast::TransitionTarget::State("a2".to_string()),
            on_event: "go".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    let b2 = BehaviorDecl {
        name: "Flow2".to_string(),
        states: vec![
            make_state("b1", true, false),
            make_state("b2", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("b1".to_string()),
            to: intent::parser::ast::TransitionTarget::State("b2".to_string()),
            on_event: "go".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    // Compose them
    let result = compose_behaviors(
        "Combined",
        &[("Flow1", &b1), ("Flow2", &b2)],
        &CompositionConfig::default(),
    )
    .unwrap();

    // Should have all 4 states
    assert_eq!(result.states.len(), 4);
    // Should have both transitions
    assert_eq!(result.transitions.len(), 2);
    // Should detect multiple initial states as a conflict
    assert!(result.has_conflicts());
}

#[test]
fn test_behavior_refinement() {
    use intent::behavioral::validate_refinement;
    use intent::parser::ast::{RefinementMap, StateDecl, TransitionDecl};

    // Abstract spec
    let abstract_spec = BehaviorDecl {
        name: "Abstract".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("done", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("idle".to_string()),
            to: intent::parser::ast::TransitionTarget::State("done".to_string()),
            on_event: "finish".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    // Concrete implementation (with additional internal state)
    let concrete = BehaviorDecl {
        name: "Concrete".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("processing", false, false),
            make_state("done", false, true),
        ],
        transitions: vec![
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("idle".to_string()),
                to: intent::parser::ast::TransitionTarget::State("processing".to_string()),
                on_event: "start".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("processing".to_string()),
                to: intent::parser::ast::TransitionTarget::State("done".to_string()),
                on_event: "finish".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
        ],
        ..Default::default()
    };

    // Map: idle->idle, processing->idle (stuttering), done->done
    let map = RefinementMap {
        mappings: vec![
            ("idle".to_string(), vec!["idle".to_string(), "processing".to_string()]),
            ("done".to_string(), vec!["done".to_string()]),
        ],
    };

    let result = validate_refinement(&concrete, &abstract_spec, &Some(map)).unwrap();

    assert!(result.is_valid, "Refinement should be valid: {:?}", result.violations);
}

#[test]
fn test_refinement_detects_violations() {
    use intent::behavioral::{validate_refinement, ViolationType};
    use intent::parser::ast::{StateDecl, TransitionDecl};

    // Abstract spec with required transition
    let abstract_spec = BehaviorDecl {
        name: "Abstract".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("done", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("idle".to_string()),
            to: intent::parser::ast::TransitionTarget::State("done".to_string()),
            on_event: "finish".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    // Concrete with WRONG event name
    let concrete = BehaviorDecl {
        name: "Concrete".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("done", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("idle".to_string()),
            to: intent::parser::ast::TransitionTarget::State("done".to_string()),
            on_event: "wrong_event".to_string(), // Different from abstract!
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    let result = validate_refinement(&concrete, &abstract_spec, &None).unwrap();

    assert!(!result.is_valid);
    let illegal = result.violations_of_type(ViolationType::IllegalTransition);
    assert!(!illegal.is_empty(), "Should detect illegal transition");
}

#[test]
fn test_tla_generation_with_data_variables() {
    use intent::transpile::tla::generate;
    use intent::parser::ast::{EffectKind, EffectStmt, Expr, StateDecl, TransitionDecl};
    use std::path::Path;

    // Create a behavior with guards that reference data variables
    let behavior = BehaviorDecl {
        name: "DataFlow".to_string(),
        states: vec![
            make_state("init", true, false),
            make_state("processing", false, false),
            make_state("done", false, true),
        ],
        transitions: vec![
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("init".to_string()),
                to: intent::parser::ast::TransitionTarget::State("processing".to_string()),
                on_event: "start".to_string(),
                guard: Some(Expr::CompOp {
                    lhs: Box::new(Expr::Ident("count".to_string())),
                    op: intent::parser::ast::ComparisonOp::Gt,
                    rhs: Box::new(Expr::Int(0)),
                }),
                effects: vec![EffectStmt {
                    kind: EffectKind::Emit {
                        name: "Started".to_string(),
                        args: vec![Expr::Ident("count".to_string())],
                    },
                }],
                timing: None,
                span: Span::synthetic(),
            },
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("processing".to_string()),
                to: intent::parser::ast::TransitionTarget::State("done".to_string()),
                on_event: "finish".to_string(),
                guard: Some(Expr::Ident("valid".to_string())),
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
        ],
        ..Default::default()
    };

    let result = generate(&behavior, "Test", Path::new(".")).unwrap();

    // Should include data variables
    assert!(result.content.contains("count"), "Should include count variable");
    assert!(result.content.contains("valid"), "Should include valid variable");

    // Should have proper initialization
    assert!(result.content.contains("count = 0"), "count should be initialized to 0");
    assert!(result.content.contains("valid = FALSE"), "valid should be initialized to FALSE");

    // Should have UNCHANGED for data vars in transitions
    assert!(result.content.contains("UNCHANGED <<count, valid>>"));

    // Should emit event with args
    assert!(result.content.contains("[type |-> \"Started\", args |-> <<count>>]"));
}

#[test]
fn test_tla_generation_composed_behavior() {
    use intent::transpile::tla::generate_composed;
    use intent::parser::ast::{StateDecl, TransitionDecl};

    // Create two behaviors to compose
    let b1 = BehaviorDecl {
        name: "FlowA".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("active", false, false),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("idle".to_string()),
            to: intent::parser::ast::TransitionTarget::State("active".to_string()),
            on_event: "start".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    let b2 = BehaviorDecl {
        name: "FlowB".to_string(),
        states: vec![
            make_state("active", false, false),
            make_state("done", false, true),
        ],
        transitions: vec![TransitionDecl {
            from: intent::parser::ast::TransitionSource::State("active".to_string()),
            to: intent::parser::ast::TransitionTarget::State("done".to_string()),
            on_event: "finish".to_string(),
            guard: None,
            effects: vec![],
            timing: None,
            span: Span::synthetic(),
        }],
        ..Default::default()
    };

    // Target behavior that composes the two
    let composed = BehaviorDecl {
        name: "Combined".to_string(),
        composes: vec!["FlowA".to_string(), "FlowB".to_string()],
        ..Default::default()
    };

    let result = generate_composed(
        &composed,
        &[("FlowA", &b1), ("FlowB", &b2)],
        "Test",
        None,
    )
    .unwrap();

    // Should have all three unique states
    assert!(result.content.contains("idle"), "Should include idle state");
    assert!(result.content.contains("active"), "Should include active state");
    assert!(result.content.contains("done"), "Should include done state");

    // Should have both transitions
    assert!(result.content.contains("idle_start"), "Should include start transition");
    assert!(result.content.contains("active_finish"), "Should include finish transition");

    // Should note composition source
    assert!(result.content.contains("Composed from:"), "Should note composition");
}

#[test]
fn test_parallel_composition_tla_generation() {
    use intent::behavioral::{parallel_compose, ParallelConfig};
    use intent::transpile::tla::generate;
    use intent::parser::ast::{StateDecl, TransitionDecl};
    use std::path::Path;

    // Create two concurrent behaviors
    let producer = BehaviorDecl {
        name: "Producer".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("producing", false, false),
        ],
        transitions: vec![
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("idle".to_string()),
                to: intent::parser::ast::TransitionTarget::State("producing".to_string()),
                on_event: "produce".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("producing".to_string()),
                to: intent::parser::ast::TransitionTarget::State("idle".to_string()),
                on_event: "done".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
        ],
        ..Default::default()
    };

    let consumer = BehaviorDecl {
        name: "Consumer".to_string(),
        states: vec![
            make_state("waiting", true, false),
            make_state("consuming", false, false),
        ],
        transitions: vec![
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("waiting".to_string()),
                to: intent::parser::ast::TransitionTarget::State("consuming".to_string()),
                on_event: "consume".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
            TransitionDecl {
                from: intent::parser::ast::TransitionSource::State("consuming".to_string()),
                to: intent::parser::ast::TransitionTarget::State("waiting".to_string()),
                on_event: "done".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
        ],
        ..Default::default()
    };

    // Parallel composition with "done" as synchronized event
    let config = ParallelConfig {
        sync_events: vec!["done".to_string()],
        interleaving: true,
        ..Default::default()
    };

    let parallel = parallel_compose(
        "ProducerConsumer",
        ("Producer", &producer),
        ("Consumer", &consumer),
        &config,
    ).unwrap();

    // Convert to BehaviorDecl and generate TLA+
    let behavior = parallel.to_behavior_decl();
    let result = generate(&behavior, "Test", Path::new(".")).unwrap();

    // Should have product states
    assert!(result.content.contains("idle_x_waiting"), "Should have initial product state");
    assert!(result.content.contains("producing_x_consuming"), "Should have composite state");

    // Should have synchronized transition
    assert!(result.content.contains("sync_done"), "Should have synchronized done transition");

    // Should have interleaved transitions
    assert!(result.content.contains("Producer_produce"), "Should have interleaved Producer transition");
    assert!(result.content.contains("Consumer_consume"), "Should have interleaved Consumer transition");
}
