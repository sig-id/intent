use std::path::Path;

use anyhow::{bail, Result};

use crate::parser::ast::{ParamValue, PatternApplication};

use super::PatternObligation;

struct CbParams {
    threshold: i64,
    probe_limit: i64,
}

fn extract_params(app: &PatternApplication) -> Result<CbParams> {
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

pub fn generate(
    concern_name: &str,
    app: &PatternApplication,
    project_root: &Path,
) -> Result<PatternObligation> {
    // Validate the spec file exists
    if let Some(ref refines) = app.refines {
        let spec_path = project_root.join(refines);
        if !spec_path.exists() {
            bail!(
                "TLA+ spec not found: {} (resolved to {})",
                refines,
                spec_path.display()
            );
        }
    }

    let params = extract_params(app)?;

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

    let mut tla = String::new();
    tla.push_str(&format!("---- MODULE Obligation_{concern_name} ----\n"));
    tla.push_str(&format!(
        "(* Auto-generated from formal/intent/{snake_name}.intent *)\n"
    ));
    tla.push_str("(* DO NOT EDIT — regenerated on every intent run. *)\n\n");
    tla.push_str("EXTENDS Integers, Sequences\n\n");

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

    tla.push_str("INSTANCE CircuitBreaker WITH\n");
    tla.push_str(&format!(
        "    FAILURE_THRESHOLD <- {},\n",
        params.threshold
    ));
    tla.push_str(&format!(
        "    HALF_OPEN_SUCCESS_THRESHOLD <- {},\n",
        params.probe_limit
    ));
    tla.push_str("    RECOVERY_TIMEOUT <- 0\n\n");

    tla.push_str("ConstInit == TRUE\n\n");

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

    Ok(PatternObligation {
        tla_content: tla,
        instance_module: Some("CircuitBreaker".into()),
        invariant_name: "PatternObligation".into(),
    })
}
