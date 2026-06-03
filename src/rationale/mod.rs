//! Rationale report generation.
//!
//! This module is being updated for v0.4. Currently a stub.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::behavioral::ObligationResult;
use crate::parser::ast::SystemDecl;
use crate::structural::ConstraintResult;

/// Top-level rationale output.
#[derive(Debug, Serialize)]
pub struct RationaleReport {
    pub systems: Vec<SystemRationale>,
}

/// Rationale for a single system.
#[derive(Debug, Serialize)]
pub struct SystemRationale {
    pub name: String,
    pub rationales: Vec<RationaleEntry>,
    pub structural_constraints: Vec<ConstraintSummary>,
    pub behavioral_obligations: Vec<ObligationSummary>,
}

#[derive(Debug, Serialize)]
pub struct RationaleEntry {
    pub name: String,
    pub decided_because: Vec<String>,
    pub rejected_alternatives: Vec<RejectedAlternative>,
    pub revisit_when: Vec<String>,
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

/// Build a rationale report from parsed systems and verification results.
///
/// This is a stub for v0.4.
pub fn build_report(
    systems: &[SystemDecl],
    structural_results: &[ConstraintResult],
    obligation_results: &[ObligationResult],
) -> RationaleReport {
    let mut report_systems = Vec::new();

    for system in systems {
        let structural_constraints: Vec<ConstraintSummary> = structural_results
            .iter()
            .filter(|r| r.concern == system.name)
            .map(|r| ConstraintSummary {
                name: r.name.clone(),
                status: if r.holds {
                    "pass".into()
                } else {
                    "fail".into()
                },
            })
            .collect();

        let behavioral_obligations: Vec<ObligationSummary> = obligation_results
            .iter()
            .filter(|r| r.concern == system.name)
            .map(|r| ObligationSummary {
                pattern: r.pattern.clone(),
                target: r.target.clone(),
                refines: r.refines.clone(),
                status: r.status.to_string(),
            })
            .collect();

        let rationales: Vec<RationaleEntry> = system
            .rationales
            .iter()
            .map(|r| RationaleEntry {
                name: r.name.clone(),
                decided_because: r.decided_because.clone(),
                rejected_alternatives: r
                    .rejected
                    .iter()
                    .map(|(name, reason)| RejectedAlternative {
                        name: name.clone(),
                        reason: reason.clone(),
                    })
                    .collect(),
                revisit_when: r.revisit_when.clone(),
            })
            .collect();

        report_systems.push(SystemRationale {
            name: system.name.clone(),
            rationales,
            structural_constraints,
            behavioral_obligations,
        });
    }

    RationaleReport {
        systems: report_systems,
    }
}

/// Write the rationale report to a JSON file.
pub fn write_json(report: &RationaleReport, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dirs for {}", output.display()))?;
    }
    let json = serde_json::to_string_pretty(report).context("serializing rationale report")?;
    std::fs::write(output, json).with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}
