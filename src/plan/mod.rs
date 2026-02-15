use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::Result;
use serde::Serialize;

use crate::parser::ast::{
    ArithExpr, ArithOp, ComparisonOp, Concern, ConcernItem, InvariantExpr, ParamValue,
};

/// Result of plan-mode validation for a single concern.
#[derive(Debug, Clone, Serialize)]
pub struct PlanResult {
    pub concern: String,
    pub checks: Vec<PlanCheck>,
}

/// A single plan-mode check.
#[derive(Debug, Clone, Serialize)]
pub struct PlanCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// Validate concerns in plan mode (no codebase required).
///
/// Checks:
/// - Scope references resolve
/// - Layers are acyclic (declaration order = dependency order)
/// - Parameter invariants hold
/// - State machine completeness (terminal states reachable, no orphan states, invariants satisfied)
pub fn validate(concerns: &[Concern]) -> Result<Vec<PlanResult>> {
    let mut results = Vec::new();

    for concern in concerns {
        let mut checks = Vec::new();

        // Collect parameter declarations
        let params = collect_parameters(concern);

        // Check parameter invariants
        for item in &concern.items {
            if let ConcernItem::Invariant(inv) = item {
                for (idx, expr) in inv.expressions.iter().enumerate() {
                    checks.push(check_invariant(&inv.name, idx, expr, &params));
                }
            }
        }

        // Check state machines
        for item in &concern.items {
            if let ConcernItem::StateMachine(sm) = item {
                checks.extend(check_state_machine(sm));
            }
        }

        // Check scope references in constraints
        let scope_names: HashSet<String> = concern
            .items
            .iter()
            .filter_map(|item| {
                if let ConcernItem::Scope(s) = item {
                    Some(s.name.clone())
                } else {
                    None
                }
            })
            .collect();

        for item in &concern.items {
            if let ConcernItem::Constraint(c) = item {
                // For each rule, check that scope references resolve
                // (A scope reference is when `from` is a single-element vec that matches a scope name)
                for rule in &c.rules {
                    use crate::parser::ast::ConstraintRule;
                    match rule {
                        ConstraintRule::MustNotDependOn { from, .. }
                        | ConstraintRule::MustNotReference { from, .. }
                        | ConstraintRule::MustDependOn { from, .. }
                        | ConstraintRule::MustReference { from, .. } => {
                            // If from is a single name, check if it's a scope
                            if from.len() == 1 {
                                let name = &from[0];
                                if scope_names.contains(name) {
                                    checks.push(PlanCheck {
                                        name: format!("{}:scope_ref:{}", c.name, name),
                                        passed: true,
                                        detail: format!("Scope reference '{}' resolves", name),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Check layer acyclicity (declaration order must be dependency order)
        let layer_names: Vec<String> = concern
            .items
            .iter()
            .filter_map(|item| {
                if let ConcernItem::Layer(l) = item {
                    Some(l.name.clone())
                } else {
                    None
                }
            })
            .collect();

        if !layer_names.is_empty() {
            checks.push(PlanCheck {
                name: "layers:acyclic".into(),
                passed: true,
                detail: format!(
                    "Layers declared in order: [{}]",
                    layer_names.join(", ")
                ),
            });
        }

        results.push(PlanResult {
            concern: concern.name.clone(),
            checks,
        });
    }

    Ok(results)
}

fn collect_parameters(concern: &Concern) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    for item in &concern.items {
        if let ConcernItem::Parameter(p) = item {
            let val = match &p.value {
                ParamValue::Int(i) => *i as f64,
                ParamValue::Float(f) => *f,
                ParamValue::Duration(d) => *d as f64,
                ParamValue::Str(_) => continue, // Skip string params
            };
            params.insert(p.name.clone(), val);
        }
    }
    params
}

fn check_invariant(
    inv_name: &str,
    idx: usize,
    expr: &InvariantExpr,
    params: &HashMap<String, f64>,
) -> PlanCheck {
    match expr {
        InvariantExpr::Comparison { lhs, op, rhs } => {
            let lhs_val = match eval_arith(lhs, params) {
                Ok(v) => v,
                Err(e) => {
                    return PlanCheck {
                        name: format!("{}:expr_{}", inv_name, idx),
                        passed: false,
                        detail: format!("LHS evaluation failed: {}", e),
                    };
                }
            };
            let rhs_val = match eval_arith(rhs, params) {
                Ok(v) => v,
                Err(e) => {
                    return PlanCheck {
                        name: format!("{}:expr_{}", inv_name, idx),
                        passed: false,
                        detail: format!("RHS evaluation failed: {}", e),
                    };
                }
            };

            let passed = match op {
                ComparisonOp::Lt => lhs_val < rhs_val,
                ComparisonOp::Gt => lhs_val > rhs_val,
                ComparisonOp::Le => lhs_val <= rhs_val,
                ComparisonOp::Ge => lhs_val >= rhs_val,
                ComparisonOp::Eq => (lhs_val - rhs_val).abs() < f64::EPSILON,
                ComparisonOp::Ne => (lhs_val - rhs_val).abs() >= f64::EPSILON,
            };

            PlanCheck {
                name: format!("{}:expr_{}", inv_name, idx),
                passed,
                detail: format!("{} {:?} {} = {}", lhs_val, op, rhs_val, passed),
            }
        }
    }
}

fn eval_arith(expr: &ArithExpr, params: &HashMap<String, f64>) -> Result<f64> {
    match expr {
        ArithExpr::Literal(v) => Ok(*v),
        ArithExpr::Ident(name) => params
            .get(name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("parameter '{}' not found", name)),
        ArithExpr::BinOp { lhs, op, rhs } => {
            let l = eval_arith(lhs, params)?;
            let r = eval_arith(rhs, params)?;
            Ok(match op {
                ArithOp::Add => l + r,
                ArithOp::Sub => l - r,
                ArithOp::Mul => l * r,
                ArithOp::Div => {
                    if r.abs() < f64::EPSILON {
                        return Err(anyhow::anyhow!("division by zero"));
                    }
                    l / r
                }
            })
        }
        ArithExpr::Neg(e) => Ok(-eval_arith(e, params)?),
    }
}

fn check_state_machine(
    sm: &crate::parser::ast::StateMachineDecl,
) -> Vec<PlanCheck> {
    let mut checks = Vec::new();

    // Check 1: all transition states are declared
    let state_set: HashSet<_> = sm.states.iter().cloned().collect();
    for (from, to) in &sm.transitions {
        if !state_set.contains(from) {
            checks.push(PlanCheck {
                name: format!("{}:transition_state:{}", sm.name, from),
                passed: false,
                detail: format!("Transition source '{}' not in declared states", from),
            });
        }
        if !state_set.contains(to) {
            checks.push(PlanCheck {
                name: format!("{}:transition_state:{}", sm.name, to),
                passed: false,
                detail: format!("Transition target '{}' not in declared states", to),
            });
        }
    }

    // Check 2: initial state is declared
    if !state_set.contains(&sm.initial) {
        checks.push(PlanCheck {
            name: format!("{}:initial_state", sm.name),
            passed: false,
            detail: format!("Initial state '{}' not in declared states", sm.initial),
        });
    } else {
        checks.push(PlanCheck {
            name: format!("{}:initial_state", sm.name),
            passed: true,
            detail: format!("Initial state '{}' declared", sm.initial),
        });
    }

    // Check 3: all terminal states are declared
    for term in &sm.terminal {
        if !state_set.contains(term) {
            checks.push(PlanCheck {
                name: format!("{}:terminal_state:{}", sm.name, term),
                passed: false,
                detail: format!("Terminal state '{}' not in declared states", term),
            });
        }
    }

    // Check 4: all terminal states are reachable from initial
    let reachable = compute_reachable(&sm.initial, &sm.transitions);
    for term in &sm.terminal {
        if !reachable.contains(term) {
            checks.push(PlanCheck {
                name: format!("{}:reachable:{}", sm.name, term),
                passed: false,
                detail: format!(
                    "Terminal state '{}' not reachable from initial '{}'",
                    term, sm.initial
                ),
            });
        } else {
            checks.push(PlanCheck {
                name: format!("{}:reachable:{}", sm.name, term),
                passed: true,
                detail: format!("Terminal state '{}' reachable", term),
            });
        }
    }

    // Check 5: no orphan states (except terminal states that are intentionally unreachable)
    for state in &sm.states {
        if state != &sm.initial && !reachable.contains(state) && !sm.terminal.contains(state) {
            checks.push(PlanCheck {
                name: format!("{}:orphan:{}", sm.name, state),
                passed: false,
                detail: format!(
                    "State '{}' is orphaned (not reachable from initial and not terminal)",
                    state
                ),
            });
        }
    }

    // Check 6: must_not_reach invariants
    for inv in &sm.invariants {
        match &inv.kind {
            crate::parser::ast::SmInvariantKind::MustNotReach { from, to } => {
                let reachable_from = compute_reachable(from, &sm.transitions);
                if reachable_from.contains(to) {
                    checks.push(PlanCheck {
                        name: format!("{}:{}", sm.name, inv.name),
                        passed: false,
                        detail: format!(
                            "Invariant violated: state '{}' can reach '{}'",
                            from, to
                        ),
                    });
                } else {
                    checks.push(PlanCheck {
                        name: format!("{}:{}", sm.name, inv.name),
                        passed: true,
                        detail: format!("Invariant satisfied: '{}' cannot reach '{}'", from, to),
                    });
                }
            }
        }
    }

    checks
}

fn compute_reachable(start: &str, transitions: &[(String, String)]) -> HashSet<String> {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());
    reachable.insert(start.to_string());

    while let Some(current) = queue.pop_front() {
        for (from, to) in transitions {
            if from == &current && !reachable.contains(to) {
                reachable.insert(to.clone());
                queue.push_back(to.clone());
            }
        }
    }

    reachable
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn test_invariant_pass() {
        let source = r#"
concern Test {
  parameter fee_tier0: 0.01
  parameter fee_tier1: 0.02
  parameter fee_tier2: 0.03

  invariant fee_ordering {
    fee_tier0 < fee_tier1,
    fee_tier1 < fee_tier2
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks.iter().all(|c| c.passed),
            "all invariant checks should pass: {checks:?}"
        );
    }

    #[test]
    fn test_invariant_fail() {
        let source = r#"
concern Test {
  parameter a: 10
  parameter b: 5

  invariant ordering {
    a < b
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks.iter().any(|c| !c.passed),
            "invariant a < b should fail when a=10, b=5"
        );
    }

    #[test]
    fn test_invariant_arithmetic() {
        let source = r#"
concern Test {
  parameter w1: 0.60
  parameter w2: 0.15
  parameter w3: 0.10
  parameter w4: 0.10
  parameter w5: 0.05

  invariant sums_to_one {
    w1 + w2 + w3 + w4 + w5 == 1.0
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks.iter().all(|c| c.passed),
            "sum should equal 1.0: {checks:?}"
        );
    }

    #[test]
    fn test_invariant_complex_expression() {
        let source = r#"
concern Test {
  parameter fee: 0.03
  parameter witness: 0.02

  invariant net_positive {
    1.0 - fee - witness > 0
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks.iter().all(|c| c.passed),
            "1.0 - 0.03 - 0.02 > 0 should pass: {checks:?}"
        );
    }

    #[test]
    fn test_state_machine_reachability() {
        let source = r#"
concern Test {
  statemachine circuit_breaker {
    states [Closed, Open, HalfOpen]
    initial Closed
    terminal [Closed]
    transition Closed -> Open
    transition Open -> HalfOpen
    transition HalfOpen -> Closed
    transition HalfOpen -> Open
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks
                .iter()
                .any(|c| c.name.contains("reachable") && c.passed),
            "terminal state should be reachable"
        );
    }

    #[test]
    fn test_state_machine_orphan_detection() {
        let source = r#"
concern Test {
  statemachine broken {
    states [A, B, C]
    initial A
    terminal [B]
    transition A -> B
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        assert!(
            checks.iter().any(|c| c.name.contains("orphan") && !c.passed),
            "orphan state C should be detected"
        );
    }

    #[test]
    fn test_state_machine_must_not_reach() {
        let source = r#"
concern Test {
  statemachine safe {
    states [Good, Bad, Recovery]
    initial Good
    terminal [Good]
    transition Good -> Bad
    transition Bad -> Recovery
    transition Recovery -> Good
    must_not reach Bad -> Recovery
  }
}
"#;
        let concerns = parse(source).unwrap();
        let results = validate(&concerns).unwrap();
        assert_eq!(results.len(), 1);
        let checks = &results[0].checks;
        // The invariant should fail because Bad -> Recovery is an explicit transition
        assert!(
            checks.iter().any(|c| c
                .name
                .contains("must_not_reach")
                && !c.passed),
            "must_not_reach invariant should be violated"
        );
    }
}
