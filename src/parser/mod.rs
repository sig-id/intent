pub mod ast;

use lalrpop_util::lalrpop_mod;
lalrpop_mod!(
    #[allow(clippy::all)]
    #[allow(unused)]
    pub intent,
    "/intent.rs"
);

use anyhow::Result;
use ast::TopLevel;

/// Helper: strip surrounding quotes from a string literal.
pub fn unquote(s: &str) -> String {
    s[1..s.len() - 1].to_string()
}

/// Helper: parse a duration literal (e.g., "30s", "5m", "2h", "100ms").
pub fn parse_duration(s: &str) -> u64 {
    // Try multi-char suffixes first
    if s.ends_with("ms") {
        let num_part = &s[..s.len() - 2];
        let base: u64 = num_part.parse().unwrap_or(0);
        return base; // milliseconds as base units
    } else if s.ends_with("us") || s.ends_with("μs") {
        let num_part = &s[..s.len() - 2];
        let base: u64 = num_part.parse().unwrap_or(0);
        return base; // microseconds as base units
    } else if s.ends_with("ns") {
        let num_part = &s[..s.len() - 2];
        let base: u64 = num_part.parse().unwrap_or(0);
        return base; // nanoseconds as base units
    }

    // Single-char suffixes
    let num_part = &s[..s.len() - 1];
    let unit = &s[s.len() - 1..];
    let base: u64 = num_part.parse().unwrap_or(0);
    match unit {
        "s" => base * 1000,         // seconds -> ms
        "m" => base * 60 * 1000,    // minutes -> ms
        "h" => base * 3600 * 1000,  // hours -> ms
        "d" => base * 86400 * 1000, // days -> ms
        _ => base,
    }
}

