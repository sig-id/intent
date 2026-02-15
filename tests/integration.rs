use intent::parser;
use intent::parser::ast::*;
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
