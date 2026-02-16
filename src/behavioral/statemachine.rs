//! State machine to TLA+ transpiler.
//!
//! This module converts Intent state machine declarations to TLA+ specifications
//! that can be verified with Apalache or TLC.

use std::path::Path;

use anyhow::Result;

use crate::parser::ast::{SmInvariantKind, StateMachineDecl};

/// A generated TLA+ module for a state machine.
pub struct StateMachineTla {
    /// TLA+ module content.
    pub content: String,
    /// The module name.
    pub module_name: String,
    /// Invariants to check.
    pub invariants: Vec<String>,
}

/// Generate a TLA+ specification from an Intent state machine.
pub fn generate(sm: &StateMachineDecl, concern_name: &str, _project_root: &Path) -> Result<StateMachineTla> {
    let module_name = format!("{}_{}", concern_name, sm.name);

    let mut tla = String::new();

    // Module header
    tla.push_str(&format!("---- MODULE {} ----\n", module_name));
    tla.push_str("EXTENDS TLC, Sequences, Integers\n\n");

    // Variables
    tla.push_str("VARIABLES state, history\n\n");

    // State type definition
    tla.push_str("States == {");
    for (i, s) in sm.states.iter().enumerate() {
        if i > 0 {
            tla.push_str(", ");
        }
        tla.push_str(&format!("\"{}\"", s));
    }
    tla.push_str("}\n\n");

    // Initial states
    tla.push_str(&format!("Init == state = \"{}\" /\\ history = << >>\n\n", sm.initial));

    // Transitions
    let mut transition_defs = Vec::new();
    for (from, to) in &sm.transitions {
        let trans_name = format!("{}_to_{}", from.to_lowercase(), to.to_lowercase());
        tla.push_str(&format!(
            "{} == state = \"{}\" /\\ state' = \"{}\" /\\ history' = Append(history, \"{}\")\n\n",
            trans_name, from, to, trans_name
        ));
        transition_defs.push(trans_name);
    }

    // Next relation (disjunction of all transitions)
    tla.push_str("Next == ");
    for (i, trans) in transition_defs.iter().enumerate() {
        if i > 0 {
            tla.push_str(" \\/ ");
        }
        tla.push_str(trans);
    }
    if transition_defs.is_empty() {
        tla.push_str("UNCHANGED <<state, history>>");
    }
    tla.push_str("\n\n");

    // Terminal states stay terminal (absorbing)
    if !sm.terminal.is_empty() {
        tla.push_str("Terminal == {");
        for (i, t) in sm.terminal.iter().enumerate() {
            if i > 0 {
                tla.push_str(", ");
            }
            tla.push_str(&format!("\"{}\"", t));
        }
        tla.push_str("}\n\n");

        tla.push_str("TerminalAbsorbing == state \\in Terminal => UNCHANGED state\n\n");
    }

    // Specification
    tla.push_str("Spec == Init /\\ [][Next]_<<state, history>>\n\n");

    // Invariants
    let mut invariants = Vec::new();
    for inv in &sm.invariants {
        let (inv_name, inv_def) = match &inv.kind {
            SmInvariantKind::MustNotReach { from, to } => {
                let name = format!("Inv_{}", inv.name);
                let def = format!(
                    "{} == state /= \"{}\" \\/ history /= << >> =>\n    ~(\"{}\" \\in history)",
                    name, from, to
                );
                (name, def)
            }
            SmInvariantKind::WasVisited { target_state, required_prior } => {
                let name = format!("Inv_{}", inv.name);
                let def = format!(
                    "{} == state = \"{}\" => \"{}\" \\in history",
                    name, target_state, required_prior
                );
                (name, def)
            }
            SmInvariantKind::TerminalAbsorbing => {
                let name = "Inv_TerminalAbsorbing".to_string();
                let def = "Inv_TerminalAbsorbing == state \\in Terminal => UNCHANGED state".to_string();
                (name, def)
            }
            SmInvariantKind::Custom { expr } => {
                let name = format!("Inv_{}", inv.name);
                let def = format!("{} == {}", name, expr);
                (name, def)
            }
        };
        tla.push_str(&inv_def);
        tla.push_str("\n\n");
        invariants.push(inv_name);
    }

    // Type correctness invariant
    tla.push_str(&format!(
        "TypeOK == state \\in States /\\ history \\in Seq(STRING)\n\n"
    ));
    invariants.push("TypeOK".to_string());

    // Module footer
    tla.push_str("====\n");

    Ok(StateMachineTla {
        content: tla,
        module_name,
        invariants,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_statemachine() {
        let sm = StateMachineDecl {
            name: "Order".to_string(),
            states: vec!["PENDING".to_string(), "PAID".to_string(), "SHIPPED".to_string()],
            initial: "PENDING".to_string(),
            terminal: vec!["SHIPPED".to_string()],
            transitions: vec![
                ("PENDING".to_string(), "PAID".to_string()),
                ("PAID".to_string(), "SHIPPED".to_string()),
            ],
            invariants: vec![],
            refines: None,
        };

        let result = generate(&sm, "Test", Path::new(".")).unwrap();
        assert!(result.content.contains("MODULE Test_Order"));
        assert!(result.content.contains("PENDING"));
        assert!(result.content.contains("pending_to_paid"));
        assert!(result.invariants.contains(&"TypeOK".to_string()));
    }

    #[test]
    fn test_statemachine_with_invariants() {
        let sm = StateMachineDecl {
            name: "Order".to_string(),
            states: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            initial: "A".to_string(),
            terminal: vec!["C".to_string()],
            transitions: vec![
                ("A".to_string(), "B".to_string()),
                ("B".to_string(), "C".to_string()),
            ],
            invariants: vec![crate::parser::ast::SmInvariant {
                name: "no_skip".to_string(),
                kind: SmInvariantKind::MustNotReach {
                    from: "A".to_string(),
                    to: "C".to_string(),
                },
            }],
            refines: None,
        };

        let result = generate(&sm, "Test", Path::new(".")).unwrap();
        assert!(result.content.contains("Inv_no_skip"));
    }
}
