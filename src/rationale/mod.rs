use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::behavioral::ObligationResult;
use crate::parser::ast::{Concern, ConcernItem};
use crate::structural::ConstraintResult;

/// Top-level rationale output.
#[derive(Debug, Serialize)]
pub struct RationaleReport {
    pub concerns: Vec<ConcernRationale>,
}

/// Rationale for a single concern.
#[derive(Debug, Serialize)]
pub struct ConcernRationale {
    pub name: String,
    pub decided_because: Vec<String>,
    pub rejected_alternatives: Vec<RejectedAlternative>,
    pub revisit_when: Vec<String>,
    pub structural_constraints: Vec<ConstraintSummary>,
    pub behavioral_obligations: Vec<ObligationSummary>,
    pub scenario_coverage: Vec<ScenarioCoverage>,
}

#[derive(Debug, Serialize)]
pub struct ScenarioCoverage {
    pub scenario: String,
    pub covered_by: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RejectedAlternative {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ConstraintSummary {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ObligationSummary {
    pub pattern: String,
    pub target: String,
    pub refines: String,
    pub status: String,
}

/// Build a rationale report from parsed concerns and verification results.
///
/// Results are filtered per-concern using the `concern` field on each result.
pub fn build_report(
    concerns: &[Concern],
    structural_results: &[ConstraintResult],
    obligation_results: &[ObligationResult],
) -> RationaleReport {
    let mut report_concerns = Vec::new();

    for concern in concerns {
        let mut decided_because = Vec::new();
        let mut rejected_alternatives = Vec::new();
        let mut revisit_when = Vec::new();

        for item in &concern.items {
            match item {
                ConcernItem::DecidedBecause(reasons) => {
                    decided_because.extend(reasons.clone());
                }
                ConcernItem::RejectedAlternatives(alts) => {
                    for (name, reason) in alts {
                        rejected_alternatives.push(RejectedAlternative {
                            name: name.clone(),
                            reason: reason.clone(),
                        });
                    }
                }
                ConcernItem::RevisitWhen(conditions) => {
                    revisit_when.extend(conditions.clone());
                }
                _ => {}
            }
        }

        // Filter structural results to only those belonging to this concern
        let structural_constraints: Vec<ConstraintSummary> = structural_results
            .iter()
            .filter(|r| r.concern == concern.name)
            .map(|r| ConstraintSummary {
                name: r.name.clone(),
                status: if r.passed {
                    "pass".into()
                } else {
                    "fail".into()
                },
            })
            .collect();

        // Filter behavioral results to only those belonging to this concern
        let behavioral_obligations: Vec<ObligationSummary> = obligation_results
            .iter()
            .filter(|r| r.concern == concern.name)
            .map(|r| ObligationSummary {
                pattern: r.pattern.clone(),
                target: r.target.clone(),
                refines: r.refines.clone(),
                status: r.status.to_string(),
            })
            .collect();

        // Build scenario coverage map
        let mut coverage_map: HashMap<String, Vec<String>> = HashMap::new();
        for item in &concern.items {
            if let ConcernItem::Constraint(c) = item {
                for scenario in &c.covers {
                    coverage_map
                        .entry(scenario.clone())
                        .or_default()
                        .push(c.name.clone());
                }
            }
        }
        let scenario_coverage: Vec<ScenarioCoverage> = coverage_map
            .into_iter()
            .map(|(scenario, covered_by)| ScenarioCoverage {
                scenario,
                covered_by,
            })
            .collect();

        report_concerns.push(ConcernRationale {
            name: concern.name.clone(),
            decided_because,
            rejected_alternatives,
            revisit_when,
            structural_constraints,
            behavioral_obligations,
            scenario_coverage,
        });
    }

    RationaleReport {
        concerns: report_concerns,
    }
}

/// Write the rationale report to a JSON file.
pub fn write_json(report: &RationaleReport, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dirs for {}", output.display()))?;
    }
    let json = serde_json::to_string_pretty(report)
        .context("serializing rationale report")?;
    std::fs::write(output, json)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::behavioral::ObligationStatus;
    use crate::parser::ast::Concern;

    #[test]
    fn test_per_concern_filtering() {
        let concerns = vec![
            Concern {
                name: "A".into(),
                items: vec![ConcernItem::DecidedBecause(vec!["reason A".into()])],
                span: None,
            },
            Concern {
                name: "B".into(),
                items: vec![ConcernItem::DecidedBecause(vec!["reason B".into()])],
                span: None,
            },
        ];

        let structural = vec![
            ConstraintResult {
                name: "rule_a".into(),
                concern: "A".into(),
                passed: true,
                violations: vec![],
            },
            ConstraintResult {
                name: "rule_b".into(),
                concern: "B".into(),
                passed: false,
                violations: vec![],
            },
        ];

        let obligations = vec![ObligationResult {
            pattern: "CB".into(),
            target: "target".into(),
            refines: "spec.tla".into(),
            concern: "A".into(),
            status: ObligationStatus::Pass,
            detail: String::new(),
        }];

        let report = build_report(&concerns, &structural, &obligations);

        // Concern A should have rule_a and the obligation
        assert_eq!(report.concerns[0].structural_constraints.len(), 1);
        assert_eq!(report.concerns[0].structural_constraints[0].name, "rule_a");
        assert_eq!(report.concerns[0].behavioral_obligations.len(), 1);

        // Concern B should have only rule_b and no obligations
        assert_eq!(report.concerns[1].structural_constraints.len(), 1);
        assert_eq!(report.concerns[1].structural_constraints[0].name, "rule_b");
        assert_eq!(report.concerns[1].behavioral_obligations.len(), 0);
    }

    #[test]
    fn test_json_roundtrip() {
        let report = RationaleReport {
            concerns: vec![ConcernRationale {
                name: "Test".into(),
                decided_because: vec!["reason".into()],
                rejected_alternatives: vec![RejectedAlternative {
                    name: "alt".into(),
                    reason: "bad".into(),
                }],
                revisit_when: vec!["condition".into()],
                structural_constraints: vec![ConstraintSummary {
                    name: "rule".into(),
                    status: "pass".into(),
                }],
                behavioral_obligations: vec![],
                scenario_coverage: vec![],
            }],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["concerns"][0]["name"].as_str().unwrap(),
            "Test"
        );
    }
}