/// Parse an Intent source string into a list of top-level declarations.
pub fn parse(source: &str) -> Result<Vec<TopLevel>> {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ast::*;
    use super::*;

    #[test]
    fn test_parse_empty_system() {
        let top = parse("system Empty { }").unwrap();
        assert_eq!(top.len(), 1);
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.name, "Empty");
                assert!(!s.span.is_synthetic());
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_system_description() {
        let top = parse(
            r#"system X {
                description "Test system"
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.description.as_deref(), Some("Test system"));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_component() {
        let top = parse(
            r#"system X {
                component API {
                    contains [routes, handlers]
                    depends_only [Processing]
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.components.len(), 1);
                let c = &s.components[0];
                assert_eq!(c.name, "API");
                assert_eq!(c.contains, vec!["routes", "handlers"]);
                assert_eq!(c.depends_only, vec!["Processing"]);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_component_with_behavior() {
        let top = parse(
            r#"system X {
                component Processing {
                    implements "crates/processing/src"

                    behavior Lifecycle {
                        states { idle active }
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let c = &s.components[0];
                assert_eq!(c.implements.as_deref(), Some("crates/processing/src"));
                assert_eq!(c.behaviors.len(), 1);
                assert_eq!(c.behaviors[0].name, "Lifecycle");
            }
            _ => panic!("expected System"),
        }
    }

    // Note: scope is now a std lib pattern (applies Scoped { ... })
    // Scope syntax removed from core language

    #[test]
    fn test_parse_constraint_predicate() {
        let top = parse(
            r#"system X {
                constraint isolation {
                    !Processing.depends(storage_backends)
                    Processing.references([AppError])
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.constraints.len(), 1);
                let c = &s.constraints[0];
                assert_eq!(c.name, "isolation");
                assert_eq!(c.rules.len(), 2);

                // First rule: !A.depends(B)
                match &c.rules[0] {
                    ConstraintRule::Not(inner) => match inner.as_ref() {
                        ConstraintRule::Predicate(PredicateCall::Depends { from, to }) => {
                            assert_eq!(
                                *from,
                                ScopeExpr::Ident(QualifiedName::simple("Processing"))
                            );
                            assert_eq!(
                                *to,
                                vec![ScopeExpr::Ident(QualifiedName::simple("storage_backends"))]
                            );
                        }
                        _ => panic!("expected Depends predicate"),
                    },
                    _ => panic!("expected Not"),
                }

                // Second rule: A.references(B)
                match &c.rules[1] {
                    ConstraintRule::Predicate(PredicateCall::References { from, to }) => {
                        assert_eq!(*from, ScopeExpr::Ident(QualifiedName::simple("Processing")));
                        assert_eq!(*to, vec![ScopeExpr::EntityList(vec!["AppError".into()])]);
                    }
                    _ => panic!("expected References predicate"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_constraint_operators() {
        let top = parse(
            r#"system X {
                constraint logic {
                    A.depends(B) && !A.references(C)
                    A.depends(D) => A.references(E)
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let c = &s.constraints[0];
                assert_eq!(c.rules.len(), 2);

                // First: AND
                match &c.rules[0] {
                    ConstraintRule::And(left, right) => {
                        assert!(matches!(left.as_ref(), ConstraintRule::Predicate(_)));
                        assert!(matches!(right.as_ref(), ConstraintRule::Not(_)));
                    }
                    _ => panic!("expected And"),
                }

                // Second: IMPLIES
                match &c.rules[1] {
                    ConstraintRule::Implies(left, right) => {
                        assert!(matches!(left.as_ref(), ConstraintRule::Predicate(_)));
                        assert!(matches!(right.as_ref(), ConstraintRule::Predicate(_)));
                    }
                    _ => panic!("expected Implies"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_iff_operator() {
        let top = parse(
            r#"system X {
                constraint equivalence {
                    A.depends(B) <=> B.depends(A)
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let c = &s.constraints[0];
                assert_eq!(c.rules.len(), 1);

                match &c.rules[0] {
                    ConstraintRule::Iff(left, right) => {
                        assert!(matches!(left.as_ref(), ConstraintRule::Predicate(_)));
                        assert!(matches!(right.as_ref(), ConstraintRule::Predicate(_)));
                    }
                    _ => panic!("expected Iff"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_precedence_chain() {
        // Test precedence: <=> has lowest, then =>, then ||, then &&
        // Using predicates as atoms
        let top = parse(
            r#"system X {
                constraint chain {
                    A.depends(B) => C.depends(D) <=> E.depends(F)
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let c = &s.constraints[0];
                // Should parse as: (A.depends(B) => C.depends(D)) <=> E.depends(F)
                match &c.rules[0] {
                    ConstraintRule::Iff(left, right) => {
                        // Left should be (A => B)
                        assert!(matches!(left.as_ref(), ConstraintRule::Implies(_, _)));
                        // Right should be E.depends(F) (simple predicate)
                        assert!(matches!(right.as_ref(), ConstraintRule::Predicate(_)));
                    }
                    _ => panic!("expected Iff at top level"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_forall() {
        let top = parse(
            r#"system X {
                constraint error_policy {
                    forall s in services: s.references([AppError])
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let c = &s.constraints[0];
                match &c.rules[0] {
                    ConstraintRule::Forall {
                        var, domain, body, ..
                    } => {
                        assert_eq!(var, "s");
                        assert_eq!(*domain, ScopeExpr::Ident(QualifiedName::simple("services")));
                        assert!(matches!(body.as_ref(), ConstraintRule::Predicate(_)));
                    }
                    _ => panic!("expected Forall"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_predicate_def() {
        let top = parse(
            r#"system X {
                predicate isolated(src, target) {
                    !src.depends(target)
                    !src.references(target)
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.predicates.len(), 1);
                let p = &s.predicates[0];
                assert_eq!(p.name, "isolated");
                assert_eq!(p.params, vec!["src", "target"]);
                assert_eq!(p.body.len(), 2);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_behavior() {
        let top = parse(
            r#"system X {
                behavior OrderLifecycle {
                    states {
                        pending { initial: true }
                        settled { terminal: true }
                    }
                    transitions {
                        pending -> settled on confirm
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.behaviors.len(), 1);
                let b = &s.behaviors[0];
                assert_eq!(b.name, "OrderLifecycle");
                assert_eq!(b.states.len(), 2);
                assert!(b.states[0].initial);
                assert!(b.states[1].terminal);
                assert_eq!(b.transitions.len(), 1);
                assert_eq!(b.transitions[0].from.as_state(), Some("pending"));
                assert_eq!(b.transitions[0].to.as_state(), Some("settled"));
                assert_eq!(b.transitions[0].on_event, "confirm");
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_executable_behavior_aliases() {
        let top = parse(
            r#"system X {
                behavior SessionLifecycle executable {
                    model {
                        state anonymous { initial: true }
                        state authenticated { terminal: true }
                    }
                    vars {
                        session_id: Bytes[32]
                        proof: crypto.Proof<Bytes[32]>
                        principal: auth.Subject?
                    }
                    memory {
                        attempts: Int = 0
                    }
                    transition anonymous -> authenticated on login
                        where { attempts < 3 }
                        effect {
                            attempts = attempts + 1
                            emit SessionIssued(session_id)
                        }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let b = &s.behaviors[0];
                assert_eq!(b.name, "SessionLifecycle");
                assert_eq!(b.states.len(), 2);
                assert_eq!(b.states[0].name, "anonymous");
                assert!(b.states[0].initial);
                assert_eq!(b.states[1].name, "authenticated");
                assert!(b.states[1].terminal);

                assert_eq!(b.variables.len(), 4);
                assert_eq!(b.variables[0].type_name, "Bytes[32]");
                assert_eq!(b.variables[1].type_name, "crypto.Proof<Bytes[32]>");
                assert_eq!(b.variables[2].type_name, "auth.Subject?");
                assert_eq!(b.variables[3].name, "attempts");
                assert!(matches!(b.variables[3].initial_value, Some(Expr::Int(0))));

                assert_eq!(b.transitions.len(), 1);
                let transition = &b.transitions[0];
                assert_eq!(transition.from.as_state(), Some("anonymous"));
                assert_eq!(transition.to.as_state(), Some("authenticated"));
                assert_eq!(transition.on_event, "login");
                assert!(matches!(
                    transition.guard,
                    Some(Expr::CompOp {
                        op: ComparisonOp::Lt,
                        ..
                    })
                ));
                assert_eq!(transition.effects.len(), 2);
                assert!(matches!(
                    transition.effects[0].kind,
                    EffectKind::Assign { ref var, .. } if var == "attempts"
                ));
                assert!(matches!(
                    transition.effects[1].kind,
                    EffectKind::Emit { ref name, ref args }
                        if name == "SessionIssued" && args.len() == 1
                ));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_executable_transition_body() {
        let top = parse(
            r#"system X {
                behavior SessionLifecycle executable {
                    model {
                        state anonymous { initial: true }
                        state authenticated { terminal: true }
                    }
                    vars {
                        attempts: Int = 0
                    }
                    transition anonymous -> authenticated on login {
                        input code: String default { "seed" }
                        set attempts = { attempts + 1 }
                        expect { attempts == 1 }
                        emit SessionIssued(code)
                    }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let transition = &s.behaviors[0].transitions[0];
                assert_eq!(transition.inputs.len(), 1);
                assert_eq!(transition.inputs[0].name, "code");
                assert_eq!(transition.inputs[0].type_name, "String");
                assert!(matches!(
                    transition.inputs[0].default_value,
                    Some(Expr::String(ref s)) if s == "seed"
                ));
                assert_eq!(transition.expects.len(), 1);
                assert_eq!(transition.effects.len(), 2);
                assert!(matches!(
                    transition.effects[0].kind,
                    EffectKind::Assign { ref var, .. } if var == "attempts"
                ));
                assert!(matches!(
                    transition.effects[1].kind,
                    EffectKind::Emit { ref name, ref args }
                        if name == "SessionIssued" && args.len() == 1
                ));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_memory_alias_in_executable_expressions() {
        let top = parse(
            r#"system X {
                behavior SessionLifecycle executable {
                    model {
                        state pending { initial: true }
                        state done { terminal: true }
                    }
                    vars {
                        attempts: Int = 0
                    }
                    transition pending -> done on complete {
                        expect { memory.attempts == 1 }
                        set attempts = { memory.attempts + 1 }
                    }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let transition = &s.behaviors[0].transitions[0];
                assert!(matches!(
                    &transition.expects[0],
                    Expr::CompOp { lhs, rhs, .. }
                        if matches!(lhs.as_ref(), Expr::DottedName(name) if name == "memory.attempts")
                            && matches!(rhs.as_ref(), Expr::Int(1))
                ));
                assert!(matches!(
                    &transition.effects[0].kind,
                    EffectKind::Assign { ref var, value: Expr::BinOp { lhs, .. } }
                        if var == "attempts"
                            && matches!(lhs.as_ref(), Expr::DottedName(name) if name == "memory.attempts")
                ));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_executable_contract_metadata() {
        let top = parse(
            r#"system X {
                behavior TokenLifecycle executable {
                    model {
                        state pending { initial: true }
                        state done { terminal: true }
                    }
                    vars {
                        tenant_id: Int
                        code_id: Int
                    }
                    fixture "seed_code" {
                        insert tenant { name: "MBT Tenant" } -> tenant_id
                        call "seed_code" { tenant_id: $tenant_id, scopes: ["openid"] } -> code_id
                    }
                    projection model_state from db.authorization_code where id = $code_id {
                        when meta.reducer_state == "done" => done
                        else => pending
                    }
                    transition pending -> done on exchange {
                        binds call "svc::mark_used" { id: $code_id }
                        binds update db.authorization_code {
                            set used = true
                            where id = $code_id
                        }
                        expect { result.is_some == true }
                    }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let behavior = &s.behaviors[0];
                assert_eq!(behavior.fixtures.len(), 1);
                assert_eq!(behavior.projections.len(), 1);
                assert_eq!(behavior.fixtures[0].name, "seed_code");
                assert_eq!(behavior.fixtures[0].steps.len(), 2);
                assert!(matches!(
                    behavior.fixtures[0].steps[0],
                    FixtureStep::Insert { ref target, ref bind, .. }
                        if target == "tenant" && bind.as_deref() == Some("tenant_id")
                ));
                assert!(matches!(
                    behavior.fixtures[0].steps[1],
                    FixtureStep::Call { ref path, ref bind, .. }
                        if path == "seed_code" && bind.as_deref() == Some("code_id")
                ));

                let projection = &behavior.projections[0];
                assert_eq!(projection.name, "model_state");
                assert!(matches!(
                    projection.source,
                    Some(ProjectionSource { ref source, .. }) if source == "db.authorization_code"
                ));
                assert_eq!(projection.clauses.len(), 1);
                assert_eq!(projection.else_state.as_deref(), Some("pending"));

                let transition = &behavior.transitions[0];
                assert_eq!(transition.bindings.len(), 2);
                assert!(matches!(
                    transition.bindings[0],
                    TransitionBinding::Call { ref path, .. } if path == "svc::mark_used"
                ));
                assert!(matches!(
                    transition.bindings[1],
                    TransitionBinding::Update { ref target, ref assignments, .. }
                        if target == "db.authorization_code" && assignments.len() == 1
                ));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_mbt_block() {
        let top = parse(
            r#"system X {
                behavior SessionLifecycle executable {
                    model {
                        state pending { initial: true }
                        state done { terminal: true }
                    }
                    projection model_state {
                        else => pending
                    }
                    mbt {
                        generator apalache {
                            invariant NotTerminated
                            max_traces 16
                            max_length 4
                            mode check
                            view "state"
                        }
                        replay {
                            allow_unknown_action true
                            state_projection model_state
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let behavior = &s.behaviors[0];
                let mbt = behavior.mbt.as_ref().expect("expected mbt block");
                let generator = mbt.generator.as_ref().expect("expected generator");
                let replay = mbt.replay.as_ref().expect("expected replay");

                assert_eq!(generator.engine, "apalache");
                assert_eq!(generator.invariants, vec!["NotTerminated"]);
                assert_eq!(generator.max_traces, Some(16));
                assert_eq!(generator.max_length, Some(4));
                assert_eq!(generator.mode.as_deref(), Some("check"));
                assert_eq!(generator.view.as_deref(), Some("state"));
                assert_eq!(replay.allow_unknown_action, Some(true));
                assert_eq!(replay.state_projection.as_deref(), Some("model_state"));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_executable_transition_receive() {
        let top = parse(
            r#"system X {
                behavior SessionLifecycle executable {
                    model {
                        state pending { initial: true }
                        state done { terminal: true }
                    }
                    transition pending -> done on complete {
                        receive { Mailbox.CodeProvided where { code_ok } }
                    }
                }
            }"#,
        )
        .unwrap();

        match &top[0] {
            TopLevel::System(s) => {
                let transition = &s.behaviors[0].transitions[0];
                assert_eq!(transition.effects.len(), 1);
                assert!(matches!(
                    transition.effects[0].kind,
                    EffectKind::Receive {
                        ref channel,
                        ref message,
                        filter: Some(Expr::Ident(ref filter))
                    } if channel == "Mailbox" && message == "CodeProvided" && filter == "code_ok"
                ));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_import_pattern() {
        let top = parse(r#"import pattern Saga from "github.com/org/patterns@v1.2""#).unwrap();
        match &top[0] {
            TopLevel::Import(i) => {
                assert_eq!(i.kind, ImportKind::Pattern);
                assert_eq!(i.name, "Saga");
                assert_eq!(i.source, "github.com/org/patterns@v1.2");
            }
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn test_parse_import_template() {
        let top = parse(
            r#"import template Auth from "github.com/org/auth@main"
                with { mfa: true }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::Import(i) => {
                assert_eq!(i.kind, ImportKind::Template);
                assert_eq!(i.name, "Auth");
                assert_eq!(i.source, "github.com/org/auth@main");
                assert_eq!(i.with_params.len(), 1);
            }
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn test_parse_pattern() {
        let top = parse(
            r#"pattern Retry<Op> {
                parameters {
                    max_attempts: Int { default: 3 }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::Pattern(p) => {
                assert_eq!(p.name, "Retry");
                assert_eq!(p.type_params.len(), 1);
                assert_eq!(p.type_params[0].name, "Op");
            }
            _ => panic!("expected Pattern"),
        }
    }

    #[test]
    fn test_parse_system_properties() {
        let top = parse(
            r#"system X {
                platform: "kubernetes"
                timeout: 30
                enabled: true
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert!(s.properties.iter().any(|(k, v)| {
                    k == "platform" && matches!(v, PropertyValue::String(id) if id == "kubernetes")
                }));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_rationale() {
        let top = parse(
            r#"system X {
                rationale CircuitBreakerDecision {
                    decided because {
                        "Circuit breakers prevent cascading failures."
                    }
                    rejected {
                        retry_only: "Retries cause request pileup."
                    }
                    revisit when {
                        "HA configuration is added"
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.rationales.len(), 1);
                let r = &s.rationales[0];
                assert_eq!(r.decided_because.len(), 1);
                assert_eq!(r.rejected.len(), 1);
                assert_eq!(r.revisit_when.len(), 1);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_uses() {
        let top = parse(
            r#"system X {
                uses Auth
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.uses, vec!["Auth"]);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_distilled() {
        let top = parse(
            r#"system X {
                distilled pattern CacheInvalidation {
                    source: "crates/client/src/*.rs"
                    commit: "a1b2c3d"
                    extracted: "2026-02-15"
                    observation { "Pattern emerged." }
                    applies_to { "cache.*" }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                assert_eq!(s.distilled.len(), 1);
                let d = &s.distilled[0];
                assert_eq!(d.name, "CacheInvalidation");
                assert_eq!(d.source, "crates/client/src/*.rs");
                assert_eq!(d.commit, "a1b2c3d");
                assert_eq!(d.extracted, Some("2026-02-15".to_string()));
                assert_eq!(d.applies_to.as_ref().unwrap().path, "cache.*");
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_rationale_top_level() {
        let top = parse(
            r#"rationale LatentCoupling {
                discovered: "2026-02-10"
                source: "Code review"
                observation { "Inconsistent cache invalidation." }
                decided because { "Use cache invalidator pattern." }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::Rationale(r) => {
                assert_eq!(r.name, "LatentCoupling");
                assert_eq!(r.decided_because.len(), 1);
            }
            _ => panic!("expected Rationale"),
        }
    }

    #[test]
    fn test_parse_full_v02_system() {
        let source = r#"
import pattern Saga from "github.com/org/patterns@v1.2"

system PaymentPlatform {
    description "Multi-tenant payment processing"
    components [Ingestion, Processing, Settlement]

    component Processing {
        implements "crates/processing/src"

        behavior TransactionLifecycle {
            states {
                pending { initial: true }
                settled { terminal: true }
                failed { terminal: true }
            }
            transitions {
                pending -> settled on confirm
                pending -> failed on timeout
            }
        }
    }

    component API {
        contains [routes, handlers]
        depends_only [Processing]
    }

    constraint isolation {
        !Processing.depends([DgraphClient, MilvusClient])
        Processing.references([AppError])
    }

    predicate isolated(src, tgt) {
        !src.depends(tgt) && !src.references(tgt)
    }

    platform: "kubernetes"

    rationale ArchitectureDecisions {
        decided because { "Layered architecture with circuit breakers." }
    }
}
"#;
        let top = parse(source).unwrap();
        assert_eq!(top.len(), 2); // import + system

        match &top[1] {
            TopLevel::System(s) => {
                assert_eq!(s.name, "PaymentPlatform");
                assert_eq!(s.components.len(), 2);
                assert_eq!(s.constraints.len(), 1);
                assert_eq!(s.predicates.len(), 1);
                assert_eq!(s.rationales.len(), 1);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_error_location() {
        let result = parse("system { }");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("1:"),
            "error should mention line 1, got: {msg}"
        );
    }

    #[test]
    fn test_parse_pattern_multiple_parameters() {
        let top = parse(
            r#"
            pattern RetryPolicy {
                parameters {
                    maxRetries: Int
                    backoff: Duration
                    timeout: Duration
                }
            }
            "#,
        )
        .unwrap();
        assert_eq!(top.len(), 1);
        match &top[0] {
            TopLevel::Pattern(p) => {
                assert_eq!(p.name, "RetryPolicy");
                assert_eq!(p.parameters.len(), 3);
                assert_eq!(p.parameters[0].name, "maxRetries");
                assert_eq!(p.parameters[1].name, "backoff");
                assert_eq!(p.parameters[2].name, "timeout");
            }
            _ => panic!("expected Pattern"),
        }
    }

    #[test]
    fn test_parse_comments_ignored() {
        let top = parse(
            r#"
            // This is a comment
            system X {
                // Another comment
            }
            "#,
        )
        .unwrap();
        assert_eq!(top.len(), 1);
        match &top[0] {
            TopLevel::System(s) => assert_eq!(s.name, "X"),
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_transition_with_guard() {
        let top = parse(
            r#"system X {
                behavior PaymentFlow {
                    states {
                        validating
                        processing
                    }
                    transitions {
                        validating -> processing on valid where { amount <= limit }
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let t = &s.behaviors[0].transitions[0];
                assert_eq!(t.from.as_state(), Some("validating"));
                assert_eq!(t.to.as_state(), Some("processing"));
                assert_eq!(t.on_event, "valid");
                assert!(t.guard.is_some());
                match t.guard.as_ref().unwrap() {
                    Expr::CompOp { lhs, op, rhs } => {
                        assert!(matches!(lhs.as_ref(), Expr::Ident(s) if s == "amount"));
                        assert_eq!(*op, ComparisonOp::Le);
                        assert!(matches!(rhs.as_ref(), Expr::Ident(s) if s == "limit"));
                    }
                    _ => panic!("expected CompOp for guard"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_transition_with_effect() {
        let top = parse(
            r#"system X {
                behavior OrderProcessor {
                    states {
                        idle
                        reserving
                    }
                    transitions {
                        idle -> reserving on OrderCreated
                            effect { emit ReserveInventory(order_id, items) }
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let t = &s.behaviors[0].transitions[0];
                assert_eq!(t.from.as_state(), Some("idle"));
                assert_eq!(t.to.as_state(), Some("reserving"));
                assert_eq!(t.on_event, "OrderCreated");
                assert_eq!(t.effects.len(), 1);
                match &t.effects[0].kind {
                    EffectKind::Emit { name, args } => {
                        assert_eq!(name, "ReserveInventory");
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("expected Emit effect"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_transition_with_timing_after() {
        let top = parse(
            r#"system X {
                behavior Flow {
                    states { a b }
                    transitions {
                        a -> b on trigger after { 5m }
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let t = &s.behaviors[0].transitions[0];
                assert!(t.timing.is_some());
                match t.timing.as_ref().unwrap() {
                    TransitionTiming::After(e) => {
                        // 5 minutes = 5 * 60 * 1000 = 300000 ms
                        assert!(matches!(e, Expr::Duration(300000)));
                    }
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_transition_full() {
        let top = parse(
            r#"system X {
                behavior PaymentFlow {
                    states {
                        validating
                        processing
                    }
                    transitions {
                        validating -> processing on valid
                            where { amount <= limit }
                            effect { emit ProcessPayment(order_id) }
                            after { 30s }
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let t = &s.behaviors[0].transitions[0];
                assert_eq!(t.from.as_state(), Some("validating"));
                assert_eq!(t.to.as_state(), Some("processing"));
                assert_eq!(t.on_event, "valid");
                assert!(t.guard.is_some());
                assert_eq!(t.effects.len(), 1);
                match &t.effects[0].kind {
                    EffectKind::Emit { name, args } => {
                        assert_eq!(name, "ProcessPayment");
                        assert_eq!(args.len(), 1);
                    }
                    _ => panic!("expected Emit"),
                }
                assert!(matches!(t.timing, Some(TransitionTiming::After(_))));
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_fairness_with_alt() {
        let top = parse(
            r#"system X {
                behavior Flow {
                    states { validating processing failed }
                    fairness {
                        weak(validating -> processing | failed)
                        strong(processing -> validating | failed)
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let b = &s.behaviors[0];
                assert_eq!(b.fairness.len(), 2);

                let f0 = &b.fairness[0];
                assert_eq!(f0.kind, FairnessKind::Weak);
                assert_eq!(f0.from, "validating");
                assert_eq!(f0.to, "processing");
                assert_eq!(f0.alts, vec!["failed".to_string()]);

                let f1 = &b.fairness[1];
                assert_eq!(f1.kind, FairnessKind::Strong);
                assert_eq!(f1.from, "processing");
                assert_eq!(f1.to, "validating");
                assert_eq!(f1.alts, vec!["failed".to_string()]);
            }
            _ => panic!("expected System"),
        }

        // Also test without alt (existing behavior)
        let top2 = parse(
            r#"system X {
                behavior Flow {
                    states { a b }
                    fairness {
                        weak(a -> b)
                    }
                }
            }"#,
        )
        .unwrap();
        match &top2[0] {
            TopLevel::System(s) => {
                let f = &s.behaviors[0].fairness[0];
                assert!(f.alts.is_empty());
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_value_types() {
        let top = parse(
            r#"
            pattern TestPattern {
                parameters {
                    timeout: Duration { default: 30s }
                    rate: Float { default: 0.5 }
                    items: List { default: [1, 2, 3] }
                    nested: List { default: ["a", true, 42] }
                }
            }
            "#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::Pattern(p) => {
                assert_eq!(p.parameters.len(), 4);

                // Duration: 30s = 30 * 1000 = 30000 ms
                assert_eq!(p.parameters[0].name, "timeout");
                assert!(p.parameters[0]
                    .constraints
                    .contains(&FieldConstraint::Default(ParamValue::Duration(30000))));

                // Float
                assert_eq!(p.parameters[1].name, "rate");
                assert!(p.parameters[1]
                    .constraints
                    .contains(&FieldConstraint::Default(ParamValue::Float(0.5))));

                // List of ints
                assert_eq!(p.parameters[2].name, "items");
                match &p.parameters[2].constraints[0] {
                    FieldConstraint::Default(ParamValue::List(items)) => {
                        assert_eq!(items.len(), 3);
                        assert_eq!(items[0], ParamValue::Int(1));
                        assert_eq!(items[1], ParamValue::Int(2));
                        assert_eq!(items[2], ParamValue::Int(3));
                    }
                    _ => panic!("expected List default"),
                }

                // Mixed list
                assert_eq!(p.parameters[3].name, "nested");
                match &p.parameters[3].constraints[0] {
                    FieldConstraint::Default(ParamValue::List(items)) => {
                        assert_eq!(items.len(), 3);
                        assert_eq!(items[0], ParamValue::String("a".to_string()));
                        assert_eq!(items[1], ParamValue::Bool(true));
                        assert_eq!(items[2], ParamValue::Int(42));
                    }
                    _ => panic!("expected List default"),
                }
            }
            _ => panic!("expected Pattern"),
        }
    }

    #[test]
    fn test_parse_behavior_nodes() {
        let top = parse(
            r#"system X {
                behavior LeaderElection {
                    nodes: replicas
                    states {
                        follower { initial: true }
                        candidate
                        leader
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let b = &s.behaviors[0];
                assert_eq!(b.name, "LeaderElection");
                assert_eq!(b.nodes, Some("replicas".to_string()));
                assert_eq!(b.states.len(), 3);
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_count_in_property() {
        let top = parse(
            r#"system X {
                behavior LeaderElection {
                    nodes: replicas
                    states {
                        follower { initial: true }
                        leader
                    }
                    property single_leader {
                        always(count(leader) <= 1)
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let b = &s.behaviors[0];
                assert_eq!(b.properties.len(), 1);
                let prop = &b.properties[0];
                assert_eq!(prop.name, "single_leader");
                match &prop.expr {
                    TemporalExpr::Always(inner) => match inner.as_ref() {
                        TemporalExpr::BinOp { lhs, op, rhs } => {
                            assert!(
                                matches!(lhs.as_ref(), TemporalExpr::Count(s) if s == "leader")
                            );
                            assert_eq!(*op, TemporalOp::Le);
                            assert!(matches!(rhs.as_ref(), TemporalExpr::Int(1)));
                        }
                        _ => panic!("expected BinOp"),
                    },
                    _ => panic!("expected Always"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_count_comparison_operators() {
        // Test all comparison operators
        let test_cases = vec![
            ("count(x) <= 1", TemporalOp::Le),
            ("count(x) >= 1", TemporalOp::Ge),
            ("count(x) < 1", TemporalOp::Lt),
            ("count(x) > 1", TemporalOp::Gt),
            ("count(x) == 1", TemporalOp::Eq),
            ("count(x) != 0", TemporalOp::Ne),
        ];

        for (expr_str, expected_op) in test_cases {
            let top = parse(&format!(
                r#"
                system X {{
                    behavior Test {{
                        states {{ a b }}
                        property p {{ {} }}
                    }}
                }}
            "#,
                expr_str
            ))
            .unwrap();

            match &top[0] {
                TopLevel::System(s) => {
                    let prop = &s.behaviors[0].properties[0];
                    match &prop.expr {
                        TemporalExpr::BinOp { op, .. } => {
                            assert_eq!(*op, expected_op, "failed for expr: {}", expr_str);
                        }
                        _ => panic!("expected BinOp for: {}", expr_str),
                    }
                }
                _ => panic!("expected System"),
            }
        }
    }

    #[test]
    fn test_parse_count_vs_count_comparison() {
        let top = parse(
            r#"system X {
                behavior Test {
                    states { a b }
                    property majority {
                        always(count(leader) > count(follower))
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let prop = &s.behaviors[0].properties[0];
                match &prop.expr {
                    TemporalExpr::Always(inner) => match inner.as_ref() {
                        TemporalExpr::BinOp { lhs, op, rhs } => {
                            assert!(
                                matches!(lhs.as_ref(), TemporalExpr::Count(s) if s == "leader")
                            );
                            assert_eq!(*op, TemporalOp::Gt);
                            assert!(
                                matches!(rhs.as_ref(), TemporalExpr::Count(s) if s == "follower")
                            );
                        }
                        _ => panic!("expected BinOp"),
                    },
                    _ => panic!("expected Always"),
                }
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn test_parse_always_eventually_count() {
        let top = parse(
            r#"system X {
                behavior Test {
                    nodes: replicas
                    states { a b }
                    property no_leaderless {
                        always(eventually(count(leader) >= 1))
                    }
                }
            }"#,
        )
        .unwrap();
        match &top[0] {
            TopLevel::System(s) => {
                let prop = &s.behaviors[0].properties[0];
                assert_eq!(prop.name, "no_leaderless");
                match &prop.expr {
                    TemporalExpr::Always(inner) => match inner.as_ref() {
                        TemporalExpr::Eventually(inner2) => match inner2.as_ref() {
                            TemporalExpr::BinOp { lhs, op, rhs } => {
                                assert!(
                                    matches!(lhs.as_ref(), TemporalExpr::Count(s) if s == "leader")
                                );
                                assert_eq!(*op, TemporalOp::Ge);
                                assert!(matches!(rhs.as_ref(), TemporalExpr::Int(1)));
                            }
                            _ => panic!("expected BinOp"),
                        },
                        _ => panic!("expected Eventually"),
                    },
                    _ => panic!("expected Always"),
                }
            }
            _ => panic!("expected System"),
        }
    }
}
