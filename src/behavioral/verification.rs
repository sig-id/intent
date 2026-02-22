//! TLA+ Verification - Running model checkers on generated specifications.
//!
//! This module provides integration with TLA+ model checkers:
//! - Apalache: Symbolic model checker for type checking and bounded verification
//! - TLC: Exhaustive model checker for complete state space exploration

use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use serde::Serialize;

/// Verification mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationMode {
    /// Fast verification: Apalache only, bounded checking
    Fast,
    /// Exhaustive verification: TLC with full state space exploration
    Exhaustive,
    /// Both: Run both Apalache and TLC
    Both,
}

/// What to verify
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Verification mode
    pub mode: VerificationMode,
    /// Maximum length for bounded checking (Apalache)
    pub max_length: usize,
    /// Check type invariants
    pub check_types: bool,
    /// Check state invariants
    pub check_invariants: bool,
    /// Check temporal properties
    pub check_temporal: bool,
    /// Timeout in seconds
    pub timeout: Option<usize>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            mode: VerificationMode::Fast,
            max_length: 10,
            check_types: true,
            check_invariants: true,
            check_temporal: false, // Temporal needs exhaustive mode
            timeout: Some(300), // 5 minutes
        }
    }
}

/// Result of verifying a single TLA+ module
#[derive(Debug, Clone, Serialize)]
pub struct ModuleVerificationResult {
    /// Module name
    pub module: String,
    /// File path
    pub file: PathBuf,
    /// Type checking result
    pub type_check: Option<CheckResult>,
    /// Invariant checking results
    pub invariants: Vec<InvariantResult>,
    /// Temporal property checking results
    pub temporal_properties: Vec<TemporalResult>,
    /// Overall status
    pub status: VerificationStatus,
    /// Duration in seconds
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum VerificationStatus {
    Pass,
    Fail,
    Error,
    Timeout,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub checker: String, // "apalache" or "tlc"
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvariantResult {
    pub name: String,
    pub passed: bool,
    pub checker: String,
    pub states_checked: Option<usize>,
    pub counterexample: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemporalResult {
    pub name: String,
    pub passed: bool,
    pub checker: String,
    pub detail: String,
}

/// Verify all TLA+ modules in a directory
pub fn verify_directory(
    dir: &Path,
    config: &VerificationConfig,
) -> Result<Vec<ModuleVerificationResult>> {
    let mut results = Vec::new();

    // Find all .tla files
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("tla") {
            let module_name = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let result = verify_module(&path, &module_name, config)?;
            results.push(result);
        }
    }

    Ok(results)
}

/// Verify a single TLA+ module
pub fn verify_module(
    tla_file: &Path,
    module_name: &str,
    config: &VerificationConfig,
) -> Result<ModuleVerificationResult> {
    let start = std::time::Instant::now();

    let mut type_check = None;
    let mut invariants = Vec::new();
    let mut temporal_properties = Vec::new();
    let mut overall_status = VerificationStatus::Pass;

    match config.mode {
        VerificationMode::Fast => {
            // Run Apalache only
            if config.check_types {
                match run_apalache_typecheck(tla_file) {
                    Ok(result) => {
                        if !result.passed {
                            overall_status = VerificationStatus::Fail;
                        }
                        type_check = Some(result);
                    }
                    Err(e) => {
                        overall_status = VerificationStatus::Error;
                        type_check = Some(CheckResult {
                            name: "TypeCheck".to_string(),
                            passed: false,
                            checker: "apalache".to_string(),
                            detail: format!("Error: {}", e),
                        });
                    }
                }
            }

            if config.check_invariants {
                match run_apalache_invariants(tla_file, config.max_length) {
                    Ok(results) => {
                        for inv in &results {
                            if !inv.passed {
                                overall_status = VerificationStatus::Fail;
                            }
                        }
                        invariants = results;
                    }
                    Err(_) => {
                        overall_status = VerificationStatus::Error;
                    }
                }
            }
        }

        VerificationMode::Exhaustive => {
            // Run TLC only
            match run_tlc_verification(tla_file, config) {
                Ok((inv_results, temp_results)) => {
                    for inv in &inv_results {
                        if !inv.passed {
                            overall_status = VerificationStatus::Fail;
                        }
                    }
                    for temp in &temp_results {
                        if !temp.passed {
                            overall_status = VerificationStatus::Fail;
                        }
                    }
                    invariants = inv_results;
                    temporal_properties = temp_results;
                }
                Err(_) => {
                    overall_status = VerificationStatus::Error;
                }
            }
        }

        VerificationMode::Both => {
            // Run both Apalache and TLC
            if config.check_types {
                if let Ok(result) = run_apalache_typecheck(tla_file) {
                    if !result.passed {
                        overall_status = VerificationStatus::Fail;
                    }
                    type_check = Some(result);
                }
            }

            if let Ok((inv_results, temp_results)) = run_tlc_verification(tla_file, config) {
                for inv in &inv_results {
                    if !inv.passed {
                        overall_status = VerificationStatus::Fail;
                    }
                }
                for temp in &temp_results {
                    if !temp.passed {
                        overall_status = VerificationStatus::Fail;
                    }
                }
                invariants = inv_results;
                temporal_properties = temp_results;
            }
        }
    }

    let duration = start.elapsed().as_secs_f64();

    Ok(ModuleVerificationResult {
        module: module_name.to_string(),
        file: tla_file.to_path_buf(),
        type_check,
        invariants,
        temporal_properties,
        status: overall_status,
        duration,
    })
}

/// Run Apalache type checking
fn run_apalache_typecheck(tla_file: &Path) -> Result<CheckResult> {
    let output = Command::new("apalache-mc")
        .arg("typecheck")
        .arg(tla_file)
        .output()
        .context("Failed to run apalache-mc")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    let passed = output.status.success()
        && combined.contains("Your types are purrfect!");

    Ok(CheckResult {
        name: "TypeCheck".to_string(),
        passed,
        checker: "apalache".to_string(),
        detail: if passed {
            "Type checking passed".to_string()
        } else {
            extract_error_message(&combined)
        },
    })
}

/// Run Apalache invariant checking
fn run_apalache_invariants(tla_file: &Path, max_length: usize) -> Result<Vec<InvariantResult>> {
    let invariants_to_check = vec!["TypeOK", "HistoryConsistent"];
    let mut results = Vec::new();

    for inv in invariants_to_check {
        let output = Command::new("apalache-mc")
            .arg("check")
            .arg(format!("--inv={}", inv))
            .arg(format!("--length={}", max_length))
            .arg(tla_file)
            .output()
            .context("Failed to run apalache-mc")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        let passed = output.status.success()
            && combined.contains("NoError");

        let states_checked = extract_states_checked(&combined);

        results.push(InvariantResult {
            name: inv.to_string(),
            passed,
            checker: "apalache".to_string(),
            states_checked,
            counterexample: if !passed {
                Some(extract_error_message(&combined))
            } else {
                None
            },
        });
    }

    Ok(results)
}

/// Run TLC verification
fn run_tlc_verification(
    tla_file: &Path,
    config: &VerificationConfig,
) -> Result<(Vec<InvariantResult>, Vec<TemporalResult>)> {
    // First, check if there's a .cfg file
    let cfg_file = tla_file.with_extension("cfg");

    // Create unique work directory for TLC
    let work_dir = tla_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".tlc_work_{}", std::process::id()));

