//! Integration tests for TLA+ primitive expressions.
//!
//! Tests the parsing and TLA+ transpilation of CHOOSE, LET, IF-THEN-ELSE,
//! CASE, SUBSET, UNION, DOMAIN, EXCEPT, and quantifiers.

use intent::parser::{self, ast::*};

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
fn parse_choose_expression() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant select_min {
            choose(x, set { 1, 2, 3 }, x > 0)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Choose { var, .. } => assert_eq!(var, "x"),
        _ => panic!("expected Choose"),
    }
}

#[test]
fn parse_let_in_expression() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant with_let {
            let_in { x = 5, y = 10 } in ( x + y )
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Let { bindings, .. } => assert_eq!(bindings.len(), 2),
        _ => panic!("expected Let"),
    }
}

#[test]
fn parse_if_then_else_expression() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant conditional {
            if x > 0 then 1 else 0
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::IfThenElse { .. } => {}
        _ => panic!("expected IfThenElse"),
    }
}

#[test]
fn parse_case_expression() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant cases {
            case { x == 1 => "one", x == 2 => "two", default: "other" }
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Case { arms, default } => {
            assert_eq!(arms.len(), 2);
            assert!(default.is_some());
        }
        _ => panic!("expected Case"),
    }
}

#[test]
fn parse_set_operations() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant sets {
            subset(S)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Subset(_) => {}
        _ => panic!("expected Subset"),
    }
}

#[test]
fn parse_record_literal() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant record {
            rec { name: "test", value: 42 }
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Record(fields) => assert_eq!(fields.len(), 2),
        _ => panic!("expected Record"),
    }
}

#[test]
fn parse_tuple_literal() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant tup {
            tuple(1, 2, 3)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Tuple(elems) => assert_eq!(elems.len(), 3),
        _ => panic!("expected Tuple"),
    }
}

#[test]
fn parse_function_literal() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant fun_lit {
            fun(x, S, x + 1)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::FunctionLiteral { var, .. } => assert_eq!(var, "x"),
        _ => panic!("expected FunctionLiteral"),
    }
}

#[test]
fn parse_quantifiers_in_expr() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant quant {
            forall_expr(x, S, x > 0)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Forall { var, .. } => assert_eq!(var, "x"),
        _ => panic!("expected Forall"),
    }
}

#[test]
fn parse_assume_expression() {
    let source = r#"
system X {
    behavior B {
        states { idle active }
        invariant assumption {
            assume(N > 0)
        }
    }
}
"#;
    let top = parser::parse(source).unwrap();
    let system = match &top[0] {
        TopLevel::System(s) => s,
        _ => panic!("expected System"),
    };
    let inv = &system.behaviors[0].invariants[0];
    match &inv.expr {
        Expr::Assume(_) => {}
        _ => panic!("expected Assume"),
    }
}

#[test]
fn transpile_tla_primitives() {
    use intent::transpile::tla;
    use std::path::Path;

    let behavior = BehaviorDecl {
        name: "Test".to_string(),
        states: vec![
            make_state("idle", true, false),
            make_state("done", false, true),
        ],
        transitions: vec![
            TransitionDecl {
                from: TransitionSource::State("idle".to_string()),
                to: TransitionTarget::State("done".to_string()),
                on_event: "go".to_string(),
                guard: None,
                effects: vec![],
                timing: None,
                span: Span::synthetic(),
            },
        ],
        invariants: vec![
            InvariantDecl {
                name: "choose_test".to_string(),
                expr: Expr::Choose {
                    var: "x".to_string(),
                    domain: Box::new(Expr::Ident("S".to_string())),
                    predicate: Box::new(Expr::CompOp {
                        lhs: Box::new(Expr::Ident("x".to_string())),
                        op: ComparisonOp::Gt,
                        rhs: Box::new(Expr::Int(0)),
                    }),
                },
            },
        ],
        ..Default::default()
    };

    let result = tla::generate(&behavior, "System", Path::new(".")).unwrap();
    
    // Check that CHOOSE is in the output
    assert!(result.content.contains("CHOOSE x \\in S : (x > 0)"), 
        "Expected CHOOSE in output, got:\n{}", result.content);
}
