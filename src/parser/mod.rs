pub mod ast;

use lalrpop_util::lalrpop_mod;
lalrpop_mod!(
    #[allow(clippy::all)]
    #[allow(unused)]
    pub intent,
    "/parser/intent.rs"
);

use anyhow::Result;
use ast::{Concern, TopLevel};

/// Helper: convert a Vec of &str to Vec<String>.
pub fn strs(v: Vec<&str>) -> Vec<String> {
    v.into_iter().map(|s| s.to_string()).collect()
}

/// Helper: strip surrounding quotes from a string literal.
pub fn unquote(s: &str) -> String {
    s[1..s.len() - 1].to_string()
}

/// Parse an Intent source string into a list of top-level declarations.
pub fn parse(source: &str) -> Result<Vec<TopLevel>> {
    let parser = intent::FileParser::new();
    parser.parse(source).map_err(|e| {
        let msg = format_parse_error(source, e);
        anyhow::anyhow!("{msg}")
    })
}

/// Parse an Intent source string, returning only concerns (for backward compatibility).
pub fn parse_concerns(source: &str) -> Result<Vec<Concern>> {
    let top_levels = parse(source)?;
    let concerns: Vec<Concern> = top_levels
        .into_iter()
        .filter_map(|tl| match tl {
            TopLevel::Concern(c) => Some(c),
            _ => None,
        })
        .collect();
    Ok(concerns)
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

#[derive(Debug)]
pub enum SystemItemParsed {
    Description(String),
    Subsystems(Vec<String>),
    Scope(ast::ScopeDecl),
    Constraint(ast::ConstraintDecl),
    Refines(String),
    RefinementMap(ast::RefinementMap),
}

#[derive(Debug)]
pub enum RefinementMapParsed {
    Mapping(ast::RefinementMapping),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ast::*;
    use super::*;

    #[test]
    fn test_parse_empty_concern() {
        let concerns = parse_concerns("concern Empty { }").unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "Empty");
        assert!(concerns[0].items.is_empty());
        assert!(concerns[0].span.is_some());
    }

    #[test]
    fn test_parse_scope_entity_list() {
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(source).unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "ResilientStorage");
        // 3 scopes + 1 constraint + 2 applies + decided + rejected + revisit = 9
        assert_eq!(concerns[0].items.len(), 9);
    }

    #[test]
    fn test_parse_multiple_constraint_rules() {
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(source).unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "LayeredArchitecture");
        // 4 layers + 1 constraint + decided + rejected + revisit = 8
        assert_eq!(concerns[0].items.len(), 8);
    }

    #[test]
    fn test_parse_parameter_float() {
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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
        let concerns = parse_concerns(
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

    // ===== v0.2: Let bindings =====

    #[test]
    fn test_parse_let_simple() {
        let concerns = parse_concerns(
            r#"concern X {
                let backends = [DgraphClient, MilvusClient]
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { name, expr } => {
                assert_eq!(name, "backends");
                assert_eq!(
                    *expr,
                    ScopeExpr::EntityList(vec!["DgraphClient".into(), "MilvusClient".into()])
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_ident() {
        let concerns = parse_concerns(
            r#"concern X {
                let core = services
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { name, expr } => {
                assert_eq!(name, "core");
                assert_eq!(*expr, ScopeExpr::Ident("services".into()));
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // ===== v0.2: Set algebra =====

    #[test]
    fn test_parse_let_union() {
        let concerns = parse_concerns(
            r#"concern X {
                let external = backends | cache
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                assert!(matches!(expr, ScopeExpr::Union(_, _)));
                if let ScopeExpr::Union(l, r) = expr {
                    assert_eq!(**l, ScopeExpr::Ident("backends".into()));
                    assert_eq!(**r, ScopeExpr::Ident("cache".into()));
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_intersection() {
        let concerns = parse_concerns(
            r#"concern X {
                let shared = services & pipeline
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                assert!(matches!(expr, ScopeExpr::Intersection(_, _)));
                if let ScopeExpr::Intersection(l, r) = expr {
                    assert_eq!(**l, ScopeExpr::Ident("services".into()));
                    assert_eq!(**r, ScopeExpr::Ident("pipeline".into()));
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_difference() {
        let concerns = parse_concerns(
            r"concern X {
                let core = services \ test_helpers
            }",
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                assert!(matches!(expr, ScopeExpr::Difference(_, _)));
                if let ScopeExpr::Difference(l, r) = expr {
                    assert_eq!(**l, ScopeExpr::Ident("services".into()));
                    assert_eq!(**r, ScopeExpr::Ident("test_helpers".into()));
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_comprehension() {
        let concerns = parse_concerns(
            r#"concern X {
                let clients = { e | e matches *Client }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { name, expr } => {
                assert_eq!(name, "clients");
                match expr {
                    ScopeExpr::Comprehension { var, pattern } => {
                        assert_eq!(var, "e");
                        assert_eq!(pattern, "*Client");
                    }
                    other => panic!("expected Comprehension, got {other:?}"),
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_precedence_and_binds_tighter() {
        // & should bind tighter than |
        // a | b & c  should parse as  a | (b & c)
        let concerns = parse_concerns(
            r#"concern X {
                let x = a | b & c
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                // Should be Union(a, Intersection(b, c))
                if let ScopeExpr::Union(l, r) = expr {
                    assert_eq!(**l, ScopeExpr::Ident("a".into()));
                    assert!(matches!(**r, ScopeExpr::Intersection(_, _)));
                } else {
                    panic!("expected Union at top level, got {expr:?}");
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_parens_override_precedence() {
        // (a | b) & c  should parse as  Intersection(Union(a, b), c)
        let concerns = parse_concerns(
            r#"concern X {
                let x = (a | b) & c
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                if let ScopeExpr::Intersection(l, r) = expr {
                    assert!(matches!(**l, ScopeExpr::Union(_, _)));
                    assert_eq!(**r, ScopeExpr::Ident("c".into()));
                } else {
                    panic!("expected Intersection at top level, got {expr:?}");
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_glob() {
        let concerns = parse_concerns(
            r#"concern X {
                let clients = *Client
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                assert_eq!(*expr, ScopeExpr::Glob("*Client".into()));
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_let_complex_expr() {
        // Union of entity list and a difference
        let concerns = parse_concerns(
            r"concern X {
                let x = [A, B] | services \ test
            }",
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Let { expr, .. } => {
                // | and \ have the same precedence, left-associative
                // ([A, B] | services) \ test
                if let ScopeExpr::Difference(l, r) = expr {
                    assert!(matches!(**l, ScopeExpr::Union(_, _)));
                    assert_eq!(**r, ScopeExpr::Ident("test".into()));
                } else {
                    panic!("expected Difference at top, got {expr:?}");
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // ===== v0.2: Quantifiers =====

    #[test]
    fn test_parse_forall_single_body() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint error_handling {
                    forall s in services: s must_reference [AppError]
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                assert_eq!(c.rules.len(), 1);
                match &c.rules[0] {
                    ConstraintRule::Forall { var, domain, body } => {
                        assert_eq!(var, "s");
                        assert_eq!(*domain, ScopeExpr::Ident("services".into()));
                        assert_eq!(body.len(), 1);
                        assert!(matches!(&body[0], ConstraintRule::MustReference { .. }));
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_forall_multi_body() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint strict {
                    forall s in services {
                        s must_reference [AppError]
                        s must_depend_on logging
                    }
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { var, body, .. } => {
                        assert_eq!(var, "s");
                        assert_eq!(body.len(), 2);
                        assert!(matches!(&body[0], ConstraintRule::MustReference { .. }));
                        assert!(matches!(&body[1], ConstraintRule::MustDependOn { .. }));
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_exists() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint observability {
                    exists s in services: s must_depend_on logging
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Exists { var, domain, body } => {
                        assert_eq!(var, "s");
                        assert_eq!(*domain, ScopeExpr::Ident("services".into()));
                        assert_eq!(body.len(), 1);
                        assert!(matches!(&body[0], ConstraintRule::MustDependOn { .. }));
                    }
                    other => panic!("expected Exists, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_forall_with_set_expr_domain() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint bounded {
                    forall m in [services, pipeline]: m must_not depend_on storage_backends
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { domain, .. } => {
                        assert!(matches!(domain, ScopeExpr::EntityList(_)));
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_exists_multi_body() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint some_logging {
                    exists s in services {
                        s must_depend_on logging
                        s must_reference [Metrics]
                    }
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Exists { body, .. } => {
                        assert_eq!(body.len(), 2);
                    }
                    other => panic!("expected Exists, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    // ===== v0.2: Implication =====

    #[test]
    fn test_parse_implies_depends_on() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint caching {
                    forall m in services:
                        m depends_on cache => m must_depend_on cache_invalidation
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { body, .. } => {
                        assert_eq!(body.len(), 1);
                        match &body[0] {
                            ConstraintRule::Implies { condition, consequence } => {
                                match condition {
                                    Condition::DependsOn { entity, target } => {
                                        assert_eq!(entity, "m");
                                        assert_eq!(target, "cache");
                                    }
                                    other => panic!("expected DependsOn, got {other:?}"),
                                }
                                assert!(matches!(**consequence, ConstraintRule::MustDependOn { .. }));
                            }
                            other => panic!("expected Implies, got {other:?}"),
                        }
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_implies_references() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint auth_propagation {
                    forall m in services:
                        m references AuthToken => m must_depend_on auth
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { body, .. } => {
                        match &body[0] {
                            ConstraintRule::Implies { condition, .. } => {
                                match condition {
                                    Condition::References { entity, target } => {
                                        assert_eq!(entity, "m");
                                        assert_eq!(target, "AuthToken");
                                    }
                                    other => panic!("expected References, got {other:?}"),
                                }
                            }
                            other => panic!("expected Implies, got {other:?}"),
                        }
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    // ===== v0.2: Predicate definitions and calls =====

    #[test]
    fn test_parse_predicate_definition() {
        let concerns = parse_concerns(
            r#"concern X {
                predicate isolated(src, target) {
                    src must_not depend_on target
                    src must_not reference target
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Predicate(p) => {
                assert_eq!(p.name, "isolated");
                assert_eq!(p.params, vec!["src", "target"]);
                assert_eq!(p.body.len(), 2);
                assert!(matches!(&p.body[0], ConstraintRule::MustNotDependOn { .. }));
                assert!(matches!(&p.body[1], ConstraintRule::MustNotReference { .. }));
            }
            other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_predicate_call() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint boundaries {
                    isolated(services, storage_backends)
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Call { name, args } => {
                        assert_eq!(name, "isolated");
                        assert_eq!(args.len(), 2);
                        assert_eq!(args[0], ScopeExpr::Ident("services".into()));
                        assert_eq!(args[1], ScopeExpr::Ident("storage_backends".into()));
                    }
                    other => panic!("expected Call, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_predicate_call_with_set_expr_args() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint boundaries {
                    isolated(services | pipeline, storage_backends)
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Call { name, args } => {
                        assert_eq!(name, "isolated");
                        assert!(matches!(&args[0], ScopeExpr::Union(_, _)));
                        assert_eq!(args[1], ScopeExpr::Ident("storage_backends".into()));
                    }
                    other => panic!("expected Call, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_predicate_call_with_list_arg() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint boundaries {
                    isolated([services, pipeline], [storage])
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Call { args, .. } => {
                        assert_eq!(
                            args[0],
                            ScopeExpr::EntityList(vec!["services".into(), "pipeline".into()])
                        );
                        assert_eq!(
                            args[1],
                            ScopeExpr::EntityList(vec!["storage".into()])
                        );
                    }
                    other => panic!("expected Call, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    // ===== v0.2: Composition of features =====

    #[test]
    fn test_parse_full_v02_concern() {
        let source = r#"
concern AdvancedArchitecture {
    // Let bindings with set algebra
    let backends = [DgraphClient, MilvusClient]
    let cache = [RedisClient]
    let external = backends | cache
    let core = [services, pipeline, rag] \ [test_helpers]
    let clients = { e | e matches *Client }

    // Predicate definition
    predicate isolated(src, target) {
        src must_not depend_on target
        src must_not reference target
    }

    // Constraint with predicate calls
    constraint boundaries {
        isolated(core, external)
        isolated([pipeline], [auth])
    }

    // Quantified constraints
    constraint error_handling {
        forall s in core: s must_reference [AppError]
        exists s in core: s must_depend_on logging
    }

    // Quantified with implication
    constraint caching_discipline {
        forall m in core:
            m depends_on cache => m must_depend_on cache_invalidation
    }

    // Forall with multi-rule body
    constraint strict_services {
        forall s in [services, pipeline] {
            s must_not depend_on external
            s must_reference [Result]
        }
    }

    decided because {
        "Set algebra enables compositional scope definitions."
        "Quantifiers make constraint semantics explicit."
        "Predicates enable reusable constraint patterns."
    }
}
"#;
        let concerns = parse_concerns(source).unwrap();
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].name, "AdvancedArchitecture");

        // Count items: 5 lets + 1 predicate + 4 constraints + 1 decided = 11
        assert_eq!(concerns[0].items.len(), 11);

        // Verify let bindings
        assert!(matches!(&concerns[0].items[0], ConcernItem::Let { .. }));
        assert!(matches!(&concerns[0].items[1], ConcernItem::Let { .. }));
        assert!(matches!(&concerns[0].items[2], ConcernItem::Let { .. }));
        assert!(matches!(&concerns[0].items[3], ConcernItem::Let { .. }));
        assert!(matches!(&concerns[0].items[4], ConcernItem::Let { .. }));

        // Verify predicate
        assert!(matches!(&concerns[0].items[5], ConcernItem::Predicate(_)));

        // Verify constraints
        assert!(matches!(&concerns[0].items[6], ConcernItem::Constraint(_)));
        assert!(matches!(&concerns[0].items[7], ConcernItem::Constraint(_)));
        assert!(matches!(&concerns[0].items[8], ConcernItem::Constraint(_)));
        assert!(matches!(&concerns[0].items[9], ConcernItem::Constraint(_)));

        // Verify decided
        assert!(matches!(&concerns[0].items[10], ConcernItem::DecidedBecause(_)));
    }

    #[test]
    fn test_parse_nested_quantifiers() {
        // forall inside forall (via multi-body)
        let concerns = parse_concerns(
            r#"concern X {
                constraint cross_check {
                    forall s in services {
                        forall b in backends: s must_not depend_on b
                    }
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { var, body, .. } => {
                        assert_eq!(var, "s");
                        assert_eq!(body.len(), 1);
                        match &body[0] {
                            ConstraintRule::Forall { var: inner_var, .. } => {
                                assert_eq!(inner_var, "b");
                            }
                            other => panic!("expected nested Forall, got {other:?}"),
                        }
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_forall_with_comprehension_domain() {
        let concerns = parse_concerns(
            r#"concern X {
                constraint client_discipline {
                    forall c in { e | e matches *Client }: c must_implement Closeable
                }
            }"#,
        )
        .unwrap();
        match &concerns[0].items[0] {
            ConcernItem::Constraint(c) => {
                match &c.rules[0] {
                    ConstraintRule::Forall { var, domain, body } => {
                        assert_eq!(var, "c");
                        match domain {
                            ScopeExpr::Comprehension { var: cv, pattern } => {
                                assert_eq!(cv, "e");
                                assert_eq!(pattern, "*Client");
                            }
                            other => panic!("expected Comprehension domain, got {other:?}"),
                        }
                        assert_eq!(body.len(), 1);
                    }
                    other => panic!("expected Forall, got {other:?}"),
                }
            }
            other => panic!("expected Constraint, got {other:?}"),
        }
    }
}
