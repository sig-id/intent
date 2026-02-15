use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::parser::ast::{Concern, ConcernItem, ParamValue, PatternApplication};

/// Result of behavioral obligation verification.
#[derive(Debug, Clone, Serialize)]
pub struct ObligationResult {
    pub pattern: String,
    pub target: String,
    pub refines: String,
    pub concern: String,
    pub status: ObligationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ObligationStatus {
    Pass,
    Fail,
    Skipped,
}

impl std::fmt::Display for ObligationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObligationStatus::Pass => write!(f, "pass"),
            ObligationStatus::Fail => write!(f, "fail"),
            ObligationStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Compile TLA+ obligation modules from `apply ... refines` blocks in concerns.
///
/// Returns the list of generated obligation file paths.
pub fn compile(
    concerns: &[Concern],
    output_dir: &Path,
    project_root: &Path,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("creating output dir {}", output_dir.display()))?;

    let mut generated = Vec::new();

    for concern in concerns {
        let applications: Vec<&PatternApplication> = concern
            .items
            .iter()
            .filter_map(|item| {
                if let ConcernItem::Apply(app) = item {
                    app.refines.as_ref()?;
                    Some(app)
                } else {
                    None
                }
            })
            .collect();

        if applications.is_empty() {
            continue;
        }

        let filename = format!("Obligation_{}.tla", concern.name);
        let path = output_dir.join(&filename);

        let content = generate_obligation_module(&concern.name, &applications, project_root)?;
        std::fs::write(&path, &content)
            .with_context(|| format!("writing {}", path.display()))?;

        generated.push(path);
    }

    Ok(generated)
}

