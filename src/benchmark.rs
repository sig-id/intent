//! Benchmark extraction from non-functional constraints.
//!
//! Walks every `SystemDecl` and collects `ConstraintRule::NFConstraint` nodes
//! into a serialisable `BenchmarkConfig` that can be fed to benchmark harnesses.

use serde::Serialize;

use crate::parser::ast::{ComparisonOp, ConstraintRule, Expr, NFMetric, SystemDecl};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkConfig {
    pub system: String,
    pub benchmarks: Vec<BenchmarkEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkEntry {
    pub constraint_name: String,
    pub metric: BenchmarkMetric,
    pub target: String,
    pub operator: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkMetric {
    Latency { percentile: String },
    Throughput,
    Memory,
    Cpu,
}

// ---------------------------------------------------------------------------
// Extraction entry-point
// ---------------------------------------------------------------------------

/// Extract benchmark configurations from all systems.
pub fn extract(systems: &[SystemDecl]) -> Vec<BenchmarkConfig> {
    systems
        .iter()
        .map(|system| {
            let mut benchmarks = Vec::new();
            for constraint in &system.constraints {
                for rule in &constraint.rules {
                    extract_from_rule(rule, &constraint.name, &mut benchmarks);
                }
            }
            BenchmarkConfig {
                system: system.name.clone(),
                benchmarks,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recursive rule walker
// ---------------------------------------------------------------------------

fn extract_from_rule(
    rule: &ConstraintRule,
    constraint_name: &str,
    benchmarks: &mut Vec<BenchmarkEntry>,
) {
    match rule {
        ConstraintRule::NFConstraint { metric, op, value } => {
            let (benchmark_metric, target, default_unit) = classify_metric(metric);
            let (num, unit) = extract_value(value, &default_unit);
            benchmarks.push(BenchmarkEntry {
                constraint_name: constraint_name.to_string(),
                metric: benchmark_metric,
                target,
                operator: format_op(op),
                value: num,
                unit,
            });
        }
        ConstraintRule::And(a, b)
        | ConstraintRule::Or(a, b)
        | ConstraintRule::Implies(a, b)
        | ConstraintRule::Iff(a, b) => {
            extract_from_rule(a, constraint_name, benchmarks);
            extract_from_rule(b, constraint_name, benchmarks);
        }
        ConstraintRule::Not(inner) => {
            extract_from_rule(inner, constraint_name, benchmarks);
        }
        ConstraintRule::Forall { body, .. } | ConstraintRule::Exists { body, .. } => {
            extract_from_rule(body, constraint_name, benchmarks);
        }
        ConstraintRule::Suppressed { rule, .. } => {
            extract_from_rule(rule, constraint_name, benchmarks);
        }
        // Predicate / Comparison / Call -- not NF constraints.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map an `NFMetric` to a `BenchmarkMetric`, its target string, and a
/// sensible default unit.
fn classify_metric(metric: &NFMetric) -> (BenchmarkMetric, String, String) {
    match metric {
        NFMetric::P50(target) => (
            BenchmarkMetric::Latency {
                percentile: "p50".to_string(),
            },
            target.clone(),
            "ms".to_string(),
        ),
        NFMetric::P95(target) => (
            BenchmarkMetric::Latency {
                percentile: "p95".to_string(),
            },
            target.clone(),
            "ms".to_string(),
        ),
        NFMetric::P99(target) => (
            BenchmarkMetric::Latency {
                percentile: "p99".to_string(),
            },
            target.clone(),
            "ms".to_string(),
        ),
        NFMetric::Throughput(scope) => (
            BenchmarkMetric::Throughput,
            scope.clone(),
            "ops/s".to_string(),
        ),
        NFMetric::Memory => (BenchmarkMetric::Memory, String::new(), "MB".to_string()),
        NFMetric::Cpu => (BenchmarkMetric::Cpu, String::new(), "%".to_string()),
    }
}

/// Extract a numeric value (and unit) from the right-hand side `Expr`.
///
/// Duration literals are stored in milliseconds by the parser, so we report
/// the value in ms and set the unit accordingly.  Plain integers and floats
/// keep whatever default unit the caller supplies.
fn extract_value(expr: &Expr, default_unit: &str) -> (f64, String) {
    match expr {
        Expr::Duration(ms) => (*ms as f64, "ms".to_string()),
        Expr::Int(n) => (*n as f64, default_unit.to_string()),
        Expr::Float(f) => (*f, default_unit.to_string()),
        _ => (0.0, default_unit.to_string()),
    }
}

/// Format a `ComparisonOp` as the conventional operator string.
fn format_op(op: &ComparisonOp) -> String {
    match op {
        ComparisonOp::Lt => "<".to_string(),
        ComparisonOp::Le => "<=".to_string(),
        ComparisonOp::Gt => ">".to_string(),
        ComparisonOp::Ge => ">=".to_string(),
        ComparisonOp::Eq => "==".to_string(),
        ComparisonOp::Ne => "!=".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::*;

    fn make_system(name: &str, constraints: Vec<ConstraintDecl>) -> SystemDecl {
        SystemDecl {
            name: name.to_string(),
            visibility: Visibility::default(),
            description: None,
            refines: None,
            components_decl: vec![],
            components: vec![],
            constraints,
            behaviors: vec![],
            patterns: vec![],
            applies: vec![],
            predicates: vec![],
            invariants: vec![],
            let_bindings: vec![],
            rationales: vec![],
            properties: vec![],
            distilled: vec![],
            uses: vec![],
            events: vec![],
            messages: vec![],
            functions: vec![],
            protocols: vec![],
            constraint_templates: vec![],
            constraint_applications: vec![],
            span: Span { start: 0, end: 0 },
        }
    }

    fn make_constraint(name: &str, rules: Vec<ConstraintRule>) -> ConstraintDecl {
        ConstraintDecl {
            name: name.to_string(),
            rules,
            span: Span { start: 0, end: 0 },
        }
    }

    #[test]
    fn extracts_p99_latency() {
        let rule = ConstraintRule::NFConstraint {
            metric: NFMetric::P99("getUser".to_string()),
            op: ComparisonOp::Lt,
            value: Expr::Duration(100),
        };
        let constraint = make_constraint("perf", vec![rule]);
        let system = make_system("MyService", vec![constraint]);
        let configs = extract(&[system]);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].system, "MyService");
        assert_eq!(configs[0].benchmarks.len(), 1);

        let b = &configs[0].benchmarks[0];
        assert_eq!(b.constraint_name, "perf");
        assert_eq!(b.target, "getUser");
        assert_eq!(b.operator, "<");
        assert_eq!(b.value, 100.0);
        assert_eq!(b.unit, "ms");
    }

    #[test]
    fn extracts_throughput() {
        let rule = ConstraintRule::NFConstraint {
            metric: NFMetric::Throughput("api".to_string()),
            op: ComparisonOp::Ge,
            value: Expr::Int(1000),
        };
        let constraint = make_constraint("throughput_sla", vec![rule]);
        let system = make_system("Api", vec![constraint]);
        let configs = extract(&[system]);

        assert_eq!(configs[0].benchmarks.len(), 1);
        let b = &configs[0].benchmarks[0];
        assert_eq!(b.operator, ">=");
        assert_eq!(b.value, 1000.0);
        assert_eq!(b.unit, "ops/s");
    }

    #[test]
    fn extracts_from_nested_rules() {
        let inner_a = ConstraintRule::NFConstraint {
            metric: NFMetric::Memory,
            op: ComparisonOp::Le,
            value: Expr::Int(512),
        };
        let inner_b = ConstraintRule::NFConstraint {
            metric: NFMetric::Cpu,
            op: ComparisonOp::Lt,
            value: Expr::Float(80.0),
        };
        let rule = ConstraintRule::And(Box::new(inner_a), Box::new(inner_b));
        let constraint = make_constraint("resources", vec![rule]);
        let system = make_system("Worker", vec![constraint]);
        let configs = extract(&[system]);

        assert_eq!(configs[0].benchmarks.len(), 2);
        assert_eq!(configs[0].benchmarks[0].unit, "MB");
        assert_eq!(configs[0].benchmarks[1].unit, "%");
    }

    #[test]
    fn empty_system_produces_empty_benchmarks() {
        let system = make_system("Empty", vec![]);
        let configs = extract(&[system]);
        assert_eq!(configs.len(), 1);
        assert!(configs[0].benchmarks.is_empty());
    }
}
