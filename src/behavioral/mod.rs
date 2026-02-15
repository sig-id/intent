pub mod patterns;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::parser::ast::{Concern, ConcernItem, PatternApplication};

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

/// Metadata about a generated obligation file (stored alongside the .tla).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct ObligationMeta {
    pattern: String,
    target: String,
    refines: String,
    concern: String,
    instance_module: Option<String>,
    invariant_name: String,
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

        for app in &applications {
            let obligation = patterns::generate(
                &app.pattern,
                &concern.name,
                app,
                project_root,
            )?;

            match obligation {
                Some(ob) => {
                    let filename = format!(
                        "Obligation_{}_{}.tla",
                        concern.name,
                        sanitize_target(&app.target)
                    );
                    let path = output_dir.join(&filename);

                    std::fs::write(&path, &ob.tla_content)
                        .with_context(|| format!("writing {}", path.display()))?;

                    // Write metadata file for the verifier
                    let meta = ObligationMeta {
                        pattern: app.pattern.clone(),
                        target: app.target.clone(),
                        refines: app.refines.clone().unwrap_or_default(),
                        concern: concern.name.clone(),
                        instance_module: ob.instance_module,
                        invariant_name: ob.invariant_name,
                    };
                    let meta_path = path.with_extension("json");
                    let meta_json = serde_json::to_string_pretty(&meta)
                        .context("serializing obligation metadata")?;
                    std::fs::write(&meta_path, &meta_json)
                        .with_context(|| format!("writing {}", meta_path.display()))?;

                    generated.push(path);
                }
                None => {
                    tracing::warn!(
                        "unknown pattern '{}' in concern '{}' — skipping TLA+ generation",
                        app.pattern,
                        concern.name
                    );
                }
            }
        }
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

    let apalache_cmd = detect_apalache();

    for obligation_path in &obligations {
        // Load metadata if available
        let meta_path = obligation_path.with_extension("json");
        let meta = if meta_path.exists() {
            let content = std::fs::read_to_string(&meta_path)
                .with_context(|| format!("reading {}", meta_path.display()))?;
            serde_json::from_str::<ObligationMeta>(&content).ok()
        } else {
            None
        };

        let obligation_name = obligation_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let pattern = meta.as_ref().map_or("unknown", |m| &m.pattern).to_string();
        let target = meta.as_ref().map_or(&obligation_name, |m| &m.target).to_string();
        let refines = meta.as_ref().map_or(String::new(), |m| m.refines.clone());
        let concern_name = meta
            .as_ref()
            .map_or_else(
                || {
                    obligation_name
                        .strip_prefix("Obligation_")
                        .unwrap_or(&obligation_name)
                        .to_string()
                },
                |m| m.concern.clone(),
            );
        let instance_module = meta.as_ref().and_then(|m| m.instance_module.clone());
        let invariant_name = meta
            .as_ref()
            .map_or("PatternObligation", |m| &m.invariant_name)
            .to_string();

        match &apalache_cmd {
            Some(cmd) => {
                let result = run_apalache(
                    cmd,
                    obligation_path,
                    &pattern,
                    &target,
                    &refines,
                    &concern_name,
                    instance_module.as_deref(),
                    &invariant_name,
                    project_root,
                )?;
                results.push(result);
            }
            None => {
                tracing::warn!(
                    "Apalache not found — skipping verification of {obligation_name}"
                );
                results.push(ObligationResult {
                    pattern,
                    target,
                    refines,
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
// Helpers
// ---------------------------------------------------------------------------

fn sanitize_target(target: &str) -> String {
    target.replace('.', "_")
}

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

#[allow(clippy::too_many_arguments)]
fn run_apalache(
    cmd: &str,
    obligation_path: &Path,
    pattern: &str,
    target: &str,
    refines: &str,
    concern_name: &str,
    instance_module: Option<&str>,
    invariant_name: &str,
    project_root: &Path,
) -> Result<ObligationResult> {
    let tla_dir = project_root.join("formal/tla");
    let ob_dir = obligation_path
        .parent()
        .context("obligation has no parent dir")?;

    // Copy the instance module next to the obligation so INSTANCE resolves
    if let Some(module_name) = instance_module {
        let src = tla_dir.join(format!("{module_name}.tla"));
        let dst = ob_dir.join(format!("{module_name}.tla"));
        if src.exists() && !dst.exists() {
            std::fs::copy(&src, &dst).with_context(|| {
                format!("copying {} -> {}", src.display(), dst.display())
            })?;
        }
    }

    let output = Command::new(cmd)
        .arg("check")
        .arg("--cinit=ConstInit")
        .arg(format!("--inv={invariant_name}"))
        .arg("--length=20")
        .arg(obligation_path)
        .output()
        .with_context(|| format!("running Apalache on {}", obligation_path.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let passed = output.status.success();

    Ok(ObligationResult {
        pattern: pattern.into(),
        target: target.into(),
        refines: refines.into(),
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
