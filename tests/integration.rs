use std::path::Path;

use intent::parser;
use intent::parser::ast::*;
use intent::structural;

#[test]
fn parse_resilient_storage_intent() {
    let source = std::fs::read_to_string("../../formal/intent/resilient_storage.intent")
        .expect("resilient_storage.intent should exist");
    let concerns = parser::parse(&source).unwrap();

    assert_eq!(concerns.len(), 1);
    let c = &concerns[0];
    assert_eq!(c.name, "ResilientStorage");
    assert!(c.span.is_some());

    // Count item types
    let scopes: Vec<_> = c
        .items
        .iter()
        .filter(|i| matches!(i, ConcernItem::Scope(_)))
        .collect();
    let constraints: Vec<_> = c
        .items
        .iter()
        .filter(|i| matches!(i, ConcernItem::Constraint(_)))
        .collect();
    let applies: Vec<_> = c
        .items
        .iter()
        .filter(|i| matches!(i, ConcernItem::Apply(_)))
        .collect();

    assert_eq!(scopes.len(), 3);
    assert_eq!(constraints.len(), 1);
    assert_eq!(applies.len(), 2);
}

#[test]
fn parse_layered_architecture_intent() {
    let source = std::fs::read_to_string("../../formal/intent/layered_architecture.intent")
        .expect("layered_architecture.intent should exist");
    let concerns = parser::parse(&source).unwrap();

    assert_eq!(concerns.len(), 1);
    let c = &concerns[0];
    assert_eq!(c.name, "LayeredArchitecture");

    // 4 layers + 1 constraint + decided + rejected + revisit = 8
    assert_eq!(c.items.len(), 8);

    // Check layers
    let layers: Vec<_> = c.items.iter().filter(|i| matches!(i, ConcernItem::Layer(_))).collect();
    assert_eq!(layers.len(), 4);

    // Check explicit constraints
    let constraints: Vec<_> = c.items.iter().filter_map(|i| {
        if let ConcernItem::Constraint(cd) = i { Some(cd) } else { None }
    }).collect();

    assert_eq!(constraints.len(), 1);
    // The explicit constraint is MustNotReference (auth_boundary)
    assert!(matches!(&constraints[0].rules[0], ConstraintRule::MustNotReference { .. }));
}

#[test]
fn parse_both_intent_files() {
    let dir = Path::new("../../formal/intent");
    let mut all = Vec::new();

    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "intent") {
            let source = std::fs::read_to_string(entry.path()).unwrap();
            let concerns = parser::parse(&source).unwrap();
            all.extend(concerns);
        }
    }

    assert_eq!(all.len(), 2);
    let names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"ResilientStorage"));
    assert!(names.contains(&"LayeredArchitecture"));
}

#[test]
fn structural_check_against_codebase() {
    let codebase = Path::new("../../crates/nxbrain-core/src");
    if !codebase.exists() {
        // Skip if codebase not present (e.g., CI without full checkout)
        return;
    }

    let source = std::fs::read_to_string("../../formal/intent/resilient_storage.intent").unwrap();
    let concerns = parser::parse(&source).unwrap();
    let results = structural::check(&concerns, codebase).unwrap();

    // Should produce at least 2 results (storage_boundary + no_direct_backend_access)
    assert!(results.len() >= 2, "expected at least 2 results, got {}", results.len());

    // All results should have concern field set
    for r in &results {
        assert_eq!(r.concern, "ResilientStorage");
    }
}

#[test]
fn structural_check_layered_against_codebase() {
    let codebase = Path::new("../../crates/nxbrain-core/src");
    if !codebase.exists() {
        return;
    }

    let source = std::fs::read_to_string("../../formal/intent/layered_architecture.intent").unwrap();
    let concerns = parser::parse(&source).unwrap();
    let results = structural::check(&concerns, codebase).unwrap();

    // Should produce 7 results: 1 explicit constraint + 6 layer-generated constraints
    // (4 layers produce C(4,2) = 6 must_not_depend_on pairs)
    assert_eq!(results.len(), 7);

    for r in &results {
        assert_eq!(r.concern, "LayeredArchitecture");
    }
}