/// Run Apalache to verify generated obligation modules.
pub fn verify(
    obligation_dir: &Path,
    project_root: &Path,
) -> Result<Vec<ObligationResult>> {
    let mut results = Vec::new();

    let obligations: Vec<PathBuf> = std::fs::read_dir(obligation_dir)
        .with_context(|| format!("reading {}", obligation_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "tla")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("Obligation_"))
        })
        .collect();

    if obligations.is_empty() {
        tracing::info!("no obligation files found in {}", obligation_dir.display());
        return Ok(results);
    }

    // Check for Apalache availability
    let apalache_cmd = detect_apalache();

    for obligation_path in &obligations {
        let obligation_name = obligation_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // Extract concern name from filename: "Obligation_ConcernName" -> "ConcernName"
        let concern_name = obligation_name
            .strip_prefix("Obligation_")
            .unwrap_or(&obligation_name)
            .to_string();

        match &apalache_cmd {
            Some(cmd) => {
                let result = run_apalache(
                    cmd,
                    obligation_path,
                    &obligation_name,
                    &concern_name,
                    project_root,
                )?;
                results.push(result);
            }
            None => {
                tracing::warn!("Apalache not found — skipping verification of {obligation_name}");
                results.push(ObligationResult {
                    pattern: "CircuitBreaker".into(),
                    target: obligation_name.clone(),
                    refines: String::new(),
                    concern: concern_name,
                    status: ObligationStatus::Skipped,
                    detail: "Apalache not installed".into(),
                });
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// TLA+ generation
// ---------------------------------------------------------------------------

fn generate_obligation_module(
    concern_name: &str,
    applications: &[&PatternApplication],
    project_root: &Path,
) -> Result<String> {
    // Extract unique spec paths and validate they exist
    let mut spec_params: HashMap<String, &PatternApplication> = HashMap::new();
    for app in applications {
        if let Some(ref refines) = app.refines {
            let spec_path = project_root.join(refines);
            if !spec_path.exists() {
                bail!(
                    "TLA+ spec not found: {} (resolved to {})",
                    refines,
                    spec_path.display()
                );
            }
            spec_params.insert(refines.clone(), app);
        }
    }

    // For the PoC, we assume all applications refine the same CircuitBreaker spec.
    // Extract parameters from the first application.
    let first = applications[0];
    let params = extract_cb_params(first)?;

    let mut tla = String::new();
    tla.push_str(&format!("---- MODULE Obligation_{concern_name} ----\n"));
    // Convert CamelCase to snake_case for the source file reference
    let snake_name = concern_name
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if c.is_uppercase() && i > 0 {
                acc.push('_');
            }
            acc.push(c.to_ascii_lowercase());
            acc
        });
    tla.push_str(&format!(
        "(* Auto-generated from formal/intent/{snake_name}.intent *)\n"
    ));
    tla.push_str("(* DO NOT EDIT — regenerated on every intent run. *)\n\n");
    tla.push_str("EXTENDS Integers, Sequences\n\n");

    // Declare variables (same as CircuitBreakerMBT.tla)
    tla.push_str("VARIABLES\n");
    tla.push_str("    \\* @type: Str;\n    cb_state,\n");
    tla.push_str("    \\* @type: Int;\n    failure_count,\n");
    tla.push_str("    \\* @type: Int;\n    half_open_successes,\n");
    tla.push_str("    \\* @type: Int;\n    time_in_open,\n");
    tla.push_str("    \\* @type: Int;\n    total_requests,\n");
    tla.push_str("    \\* @type: Int;\n    rejected_requests,\n");
    tla.push_str("    \\* @type: Int;\n    clock,\n");
    tla.push_str("    \\* @type: Str;\n    action_taken,\n");
    tla.push_str("    \\* @type: Seq(Int);\n    nondet_picks\n\n");

    // INSTANCE with parameter substitution
    tla.push_str("INSTANCE CircuitBreaker WITH\n");
    tla.push_str(&format!("    FAILURE_THRESHOLD <- {},\n", params.threshold));
    tla.push_str(&format!(
        "    HALF_OPEN_SUCCESS_THRESHOLD <- {},\n",
        params.probe_limit
    ));
    // Use 0 for MBT (eliminates wall-clock dependency, same as CircuitBreakerMBT.tla)
    tla.push_str("    RECOVERY_TIMEOUT <- 0\n\n");

    tla.push_str("ConstInit == TRUE\n\n");

    // Generate pattern obligation invariants
    tla.push_str("\\* Pattern obligations (must hold for the spec with given params)\n");
    tla.push_str(&format!(
        "PatternInv_OpenRequiresThreshold ==\n    cb_state = \"Open\" => failure_count >= {}\n\n",
        params.threshold
    ));
    tla.push_str(
        "PatternInv_OpenRejects ==\n    cb_state = \"Open\" => half_open_successes = 0\n\n",
    );
    tla.push_str(&format!(
        "PatternInv_ClosedBelowThreshold ==\n    cb_state = \"Closed\" => failure_count < {}\n\n",
        params.threshold
    ));

    tla.push_str("PatternObligation ==\n");
    tla.push_str("    /\\ PatternInv_OpenRequiresThreshold\n");
    tla.push_str("    /\\ PatternInv_OpenRejects\n");
    tla.push_str("    /\\ PatternInv_ClosedBelowThreshold\n\n");

    tla.push_str("====\n");

    Ok(tla)
}

struct CbParams {
    threshold: i64,
    probe_limit: i64,
}

fn extract_cb_params(app: &PatternApplication) -> Result<CbParams> {
    let mut threshold = 5i64;
    let mut probe_limit = 2i64;

    for (key, val) in &app.params {
        match key.as_str() {
            "threshold" => {
                if let ParamValue::Int(n) = val {
                    threshold = *n;
                }
            }
            "probe_limit" => {
                if let ParamValue::Int(n) = val {
                    probe_limit = *n;
                }
            }
            _ => {}
        }
    }

    Ok(CbParams {
        threshold,
        probe_limit,
    })
}

// ---------------------------------------------------------------------------
// Apalache execution
// ---------------------------------------------------------------------------

fn detect_apalache() -> Option<String> {
    if Command::new("apalache-mc")
        .arg("version")
        .output()
        .is_ok()
    {
        return Some("apalache-mc".into());
    }
    None
}

fn run_apalache(
    cmd: &str,
    obligation_path: &Path,
    obligation_name: &str,
    concern_name: &str,
    project_root: &Path,
) -> Result<ObligationResult> {
    // Apalache resolves INSTANCE relative to the file's directory.
    // Copy CircuitBreaker.tla next to the obligation so INSTANCE resolves.
    let tla_dir = project_root.join("formal/tla");
    let ob_dir = obligation_path
        .parent()
        .context("obligation has no parent dir")?;

    let cb_src = tla_dir.join("CircuitBreaker.tla");
    let cb_dst = ob_dir.join("CircuitBreaker.tla");
    if cb_src.exists() && !cb_dst.exists() {
        std::fs::copy(&cb_src, &cb_dst).with_context(|| {
            format!(
                "copying {} -> {}",
                cb_src.display(),
                cb_dst.display()
            )
        })?;
    }

    let output = Command::new(cmd)
        .arg("check")
        .arg("--cinit=ConstInit")
        .arg("--inv=PatternObligation")
        .arg("--length=20")
        .arg(obligation_path)
        .output()
        .with_context(|| format!("running Apalache on {obligation_name}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let passed = output.status.success();

    Ok(ObligationResult {
        pattern: "CircuitBreaker".into(),
        target: obligation_name.into(),
        refines: "formal/tla/CircuitBreaker.tla".into(),
        concern: concern_name.into(),
        status: if passed {
            ObligationStatus::Pass
        } else {
            ObligationStatus::Fail
        },
        detail: if passed {
            "Apalache: invariant holds".into()
        } else {
            format!("Apalache failed:\nstdout: {stdout}\nstderr: {stderr}")
        },
    })
}