    std::fs::create_dir_all(&work_dir)?;

    let mut cmd = Command::new("tlc");
    cmd.arg("-workers").arg("auto");
    cmd.arg("-metadir").arg(&work_dir); // Use unique metadata directory

    if cfg_file.exists() {
        cmd.arg("-config").arg(&cfg_file);
    }

    if let Some(timeout) = config.timeout {
        // TLC doesn't have built-in timeout, would need external wrapper
        // For now, we'll just run it
        let _ = timeout; // Use later with timeout wrapper
    }

    cmd.arg(tla_file);

    let output = cmd.output().context("Failed to run tlc")?;

    // Clean up work directory
    let _ = std::fs::remove_dir_all(&work_dir);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    let passed = combined.contains("Model checking completed. No error has been found.");

    let mut invariants = Vec::new();
    let mut temporal = Vec::new();

    if passed {
        // Extract statistics
        let states = extract_tlc_states(&combined);

        invariants.push(InvariantResult {
            name: "All invariants".to_string(),
            passed: true,
            checker: "tlc".to_string(),
            states_checked: Some(states),
            counterexample: None,
        });

        if combined.contains("temporal properties") {
            temporal.push(TemporalResult {
                name: "All temporal properties".to_string(),
                passed: true,
                checker: "tlc".to_string(),
                detail: format!("{} states explored", states),
            });
        }
    } else {
        invariants.push(InvariantResult {
            name: "Verification".to_string(),
            passed: false,
            checker: "tlc".to_string(),
            states_checked: None,
            counterexample: Some(extract_error_message(&combined)),
        });
    }

    Ok((invariants, temporal))
}

fn extract_error_message(output: &str) -> String {
    // Look for specific error patterns
    for line in output.lines() {
        if line.contains("Error:") || line.contains("ERROR:") {
            return line.trim().to_string();
        }
        if line.starts_with("error:") || line.contains("has found an error") {
            return line.trim().to_string();
        }
        if line.contains("Parsing error:") || line.contains("Type error:") {
            return line.trim().to_string();
        }
    }

    // Look for outcome
    for line in output.lines() {
        if line.contains("The outcome is:") {
            return line.trim().to_string();
        }
    }

    // Fallback: first few lines
    output.lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_states_checked(output: &str) -> Option<usize> {
    for line in output.lines() {
        if line.contains("distinct states found") {
            // Try to extract number
            let words: Vec<&str> = line.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                if word == &"distinct" && i > 0 {
                    if let Ok(n) = words[i - 1].parse::<usize>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

fn extract_tlc_states(output: &str) -> usize {
    for line in output.lines() {
        if line.contains("states generated") {
            let words: Vec<&str> = line.split_whitespace().collect();
            if let Some(num_str) = words.first() {
                if let Ok(n) = num_str.parse::<usize>() {
                    return n;
                }
            }
        }
    }
    0
}
