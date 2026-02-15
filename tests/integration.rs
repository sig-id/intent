use intent::parser;
use intent::parser::ast::*;
use intent::plan;
use intent::structural;

#[test]
fn parse_full_concern_roundtrip() {
    let source = r#"
concern Example {
    scope backends { [FooClient, BarClient] }
    scope boundary { only [storage] accesses backends }
    scope processing { [services, pipeline] }

    constraint no_leak {
        processing must_not depend_on backends
    }

    layer presentation { [routes] }
    layer application { [services] }
    layer infrastructure { [storage] }

    apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
        to Coordinator.breaker {
            refines "spec.tla"
        }

    decided because {
        "reason one"
        "reason two"
    }

    rejected alternatives {
        alt_a: "bad because X"
        alt_b: "bad because Y"
    }

    revisit when {
        "condition changes"
    }
}
"#;
    let concerns = parser::parse(source).unwrap();
    assert_eq!(concerns.len(), 1);

    let c = &concerns[0];
    assert_eq!(c.name, "Example");

    let scopes: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Scope(_))).collect();
    let constraints: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Constraint(_))).collect();
    let layers: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Layer(_))).collect();
    let applies: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Apply(_))).collect();

    assert_eq!(scopes.len(), 3);
    assert_eq!(constraints.len(), 1);
    assert_eq!(layers.len(), 3);
    assert_eq!(applies.len(), 1);
}

#[test]
fn structural_check_with_tempdir() {
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(tmp.path().join("lib.rs"), "mod routes;\nmod services;\nmod storage;\n").unwrap();

    for dir in &["routes", "services", "storage"] {
        std::fs::create_dir(tmp.path().join(dir)).unwrap();
    }

    std::fs::write(tmp.path().join("routes/mod.rs"), "pub struct Router;\n").unwrap();
    std::fs::write(
        tmp.path().join("services/mod.rs"),
        "pub fn init() {}\n",
    ).unwrap();
    std::fs::write(
        tmp.path().join("storage/mod.rs"),
        "pub struct DbClient;\n",
    ).unwrap();

    let source = r#"
concern Layered {
    layer presentation { [routes] }
    layer application { [services] }
    layer infrastructure { [storage] }
}
"#;
    let concerns = parser::parse(source).unwrap();
    let results = structural::check(&concerns, tmp.path()).unwrap();

    // 3 layers -> C(3,2) = 3 implicit must_not_depend_on constraints
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.passed), "clean codebase should pass all layer constraints");
}

#[test]
fn structural_detects_layer_violation() {
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(tmp.path().join("lib.rs"), "mod routes;\nmod storage;\n").unwrap();

    std::fs::create_dir(tmp.path().join("routes")).unwrap();
    std::fs::create_dir(tmp.path().join("storage")).unwrap();

    std::fs::write(tmp.path().join("routes/mod.rs"), "pub struct Router;\n").unwrap();
    // storage depends on routes — violation
    std::fs::write(
        tmp.path().join("storage/mod.rs"),
        "use crate::routes::Router;\npub struct StorageClient;\n",
    ).unwrap();

    let source = r#"
concern Layered {
    layer presentation { [routes] }
    layer infrastructure { [storage] }
}
"#;
    let concerns = parser::parse(source).unwrap();
    let results = structural::check(&concerns, tmp.path()).unwrap();

    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(!failed.is_empty(), "storage depending on routes should be a violation");
}

