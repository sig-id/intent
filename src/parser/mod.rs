pub mod ast;

use lalrpop_util::lalrpop_mod;
lalrpop_mod!(
    #[allow(clippy::all)]
    #[allow(unused)]
    pub intent,
    "/parser/intent.rs"
);

use anyhow::Result;
use ast::Concern;

/// Helper: convert a Vec of &str to Vec<String>.
pub fn strs(v: Vec<&str>) -> Vec<String> {
    v.into_iter().map(|s| s.to_string()).collect()
}

/// Helper: strip surrounding quotes from a string literal.
pub fn unquote(s: &str) -> String {
    s[1..s.len() - 1].to_string()
}

/// Parse an Intent source string into a list of concerns.
pub fn parse(source: &str) -> Result<Vec<Concern>> {
    let parser = intent::FileParser::new();
    parser.parse(source).map_err(|e| {
        let msg = format_parse_error(source, e);
        anyhow::anyhow!("{msg}")
    })
}

fn format_parse_error(
    source: &str,
    err: lalrpop_util::ParseError<usize, lalrpop_util::lexer::Token<'_>, &str>,
) -> String {
    match err {
        lalrpop_util::ParseError::InvalidToken { location } => {
            let (line, col) = offset_to_line_col(source, location);
            format!("invalid token at {line}:{col}")
        }
        lalrpop_util::ParseError::UnrecognizedToken {
            token: (start, tok, _),
            expected,
        } => {
            let (line, col) = offset_to_line_col(source, start);
            let expected_str = expected.join(", ");
            format!("unexpected {tok} at {line}:{col}, expected one of: {expected_str}")
        }
        lalrpop_util::ParseError::UnrecognizedEof { location, expected } => {
            let (line, col) = offset_to_line_col(source, location);
            let expected_str = expected.join(", ");
            format!("unexpected end of file at {line}:{col}, expected one of: {expected_str}")
        }
        lalrpop_util::ParseError::ExtraToken {
            token: (start, tok, _),
        } => {
            let (line, col) = offset_to_line_col(source, start);
            format!("extra token {tok} at {line}:{col}")
        }
        lalrpop_util::ParseError::User { error } => format!("parse error: {error}"),
    }
}

fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// Internal helper types used by the LALRPOP grammar
#[derive(Debug)]
pub enum ConstraintRuleOrCover {
    Rule(ast::ConstraintRule),
    Covers(Vec<String>),
    Status(ast::ConstraintStatus),
}

#[derive(Debug)]
pub enum SmItemParsed {
    States(Vec<String>),
    Initial(String),
    Terminal(Vec<String>),
    Transition(String, String),
    Invariant(ast::SmInvariant),
    Refines(String),
}

#[derive(Debug)]
pub enum BridgeItemParsed {
    Source(ast::BridgeEndpoint),
    Sink(ast::BridgeEndpoint),
    Events(Vec<String>),
    Constraint(ast::BridgeConstraintType),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ast::*;
    use super::*;