#[test]
fn parse_gentix_style_full_concern() {
    let source = r#"
concern ContractLifecycle {
    scope on_chain lang solidity {
        [EscrowContract, ContractRegistry, ReputationBondRegistry,
         GENXToken, StakingContract, PoIRegistry, WitnessProgramRegistry,
         DisputeRegistry]
    }

    scope engines lang typescript {
        [ContractEngine, ValidationEngine, WorkflowEngine]
    }

    scope clients lang typescript {
        [CLI, SDK, MCPServer, BrowserApp]
    }

    // Layered architecture
    layer client { clients }
    layer gateway { [GatewayFederation] }
    layer engine { engines }
    layer chain { on_chain }

    // Contract lifecycle state machine
    statemachine Lifecycle {
        states [PUBLISH, CLAIM, EXECUTE, DELIVER, VERIFY,
                SETTLE, CANCEL, DISPUTE, PENDING_WITNESS]
        initial PUBLISH
        terminal [SETTLE, CANCEL]

        transition PUBLISH -> CLAIM
        transition CLAIM -> EXECUTE
        transition CLAIM -> CANCEL
        transition EXECUTE -> DELIVER
        transition DELIVER -> VERIFY
        transition VERIFY -> SETTLE
        transition VERIFY -> DISPUTE
        transition VERIFY -> PENDING_WITNESS
        transition PENDING_WITNESS -> SETTLE
    }

    // Behavioral patterns
    apply Escrow(lock_on: "publish", release_on: "consensus_pass",
                 refund_on: "consensus_fail", timeout: 7d)
        to ContractEngine.escrow {
            refines "formal/tla/Escrow.tla"
        }

    apply CommitReveal(commit_timeout: 30m, reveal_timeout: 15m,
                       penalty_on_no_reveal: 0.005)
        to ValidationEngine.consensus {
            refines "formal/tla/CommitReveal.tla"
        }

    // Cross-system coherence
    bridge escrow_events {
        source ContractEngine lang typescript
        sink EscrowContract lang solidity
        events ["EscrowDeposited", "EscrowReleased", "EscrowRefunded"]
        bidirectional
    }

    // Economic parameters
    parameter witness_fee_rate: 0.02
    parameter platform_fee_tier2: 0.03

    invariant provider_net_positive {
        1.0 - witness_fee_rate - platform_fee_tier2 > 0
    }

    // Compositional constraints
    constraint contract_coherence {
        status planned
        when_present milestones requires [budget, quality_threshold]
        when_present stages requires [retry_budget]
        covers ["scenario_7.2_subjective_job", "scenario_7.3_deterministic"]
    }

    decided because {
        "Unified contract primitive eliminates type proliferation."
        "Escrow protects consumers; commit-reveal prevents witness copying."
        "On-chain/off-chain bridge ensures event coherence across trust boundary."
    }

    rejected alternatives {
        separate_types: "Distinct task/capability/challenge types caused combinatorial explosion."
        immediate_payment: "No escrow means no consumer protection for subjective work."
    }

    revisit when {
        "A second chain is supported (cross-chain bridge coherence needed)"
        "Contract schema adds new feature sets beyond stages/milestones/conditions"
    }
}
"#;
    let concerns = parser::parse(source).unwrap();
    assert_eq!(concerns.len(), 1);

    let c = &concerns[0];
    assert_eq!(c.name, "ContractLifecycle");

    // Count items by type
    let scopes: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Scope(_))).collect();
    let layers: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Layer(_))).collect();
    let statemachines: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::StateMachine(_))).collect();
    let applies: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Apply(_))).collect();
    let bridges: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Bridge(_))).collect();
    let params: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Parameter(_))).collect();
    let invariants: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Invariant(_))).collect();
    let constraints: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Constraint(_))).collect();

    assert_eq!(scopes.len(), 3);
    assert_eq!(layers.len(), 4);
    assert_eq!(statemachines.len(), 1);
    assert_eq!(applies.len(), 2);
    assert_eq!(bridges.len(), 1);
    assert_eq!(params.len(), 2);
    assert_eq!(invariants.len(), 1);
    assert_eq!(constraints.len(), 1);

    // Verify state machine details
    if let ConcernItem::StateMachine(sm) = &statemachines[0] {
        assert_eq!(sm.name, "Lifecycle");
        assert_eq!(sm.states.len(), 9);
        assert_eq!(sm.initial, "PUBLISH");
        assert_eq!(sm.terminal, vec!["SETTLE", "CANCEL"]);
        assert_eq!(sm.transitions.len(), 9);
    }

    // Verify constraint status and covers
    if let ConcernItem::Constraint(con) = &constraints[0] {
        assert_eq!(con.status, Some(ConstraintStatus::Planned));
        assert_eq!(con.covers.len(), 2);
    }
}

#[test]
fn plan_mode_validates_gentix_concern() {
    let source = r#"
concern Economics {
    parameter witness_rewards: 0.60
    parameter poi_challenges: 0.15
    parameter audit_fund: 0.10
    parameter micro_task: 0.10
    parameter referral: 0.05

    invariant emission_sums_to_one {
        witness_rewards + poi_challenges + audit_fund + micro_task + referral == 1.0
    }

    parameter platform_fee_tier0: 0.01
    parameter platform_fee_tier1: 0.02
    parameter platform_fee_tier2: 0.03
    parameter platform_fee_tier3: 0.04

    invariant fee_ordering {
        platform_fee_tier0 < platform_fee_tier1,
        platform_fee_tier1 < platform_fee_tier2,
        platform_fee_tier2 < platform_fee_tier3
    }

    statemachine ContractLifecycle {
        states [PUBLISH, CLAIM, EXECUTE, DELIVER, VERIFY, SETTLE, CANCEL]
        initial PUBLISH
        terminal [SETTLE, CANCEL]

        transition PUBLISH -> CLAIM
        transition CLAIM -> EXECUTE
        transition CLAIM -> CANCEL
        transition EXECUTE -> DELIVER
        transition DELIVER -> VERIFY
        transition VERIFY -> SETTLE
    }
}
"#;
    let concerns = parser::parse(source).unwrap();
    let results = plan::validate(&concerns).unwrap();
    assert_eq!(results.len(), 1);

    let checks = &results[0].checks;
    // All invariants should pass
    let inv_checks: Vec<_> = checks.iter().filter(|c| c.name.contains("emission") || c.name.contains("fee")).collect();
    assert!(!inv_checks.is_empty(), "should have invariant checks");
    assert!(
        inv_checks.iter().all(|c| c.passed),
        "all invariant checks should pass: {inv_checks:?}"
    );

    // State machine checks should pass
    let sm_checks: Vec<_> = checks.iter().filter(|c| c.name.starts_with("ContractLifecycle")).collect();
    assert!(!sm_checks.is_empty(), "should have state machine checks");
    assert!(
        sm_checks.iter().all(|c| c.passed),
        "all state machine checks should pass: {sm_checks:?}"
    );
}