    #[test]
    fn test_parse_empty_concern() {
        let concerns = parse("concern Empty { }").unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "Empty");
        assert!(concerns[0].items.is_empty());
        assert!(concerns[0].span.is_some());
    }

    #[test]
    fn test_parse_scope_entity_list() {
        let concerns = parse(
            r#"concern X {
                scope backends {
                    [DgraphClient, MilvusClient]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(concerns[0].items.len(), 1);
        match &concerns[0].items[0] {
            ConcernItem::Scope(s) => {
                assert_eq!(s.name, "backends");
                assert_eq!(
                    s.kind,
                    ScopeKind::EntityList(vec![
                        "DgraphClient".into(),
                        "MilvusClient".into()
                    ])
                );
                assert!(s.within.is_none());
            }
            other => panic!("expected Scope, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_scope_only_accesses() {
        let concerns = parse(
            r#"concern X {
                scope boundary {
                    only [storage] accesses backends
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Scope(s) => {
                assert_eq!(
                    s.kind,
                    ScopeKind::OnlyAccesses {
                        accessors: vec!["storage".into()],
                        target: "backends".into(),
                    }
                );
            }
            other => panic!("expected Scope, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_scope_with_within() {
        let concerns = parse(
            r#"concern X {
                scope backends {
                    [DgraphClient]
                    within [storage, pipeline]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Scope(s) => {
                assert_eq!(s.name, "backends");
                assert_eq!(
                    s.within,
                    Some(vec!["storage".into(), "pipeline".into()])
                );
            }
            other => panic!("expected Scope, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_must_not_depend_on() {
        let concerns = parse(
            r#"concern X {
                constraint no_leak {
                    [services, pipeline] must_not depend_on backends
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.name, "no_leak");
                assert_eq!(c.rules.len(), 1);
                match &c.rules[0] {
                    ConstraintRule::MustNotDependOn { from, target } => {
                        assert_eq!(from, &["services", "pipeline"]);
                        assert_eq!(target, "backends");
                    }
                    other => panic!("expected MustNotDependOn, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_must_not_reference() {
        let concerns = parse(
            r#"concern X {
                constraint auth_boundary {
                    [services, storage] must_not reference [AuthMiddleware, SessionCookie]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.name, "auth_boundary");
                match &c.rules[0] {
                    ConstraintRule::MustNotReference { from, targets } => {
                        assert_eq!(from, &["services", "storage"]);
                        assert_eq!(targets, &["AuthMiddleware", "SessionCookie"]);
                    }
                    other => panic!("expected MustNotReference, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_occur_only_in() {
        let concerns = parse(
            r#"concern X {
                constraint pattern_loc {
                    AuthMiddleware occur_only_in [routes]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::OccurOnlyIn { pattern, modules } => {
                        assert_eq!(pattern, "AuthMiddleware");
                        assert_eq!(modules, &["routes"]);
                    }
                    other => panic!("expected OccurOnlyIn, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_apply() {
        let concerns = parse(
            r#"concern X {
                apply CircuitBreaker(threshold: 5, timeout: 30s, probe_limit: 2)
                    to StorageCoordinator.dgraph_circuit_breaker {
                        refines "formal/tla/CircuitBreaker.tla"
                    }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Apply(a) => {
                assert_eq!(a.pattern, "CircuitBreaker");
                assert_eq!(
                    a.params,
                    vec![
                        ("threshold".into(), ParamValue::Int(5)),
                        ("timeout".into(), ParamValue::Duration(30)),
                        ("probe_limit".into(), ParamValue::Int(2)),
                    ]
                );
                assert_eq!(a.target, "StorageCoordinator.dgraph_circuit_breaker");
                assert_eq!(
                    a.refines.as_deref(),
                    Some("formal/tla/CircuitBreaker.tla")
                );
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_decided() {
        let concerns = parse(
            r#"concern X {
                decided because {
                    "reason one"
                    "reason two"
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::DecidedBecause(reasons) => {
                assert_eq!(reasons, &["reason one", "reason two"]);
            }
            other => panic!("expected DecidedBecause, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rejected() {
        let concerns = parse(
            r#"concern X {
                rejected alternatives {
                    retry_only: "bad because pile-up"
                    failover: "no replicas"
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::RejectedAlternatives(alts) => {
                assert_eq!(alts.len(), 2);
                assert_eq!(alts[0].0, "retry_only");
                assert_eq!(alts[1].0, "failover");
            }
            other => panic!("expected RejectedAlternatives, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_revisit() {
        let concerns = parse(
            r#"concern X {
                revisit when {
                    "HA config added"
                    "third backend added"
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::RevisitWhen(conditions) => {
                assert_eq!(conditions, &["HA config added", "third backend added"]);
            }
            other => panic!("expected RevisitWhen, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_use_scope() {
        let concerns = parse(
            r#"concern X {
                use ResilientStorage.storage_backends
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::UseScope { concern, scope } => {
                assert_eq!(concern, "ResilientStorage");
                assert_eq!(scope, "storage_backends");
            }
            other => panic!("expected UseScope, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_multi_concern() {
        let concerns = parse(
            r#"
            concern A { }
            concern B { }
            concern C { }
            "#,
        )
        .unwrap();
        assert_eq!(concerns.len(), 3);
        assert_eq!(concerns[0].name, "A");
        assert_eq!(concerns[1].name, "B");
        assert_eq!(concerns[2].name, "C");
    }

    #[test]
    fn test_parse_error_location() {
        let result = parse("concern { }");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // Should contain line:col info
        assert!(msg.contains("1:"), "error should mention line 1, got: {msg}");
    }

    #[test]
    fn test_parse_comments_ignored() {
        let concerns = parse(
            r#"
            // This is a comment
            concern X {
                // Another comment
            }
            "#,
        )
        .unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "X");
    }

    #[test]
    fn test_parse_full_tracer_bullet() {
        let source = r#"
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
    "StorageCoordinator is split into separate per-backend coordinators"
  }
}
"#;
        let concerns = parse(source).unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "ResilientStorage");
        // 3 scopes + 1 constraint + 2 applies + decided + rejected + revisit = 9
        assert_eq!(concerns[0].items.len(), 9);
    }

    #[test]
    fn test_parse_multiple_constraint_rules() {
        let concerns = parse(
            r#"concern X {
                constraint multi {
                    [services] must_not depend_on storage_backends
                    [pipeline] must_not reference [AuthMiddleware]
                    AuthMiddleware occur_only_in [routes]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.rules.len(), 3);
                assert!(matches!(&c.rules[0], ConstraintRule::MustNotDependOn { .. }));
                assert!(matches!(&c.rules[1], ConstraintRule::MustNotReference { .. }));
                assert!(matches!(&c.rules[2], ConstraintRule::OccurOnlyIn { .. }));
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_must_implement() {
        let concerns = parse(
            r#"concern X {
                constraint trait_check {
                    DgraphClient must_implement GraphStore
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.rules.len(), 1);
                match &c.rules[0] {
                    ConstraintRule::MustImplement {
                        type_name,
                        trait_name,
                    } => {
                        assert_eq!(type_name, "DgraphClient");
                        assert_eq!(trait_name, "GraphStore");
                    }
                    other => panic!("expected MustImplement, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_scope_ref_in_from() {
        let concerns = parse(
            r#"concern X {
                scope processing { [services, pipeline] }
                constraint no_leak {
                    processing must_not depend_on storage
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[1] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MustNotDependOn { from, target } => {
                        // Bare scope name stored as single-element vec
                        assert_eq!(from, &["processing"]);
                        assert_eq!(target, "storage");
                    }
                    other => panic!("expected MustNotDependOn, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_prefix_glob_in_list() {
        let concerns = parse(
            r#"concern X {
                constraint bounded {
                    [services] must_not reference [*Middleware]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MustNotReference { from, targets } => {
                        assert_eq!(from, &["services"]);
                        assert_eq!(targets, &["*Middleware"]);
                    }
                    other => panic!("expected MustNotReference, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_suffix_glob_in_list() {
        let concerns = parse(
            r#"concern X {
                constraint bounded {
                    [services] must_not reference [Dgraph*]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MustNotReference { targets, .. } => {
                        assert_eq!(targets, &["Dgraph*"]);
                    }
                    other => panic!("expected MustNotReference, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_wildcard_bare_occur_only_in() {
        let concerns = parse(
            r#"concern X {
                constraint loc {
                    *Client occur_only_in [storage]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::OccurOnlyIn { pattern, modules } => {
                        assert_eq!(pattern, "*Client");
                        assert_eq!(modules, &["storage"]);
                    }
                    other => panic!("expected OccurOnlyIn, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_must_depend_on() {
        let concerns = parse(
            r#"concern X {
                constraint requires_storage {
                    [services] must_depend_on storage
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MustDependOn { from, target } => {
                        assert_eq!(from, &["services"]);
                        assert_eq!(target, "storage");
                    }
                    other => panic!("expected MustDependOn, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_must_reference() {
        let concerns = parse(
            r#"concern X {
                constraint must_use_error {
                    [services] must_reference [AppError, Result]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MustReference { from, targets } => {
                        assert_eq!(from, &["services"]);
                        assert_eq!(targets, &["AppError", "Result"]);
                    }
                    other => panic!("expected MustReference, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_layer_declaration() {
        let concerns = parse(
            r#"concern X {
                layer presentation { [routes] }
                layer application { [services] }
            }"#,
        )
        .unwrap();
        assert_eq!(concerns[0].items.len(), 2);
        match &concerns[0].items[0] {
            ConcernItem::Layer(l) => {
                assert_eq!(l.name, "presentation");
                assert_eq!(l.entities, vec!["routes"]);
            }
            other => panic!("expected Layer, got {other:?}"),
        }
        match &concerns[0].items[1] {
            ConcernItem::Layer(l) => {
                assert_eq!(l.name, "application");
                assert_eq!(l.entities, vec!["services"]);
            }
            other => panic!("expected Layer, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_layer_with_multiple_entities() {
        let concerns = parse(
            r#"concern X {
                layer processing { [pipeline, segmentation, rag] }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Layer(l) => {
                assert_eq!(l.name, "processing");
                assert_eq!(l.entities, vec!["pipeline", "segmentation", "rag"]);
            }
            other => panic!("expected Layer, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_layered_architecture_file() {
        let source = r#"
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
    "Core services remain testable without HTTP/auth infrastructure."
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
"#;
        let concerns = parse(source).unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "LayeredArchitecture");
        // 4 layers + 1 constraint + decided + rejected + revisit = 8
        assert_eq!(concerns[0].items.len(), 8);
    }

    #[test]
    fn test_parse_parameter_float() {
        let concerns = parse(
            r#"concern X {
                parameter platform_fee: 0.03
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Parameter(p) => {
                assert_eq!(p.name, "platform_fee");
                assert_eq!(p.value, ParamValue::Float(0.03));
            }
            other => panic!("expected Parameter, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_parameter_int() {
        let concerns = parse(
            r#"concern X {
                parameter threshold: 5
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Parameter(p) => {
                assert_eq!(p.name, "threshold");
                assert_eq!(p.value, ParamValue::Int(5));
            }
            other => panic!("expected Parameter, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_parameter_percent() {
        let concerns = parse(
            r#"concern X {
                parameter rate: 5%
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Parameter(p) => {
                assert_eq!(p.name, "rate");
                assert_eq!(p.value, ParamValue::Float(0.05));
            }
            other => panic!("expected Parameter, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_parameter_duration() {
        let concerns = parse(
            r#"concern X {
                parameter timeout: 7d
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Parameter(p) => {
                assert_eq!(p.name, "timeout");
                assert_eq!(p.value, ParamValue::Duration(7 * 86400));
            }
            other => panic!("expected Parameter, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invariant_simple() {
        let concerns = parse(
            r#"concern X {
                invariant check {
                    a < b
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Invariant(inv) => {
                assert_eq!(inv.name, "check");
                assert_eq!(inv.expressions.len(), 1);
                match &inv.expressions[0] {
                    InvariantExpr::Comparison { lhs, op, rhs } => {
                        assert_eq!(*lhs, ArithExpr::Ident("a".into()));
                        assert_eq!(*op, ComparisonOp::Lt);
                        assert_eq!(*rhs, ArithExpr::Ident("b".into()));
                    }
                }
            }
            other => panic!("expected Invariant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invariant_arithmetic() {
        let concerns = parse(
            r#"concern X {
                invariant sum_check {
                    a + b == 1.0
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Invariant(inv) => {
                assert_eq!(inv.name, "sum_check");
                assert_eq!(inv.expressions.len(), 1);
                match &inv.expressions[0] {
                    InvariantExpr::Comparison { lhs, op, rhs } => {
                        assert!(matches!(lhs, ArithExpr::BinOp { op: ArithOp::Add, .. }));
                        assert_eq!(*op, ComparisonOp::Eq);
                        assert_eq!(*rhs, ArithExpr::Literal(1.0));
                    }
                }
            }
            other => panic!("expected Invariant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invariant_multiple_expressions() {
        let concerns = parse(
            r#"concern X {
                invariant ordering {
                    a < b,
                    b < c,
                    c < d
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Invariant(inv) => {
                assert_eq!(inv.expressions.len(), 3);
            }
            other => panic!("expected Invariant, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_statemachine() {
        let concerns = parse(
            r#"concern X {
                statemachine lifecycle {
                    states [Open, Closed, HalfOpen]
                    initial Open
                    terminal [Closed]
                    transition Open -> Closed
                    transition Closed -> HalfOpen
                    transition HalfOpen -> Open
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::StateMachine(sm) => {
                assert_eq!(sm.name, "lifecycle");
                assert_eq!(sm.states, vec!["Open", "Closed", "HalfOpen"]);
                assert_eq!(sm.initial, "Open");
                assert_eq!(sm.terminal, vec!["Closed"]);
                assert_eq!(sm.transitions.len(), 3);
                assert_eq!(sm.transitions[0], ("Open".into(), "Closed".into()));
            }
            other => panic!("expected StateMachine, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_statemachine_with_invariant() {
        let concerns = parse(
            r#"concern X {
                statemachine sm {
                    states [A, B, C]
                    initial A
                    terminal [C]
                    transition A -> B
                    transition B -> C
                    must_not reach A -> C
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::StateMachine(sm) => {
                assert_eq!(sm.invariants.len(), 1);
                match &sm.invariants[0].kind {
                    SmInvariantKind::MustNotReach { from, to } => {
                        assert_eq!(from, "A");
                        assert_eq!(to, "C");
                    }
                }
            }
            other => panic!("expected StateMachine, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_statemachine_with_refines() {
        let concerns = parse(
            r#"concern X {
                statemachine sm {
                    states [A, B]
                    initial A
                    terminal [B]
                    transition A -> B
                    refines "formal/spec.tla"
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::StateMachine(sm) => {
                assert_eq!(sm.refines.as_deref(), Some("formal/spec.tla"));
            }
            other => panic!("expected StateMachine, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_bridge() {
        let concerns = parse(
            r#"concern X {
                bridge escrow_events {
                    source ContractEngine lang typescript
                    sink EscrowContract lang solidity
                    events ["Deposited", "Released"]
                    bidirectional
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Bridge(b) => {
                assert_eq!(b.name, "escrow_events");
                assert_eq!(b.source.entity, "ContractEngine");
                assert_eq!(b.source.lang.as_deref(), Some("typescript"));
                assert_eq!(b.sink.entity, "EscrowContract");
                assert_eq!(b.sink.lang.as_deref(), Some("solidity"));
                assert_eq!(b.events, vec!["Deposited", "Released"]);
                assert_eq!(b.constraint_type, BridgeConstraintType::Bidirectional);
            }
            other => panic!("expected Bridge, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_bridge_function_signatures() {
        let concerns = parse(
            r#"concern X {
                bridge abi {
                    source Gateway lang typescript
                    sink Contract lang solidity
                    function_signatures_match
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Bridge(b) => {
                assert_eq!(b.constraint_type, BridgeConstraintType::FunctionSignaturesMatch);
            }
            other => panic!("expected Bridge, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_scope_with_lang() {
        let concerns = parse(
            r#"concern X {
                scope on_chain lang solidity {
                    [EscrowContract, TokenContract]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Scope(s) => {
                assert_eq!(s.name, "on_chain");
                assert_eq!(s.lang.as_deref(), Some("solidity"));
            }
            other => panic!("expected Scope, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_when_present() {
        let concerns = parse(
            r#"concern X {
                constraint coherence {
                    when_present milestones requires [budget, quality]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::WhenPresent { field, requires } => {
                        assert_eq!(field, "milestones");
                        assert_eq!(requires, &["budget", "quality"]);
                    }
                    other => panic!("expected WhenPresent, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_mutually_exclusive() {
        let concerns = parse(
            r#"concern X {
                constraint exclusion {
                    mutually_exclusive [modeA, modeB]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::MutuallyExclusive { fields } => {
                        assert_eq!(fields, &["modeA", "modeB"]);
                    }
                    other => panic!("expected MutuallyExclusive, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_covers() {
        let concerns = parse(
            r#"concern X {
                constraint test {
                    [a] must_not depend_on b
                    covers ["scenario_1", "scenario_2"]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.covers, vec!["scenario_1", "scenario_2"]);
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_status() {
        let concerns = parse(
            r#"concern X {
                constraint test {
                    status planned
                    [a] must_not depend_on b
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.status, Some(ConstraintStatus::Planned));
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_constraint_status_deferred() {
        let concerns = parse(
            r#"concern X {
                constraint test {
                    status deferred
                    [a] must_not depend_on b
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.status, Some(ConstraintStatus::Deferred));
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_backward_compat_bracket_from() {
        // Existing syntax must still work
        let concerns = parse(
            r#"concern X {
                constraint test {
                    [services, pipeline] must_not depend_on storage_backends
                    [services] must_not reference [AuthMiddleware, SessionCookie]
                    AuthMiddleware occur_only_in [routes]
                    DgraphClient must_implement GraphStore
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.rules.len(), 4);
                assert!(matches!(&c.rules[0], ConstraintRule::MustNotDependOn { .. }));
                assert!(matches!(&c.rules[1], ConstraintRule::MustNotReference { .. }));
                assert!(matches!(&c.rules[2], ConstraintRule::OccurOnlyIn { .. }));
                assert!(matches!(&c.rules[3], ConstraintRule::MustImplement { .. }));
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }
}
