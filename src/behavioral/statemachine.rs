//! State machine to TLA+ transpiler.
//!
//! Generates TLA+ specifications from Intent behaviors with full LTL support.

use std::path::Path;

use anyhow::Result;

use crate::behavioral::composition::{compose_behaviors, CompositionConfig};
use crate::parser::ast::{
    ArithOp, BehaviorDecl, ComparisonOp, EffectKind, Expr, FairnessKind, FairnessSpec,
    InvariantDecl, LogicalOp, StateDecl, TemporalExpr, TemporalOp, TemporalProperty,
    TransitionDecl, UnaryOp,
};
use std::collections::HashSet;

/// A generated TLA+ module for a state machine.
pub struct StateMachineTla {
    /// TLA+ module content.
    pub content: String,
    /// The module name.
    pub module_name: String,
    /// Invariants to check.
    pub invariants: Vec<String>,
    /// Temporal properties to check.
    pub properties: Vec<String>,
}

/// Generate a TLA+ specification from an Intent behavior.
///
/// If the behavior composes other behaviors, this function will:
/// 1. Resolve the composed behaviors from the system
/// 2. Merge them using the composition module
/// 3. Generate TLA+ for the combined behavior
pub fn generate(
    behavior: &BehaviorDecl,
    system_name: &str,
    _project_root: &Path,
) -> Result<StateMachineTla> {
    // Check if this behavior composes others
    if !behavior.composes.is_empty() {
        // For now, we can't resolve composed behaviors without access to the full system.
        // This would require a different API that passes in all available behaviors.
        // For now, we'll generate TLA+ for just this behavior's direct states/transitions,
        // but note in a comment that composition was requested.
        // A full implementation would need to receive a behavior registry.
        return generate_with_composition_note(behavior, system_name);
    }

    generate_single(behavior, system_name)
}

/// Generate TLA+ for a single behavior (no composition).
fn generate_single(behavior: &BehaviorDecl, system_name: &str) -> Result<StateMachineTla> {
    let module_name = format!("{}_{}", system_name, behavior.name);
    let mut tla = TlaGenerator::new(&module_name);

    // Pre-scan for variables and events
    tla.extract_symbols(behavior);

    tla.generate_header();
    tla.generate_constants(&behavior.states);
    tla.generate_variables_extended();
    tla.generate_events();
    tla.generate_init_extended(&behavior.states);
    tla.generate_transitions(&behavior.transitions);
    tla.generate_next(&behavior.transitions);
    tla.generate_stuttering();
    tla.generate_fairness(&behavior.fairness, &behavior.transitions);
    tla.generate_spec(&behavior.fairness);
    tla.generate_type_invariant(&behavior.states);
    tla.generate_user_invariants(&behavior.invariants);
    tla.generate_properties(&behavior.properties);
    tla.generate_liveness_helpers(&behavior.states);
    tla.generate_deadlock_freedom(&behavior.transitions);
    tla.generate_reachability(&behavior.states);
    tla.generate_refinement_theorem(behavior);
    tla.generate_model_check_config(&behavior.states);
    tla.generate_footer();

    let invariants: Vec<String> = behavior
        .invariants
        .iter()
        .map(|i| i.name.clone())
        .chain(std::iter::once("TypeOK".to_string()))
        .collect();

    let properties: Vec<String> = behavior
        .properties
        .iter()
        .map(|p| format!("Prop_{}", p.name))
        .collect();

    Ok(StateMachineTla {
        content: tla.output,
        module_name,
        invariants,
        properties,
    })
}

/// Generate TLA+ for a behavior that composes others.
///
/// Since we don't have access to the composed behaviors, we generate
/// the TLA+ with a note about the composition requirement.
fn generate_with_composition_note(
    behavior: &BehaviorDecl,
    system_name: &str,
) -> Result<StateMachineTla> {
    // Generate as if single, but add composition comment
    let mut result = generate_single(behavior, system_name)?;

    // Add note about composition at the beginning
    let composition_note = format!(
        "\\* NOTE: This behavior composes [{}]\n\\* Full composition requires resolving all source behaviors.\n\n",
        behavior.composes.join(", ")
    );
    result.content = composition_note + &result.content;

    Ok(result)
}

/// Generate TLA+ for a composed behavior with all source behaviors provided.
///
/// This is the full-featured version that properly handles composition.
pub fn generate_composed(
    behavior: &BehaviorDecl,
    source_behaviors: &[(&str, &BehaviorDecl)],
    system_name: &str,
    config: Option<CompositionConfig>,
) -> Result<StateMachineTla> {
    // Compose the behaviors
    let composition_config = config.unwrap_or_default();
    let composed = compose_behaviors(&behavior.name, source_behaviors, &composition_config)?;

    // Convert to BehaviorDecl and generate TLA+
    let composed_decl = composed.to_behavior_decl();

    // Generate TLA+ with composition note
    let mut result = generate_single(&composed_decl, system_name)?;

    // Add composition info
    let sources: Vec<&str> = source_behaviors.iter().map(|(name, _)| *name).collect();
    let composition_note = format!(
        "\\* Composed from: {}\n\\* Conflicts: {}\n\n",
        sources.join(", "),
        composed.conflicts.len()
    );
    result.content = composition_note + &result.content;

    Ok(result)
}

struct TlaGenerator {
    module_name: String,
    output: String,
    indent: usize,
    /// Variables extracted from guards and effects
    extracted_vars: HashSet<String>,
    /// Events/messages that are emitted
    events: HashSet<String>,
}

impl TlaGenerator {
    fn new(module_name: &str) -> Self {
        Self {
            module_name: module_name.to_string(),
            output: String::new(),
            indent: 0,
            extracted_vars: HashSet::new(),
            events: HashSet::new(),
        }
    }

    /// Pre-scan behavior to extract all referenced variables and events
    fn extract_symbols(&mut self, behavior: &BehaviorDecl) {
        for t in &behavior.transitions {
            if let Some(ref guard) = t.guard {
                self.collect_vars_from_expr(guard);
            }
            for effect in &t.effects {
                self.collect_from_effect(effect);
            }
        }
        for inv in &behavior.invariants {
            self.collect_vars_from_expr(&inv.expr);
        }
    }

    fn collect_vars_from_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => {
                if !self.is_state_name(name) {
                    self.extracted_vars.insert(name.clone());
                }
            }
            Expr::DottedName(name) => {
                self.extracted_vars.insert(name.clone());
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    self.collect_vars_from_expr(arg);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::CompOp { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::UnaryOp { expr, .. } => {
                self.collect_vars_from_expr(expr);
            }
            _ => {}
        }
    }

    fn collect_from_effect(&mut self, effect: &crate::parser::ast::EffectStmt) {
        match &effect.kind {
            EffectKind::Emit { name, args } => {
                self.events.insert(name.clone());
                for arg in args {
                    self.collect_vars_from_expr(arg);
                }
            }
            EffectKind::If { cond, then_effects, else_effects } => {
                self.collect_vars_from_expr(cond);
                for e in then_effects {
                    self.collect_from_effect(e);
                }
                if let Some(else_effs) = else_effects {
                    for e in else_effs {
                        self.collect_from_effect(e);
                    }
                }
            }
            EffectKind::Expr(e) => {
                self.collect_vars_from_expr(e);
            }
        }
    }

    fn is_state_name(&self, _name: &str) -> bool {
        false
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn blank(&mut self) {
        self.output.push('\n');
    }

    fn generate_header(&mut self) {
        let dashes = "-".repeat(self.module_name.len() + 8);
        self.line(&format!("{}  MODULE {}  {}", dashes, self.module_name, dashes));
        self.line("EXTENDS Naturals, Sequences, TLC");
        self.blank();
    }

    fn generate_footer(&mut self) {
        let dashes = "=".repeat(self.module_name.len() + 20);
        self.line(&dashes);
    }

    fn generate_constants(&mut self, states: &[StateDecl]) {
        self.line("\\* State constants");
        self.line("CONSTANTS");
        self.indent += 1;
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
        self.line(&state_names.join(", "));
        self.indent -= 1;
        self.blank();

        self.line("States == {");
        self.indent += 1;
        self.line(&state_names.join(", "));
        self.indent -= 1;
        self.line("}");
        self.blank();
    }

    fn generate_variables_extended(&mut self) {
        self.line("VARIABLES");
        self.indent += 1;
        self.line("state,      \\* Current state");
        self.line("pc,         \\* Program counter for step tracking");
        self.line("history,    \\* Sequence of visited states (for trace analysis)");
        self.line("pending     \\* Pending events/messages queue");
        self.indent -= 1;
        self.blank();
        self.line("vars == <<state, pc, history, pending>>");
        self.blank();

        // Generate auxiliary variable declarations if we extracted any
        if !self.extracted_vars.is_empty() {
            self.output.push_str("\\* Auxiliary variables referenced in guards/effects\n");
            self.output.push_str("\\* These would be instantiated with actual values in a model check\n");
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for var in vars {
                self.output.push_str(&format!("\\* DECLARE: {}\n", var));
            }
            self.output.push('\n');
        }
    }

    fn generate_events(&mut self) {
        if self.events.is_empty() {
            return;
        }

        let mut events: Vec<String> = self.events.iter().cloned().collect();
        events.sort();
        self.output.push_str("\\* Event types emitted by this behavior\n");
        self.output.push_str(&format!("Events == {{\"{}\" }}\n\n", events.join("\", \"")));
    }

    fn generate_init_extended(&mut self, states: &[StateDecl]) {
        let initial: Vec<&str> = states
            .iter()
            .filter(|s| s.initial)
            .map(|s| s.name.as_str())
            .collect();

        self.line("Init ==");
        self.indent += 1;
        if initial.len() == 1 {
            self.line(&format!("/\\ state = {}", initial[0]));
        } else if initial.is_empty() {
            self.line("/\\ state \\in States");
        } else {
            self.line(&format!("/\\ state \\in {{{}}}", initial.join(", ")));
        }
        self.line("/\\ pc = 0");
        self.line("/\\ history = <<>>");
        self.line("/\\ pending = <<>>");
        self.indent -= 1;
        self.blank();
    }

    fn generate_stuttering(&mut self) {
        self.line("\\* Stuttering step (system does nothing)");
        self.line("Stutter ==");
        self.indent += 1;
        self.line("UNCHANGED vars");
        self.indent -= 1;
        self.blank();
    }

    fn generate_transitions(&mut self, transitions: &[TransitionDecl]) {
        self.line("\\* Transition actions");
        for t in transitions {
            let action_name = format!("{}_{}", t.from, t.on_event);
            self.line(&format!("{} ==", action_name));
            self.indent += 1;
            self.line(&format!("/\\ state = {}", t.from));

            if let Some(ref guard) = t.guard {
                self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
            }

            self.line(&format!("/\\ state' = {}", t.to));
            self.line("/\\ pc' = pc + 1");
            self.line("/\\ history' = Append(history, state)");

            // Handle effects - emit events go to pending queue
            let emits: Vec<_> = t.effects.iter()
                .filter_map(|e| match &e.kind {
                    EffectKind::Emit { name, args } => Some((name, args)),
                    _ => None,
                })
                .collect();

            if emits.is_empty() {
                self.line("/\\ pending' = pending");
            } else {
                // Build a sequence of emitted events
                let emit_strs: Vec<String> = emits.iter()
                    .map(|(name, args)| {
                        let args_str: Vec<String> = args.iter().map(|a| self.expr_to_tla(a)).collect();
                        if args_str.is_empty() {
                            format!("[type |-> \"{}\"]", name)
                        } else {
                            format!("[type |-> \"{}\", args |-> <<{}>>]", name, args_str.join(", "))
                        }
                    })
                    .collect();
                self.line(&format!("/\\ pending' = pending \\o <<{}>>", emit_strs.join(", ")));
            }

            // Generate comments for non-emit effects
            for effect in &t.effects {
                self.generate_effect_comment(effect);
            }

            self.indent -= 1;
            self.blank();
        }
    }

    fn generate_effect_comment(&mut self, effect: &crate::parser::ast::EffectStmt) {
        match &effect.kind {
            EffectKind::Emit { .. } => {
                // Already handled in pending queue
            }
            EffectKind::If { cond, then_effects, else_effects } => {
                self.line(&format!("\\* IF {} THEN", self.expr_to_tla(cond)));
                for e in then_effects {
                    self.generate_effect_comment(e);
                }
                if let Some(else_effs) = else_effects {
                    self.line("\\* ELSE");
                    for e in else_effs {
                        self.generate_effect_comment(e);
                    }
                }
            }
            EffectKind::Expr(e) => {
                self.line(&format!("\\* EFFECT: {}", self.expr_to_tla(e)));
            }
        }
    }

    fn generate_next(&mut self, transitions: &[TransitionDecl]) {
        self.line("Next ==");
        self.indent += 1;

        if transitions.is_empty() {
            self.line("UNCHANGED vars");
        } else {
            let actions: Vec<String> = transitions
                .iter()
                .map(|t| format!("{}_{}", t.from, t.on_event))
                .collect();

            for (i, action) in actions.iter().enumerate() {
                if i == 0 {
                    self.line(&format!("\\/ {}", action));
                } else {
                    self.line(&format!("\\/ {}", action));
                }
            }
        }

        self.indent -= 1;
        self.blank();
    }

    fn generate_fairness(&mut self, fairness: &[FairnessSpec], transitions: &[TransitionDecl]) {
        if fairness.is_empty() {
            return;
        }

        self.line("\\* Fairness conditions");
        for f in fairness {
            let action_name = self.find_action_name(f, transitions);
            let fair_type = match f.kind {
                FairnessKind::Weak => "WF",
                FairnessKind::Strong => "SF",
            };
            self.line(&format!(
                "Fairness_{}_to_{} == {}_vars({})",
                f.from, f.to, fair_type, action_name
            ));
        }
        self.blank();
    }

    fn find_action_name(&self, f: &FairnessSpec, transitions: &[TransitionDecl]) -> String {
        for t in transitions {
            if t.from == f.from && t.to == f.to {
                return format!("{}_{}", t.from, t.on_event);
            }
        }
        format!("{}_{}", f.from, f.to)
    }

    fn generate_spec(&mut self, fairness: &[FairnessSpec]) {
        self.line("Spec ==");
        self.indent += 1;
        self.line("/\\ Init");
        self.line("/\\ [][Next]_vars");

        for f in fairness {
            let fair_type = match f.kind {
                FairnessKind::Weak => "WF",
                FairnessKind::Strong => "SF",
            };
            self.line(&format!("/\\ {}_vars(Next)", fair_type));
        }

        self.indent -= 1;
        self.blank();
    }

    fn generate_type_invariant(&mut self, states: &[StateDecl]) {
        self.line("\\* Type invariant");
        self.line("TypeOK ==");
        self.indent += 1;
        self.line("/\\ state \\in States");
        self.line("/\\ pc \\in Nat");
        self.line("/\\ history \\in Seq(States)");
        self.indent -= 1;
        self.blank();

        let terminals: Vec<&str> = states
            .iter()
            .filter(|s| s.terminal)
            .map(|s| s.name.as_str())
            .collect();

        if !terminals.is_empty() {
            self.line("\\* Terminal states");
            self.line(&format!("TerminalStates == {{{}}}", terminals.join(", ")));
            self.blank();

            // Add terminal state invariant
            self.line("\\* Once in terminal state, cannot leave");
            self.line("TerminalStable ==");
            self.indent += 1;
            self.line("[](state \\in TerminalStates => [](state \\in TerminalStates))");
            self.indent -= 1;
            self.blank();
        }

        // History tracking invariant
        self.line("\\* History length matches step count");
        self.line("HistoryConsistent ==");
        self.indent += 1;
        self.line("Len(history) = pc");
        self.indent -= 1;
        self.blank();
    }

    fn generate_user_invariants(&mut self, invariants: &[InvariantDecl]) {
        if invariants.is_empty() {
            return;
        }

        self.line("\\* User-defined invariants");
        for inv in invariants {
            self.line(&format!("Inv_{} ==", inv.name));
            self.indent += 1;
            self.line(&self.expr_to_tla(&inv.expr));
            self.indent -= 1;
            self.blank();
        }
    }

    fn generate_refinement_theorem(&mut self, behavior: &BehaviorDecl) {
        if behavior.refines.is_none() {
            return;
        }

        let refines = behavior.refines.as_ref().unwrap();
        self.line("\\* Refinement relationship");
        self.line(&format!("\\* This behavior refines: {}", refines));
        self.line(&format!(
            "THEOREM Spec => {}!Spec",
            refines.replace(".tla", "").replace("/", "_")
        ));
        self.blank();
    }

    fn generate_properties(&mut self, properties: &[TemporalProperty]) {
        if properties.is_empty() {
            return;
        }

        self.line("\\* Temporal properties (LTL)");
        for prop in properties {
            let tla_expr = self.temporal_to_tla(&prop.expr);
            self.line(&format!("Prop_{} == {}", prop.name, tla_expr));
        }
        self.blank();
    }

    fn generate_liveness_helpers(&mut self, states: &[StateDecl]) {
        let terminals: Vec<&str> = states
            .iter()
            .filter(|s| s.terminal)
            .map(|s| s.name.as_str())
            .collect();

        if terminals.is_empty() {
            return;
        }

        self.line("\\* Liveness: Eventually reaches a terminal state");
        self.line("Liveness ==");
        self.indent += 1;
        self.line(&format!("<>(state \\in {{{}}})", terminals.join(", ")));
        self.indent -= 1;
        self.blank();
    }

    fn generate_deadlock_freedom(&mut self, transitions: &[TransitionDecl]) {
        if transitions.is_empty() {
            return;
        }

        // Group transitions by source state
        let mut sources: HashSet<&str> = HashSet::new();
        for t in transitions {
            sources.insert(&t.from);
        }

        self.line("\\* Deadlock freedom: From every non-terminal state, some action is enabled");
        self.line("DeadlockFree ==");
        self.indent += 1;
        self.line("[](state \\notin TerminalStates => ENABLED(Next))");
        self.indent -= 1;
        self.blank();
    }

    fn generate_reachability(&mut self, states: &[StateDecl]) {
        self.line("\\* Reachability helpers for model checking");
        for s in states {
            self.line(&format!("CanReach_{} == <>(state = {})", s.name, s.name));
        }
        self.blank();
    }

    fn generate_model_check_config(&mut self, states: &[StateDecl]) {
        self.line("\\* Model checking configuration");
        self.line("\\* Use with TLC or Apalache:");
        self.line("\\*   CONSTANTS");
        for s in states {
            self.line(&format!("\\*     {} = \"{}\"", s.name, s.name));
        }
        self.line("\\*   SPECIFICATION Spec");
        self.line("\\*   INVARIANTS TypeOK, HistoryConsistent");
        self.line("\\*   PROPERTIES Liveness, TerminalStable");
        self.blank();
    }

    fn temporal_to_tla(&self, expr: &TemporalExpr) -> String {
        match expr {
            TemporalExpr::Always(inner) => {
                format!("[]({})", self.temporal_to_tla(inner))
            }
            TemporalExpr::Eventually(inner) => {
                format!("<>({})", self.temporal_to_tla(inner))
            }
            TemporalExpr::Next(inner) => {
                let inner_tla = self.temporal_to_tla(inner);
                if inner_tla.starts_with("state = ") {
                    format!("({})'", inner_tla)
                } else {
                    format!("({})'", inner_tla)
                }
            }
            TemporalExpr::Until { lhs, rhs } => {
                format!(
                    "({}) \\U ({})",
                    self.temporal_to_tla(lhs),
                    self.temporal_to_tla(rhs)
                )
            }
            TemporalExpr::Release { lhs, rhs } => {
                // φ R ψ ≡ ¬(¬φ U ¬ψ)
                format!(
                    "~((~({})) \\U (~({})))",
                    self.temporal_to_tla(lhs),
                    self.temporal_to_tla(rhs)
                )
            }
            TemporalExpr::WeakUntil { lhs, rhs } => {
                // φ W ψ ≡ (φ U ψ) ∨ []φ
                let lhs_tla = self.temporal_to_tla(lhs);
                let rhs_tla = self.temporal_to_tla(rhs);
                format!(
                    "(({}) \\U ({})) \\/ []({})",
                    lhs_tla, rhs_tla, lhs_tla
                )
            }
            TemporalExpr::StrongRelease { lhs, rhs } => {
                // φ M ψ ≡ (φ R ψ) ∧ <>φ
                let lhs_tla = self.temporal_to_tla(lhs);
                let rhs_tla = self.temporal_to_tla(rhs);
                let release = format!("~((~({})) \\U (~({})))", lhs_tla, rhs_tla);
                format!("({}) /\\ <>({})", release, lhs_tla)
            }
            TemporalExpr::AlwaysImplies { premise, conclusion } => {
                format!(
                    "[]({} => <>({}))",
                    self.temporal_to_tla(premise),
                    self.temporal_to_tla(conclusion)
                )
            }
            TemporalExpr::State(name) => {
                format!("state = {}", name)
            }
            TemporalExpr::BinOp { lhs, op, rhs } => {
                let op_str = match op {
                    TemporalOp::And => "/\\",
                    TemporalOp::Or => "\\/",
                    TemporalOp::Implies => "=>",
                };
                format!(
                    "({}) {} ({})",
                    self.temporal_to_tla(lhs),
                    op_str,
                    self.temporal_to_tla(rhs)
                )
            }
        }
    }

    fn expr_to_tla(&self, expr: &Expr) -> String {
        match expr {
            Expr::Ident(name) => name.clone(),
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) => f.to_string(),
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Expr::String(s) => format!("\"{}\"", s),
            Expr::DottedName(name) => name.clone(),
            Expr::Duration(ms) => ms.to_string(),
            Expr::Call { name, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.expr_to_tla(a)).collect();
                format!("{}({})", name, args_str.join(", "))
            }
            Expr::BinOp { lhs, op, rhs } => {
                let op_str = match op {
                    ArithOp::Add => "+",
                    ArithOp::Sub => "-",
                    ArithOp::Mul => "*",
                    ArithOp::Div => "\\div",
                };
                format!("({} {} {})", self.expr_to_tla(lhs), op_str, self.expr_to_tla(rhs))
            }
            Expr::CompOp { lhs, op, rhs } => {
                let op_str = match op {
                    ComparisonOp::Eq => "=",
                    ComparisonOp::Ne => "/=",
                    ComparisonOp::Lt => "<",
                    ComparisonOp::Le => "<=",
                    ComparisonOp::Gt => ">",
                    ComparisonOp::Ge => ">=",
                };
                format!("({} {} {})", self.expr_to_tla(lhs), op_str, self.expr_to_tla(rhs))
            }
            Expr::LogicalOp { lhs, op, rhs } => {
                let op_str = match op {
                    LogicalOp::And => "/\\",
                    LogicalOp::Or => "\\/",
                };
                format!("({} {} {})", self.expr_to_tla(lhs), op_str, self.expr_to_tla(rhs))
            }
            Expr::UnaryOp { op, expr } => {
                let op_str = match op {
                    UnaryOp::Not => "~",
                    UnaryOp::Neg => "-",
                };
                format!("{}({})", op_str, self.expr_to_tla(expr))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::*;

    fn make_test_behavior() -> BehaviorDecl {
        BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                StateDecl { name: "idle".to_string(), initial: true, terminal: false },
                StateDecl { name: "active".to_string(), initial: false, terminal: false },
                StateDecl { name: "done".to_string(), initial: false, terminal: true },
            ],
            transitions: vec![
                TransitionDecl {
                    from: "idle".to_string(),
                    to: "active".to_string(),
                    on_event: "start".to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: None,
                },
                TransitionDecl {
                    from: "active".to_string(),
                    to: "done".to_string(),
                    on_event: "finish".to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: None,
                },
            ],
            properties: vec![
                TemporalProperty {
                    name: "eventually_done".to_string(),
                    expr: TemporalExpr::Eventually(Box::new(TemporalExpr::State("done".to_string()))),
                },
                TemporalProperty {
                    name: "active_until_done".to_string(),
                    expr: TemporalExpr::Until {
                        lhs: Box::new(TemporalExpr::State("active".to_string())),
                        rhs: Box::new(TemporalExpr::State("done".to_string())),
                    },
                },
            ],
            fairness: vec![
                FairnessSpec {
                    kind: FairnessKind::Weak,
                    from: "idle".to_string(),
                    to: "active".to_string(),
                    alts: vec![],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_generate_tla() {
        let behavior = make_test_behavior();
        let result = generate(&behavior, "TestSystem", Path::new(".")).unwrap();

        assert_eq!(result.module_name, "TestSystem_TestMachine");
        assert!(result.content.contains("MODULE TestSystem_TestMachine"));
        assert!(result.content.contains("VARIABLES"));
        assert!(result.content.contains("Init =="));
        assert!(result.content.contains("Next =="));
        assert!(result.content.contains("TypeOK =="));
    }

    #[test]
    fn test_temporal_to_tla_always() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Always(Box::new(TemporalExpr::State("active".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "[](state = active)");
    }

    #[test]
    fn test_temporal_to_tla_eventually() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Eventually(Box::new(TemporalExpr::State("done".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "<>(state = done)");
    }

    #[test]
    fn test_temporal_to_tla_next() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Next(Box::new(TemporalExpr::State("active".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "(state = active)'");
    }

    #[test]
    fn test_temporal_to_tla_until() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Until {
            lhs: Box::new(TemporalExpr::State("active".to_string())),
            rhs: Box::new(TemporalExpr::State("done".to_string())),
        };
        assert_eq!(gen.temporal_to_tla(&expr), "(state = active) \\U (state = done)");
    }

    #[test]
    fn test_temporal_to_tla_release() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Release {
            lhs: Box::new(TemporalExpr::State("done".to_string())),
            rhs: Box::new(TemporalExpr::State("active".to_string())),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "~((~(state = done)) \\U (~(state = active)))"
        );
    }

    #[test]
    fn test_temporal_to_tla_weak_until() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::WeakUntil {
            lhs: Box::new(TemporalExpr::State("active".to_string())),
            rhs: Box::new(TemporalExpr::State("done".to_string())),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "((state = active) \\U (state = done)) \\/ [](state = active)"
        );
    }

    #[test]
    fn test_temporal_to_tla_strong_release() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::StrongRelease {
            lhs: Box::new(TemporalExpr::State("done".to_string())),
            rhs: Box::new(TemporalExpr::State("active".to_string())),
        };
        let result = gen.temporal_to_tla(&expr);
        assert!(result.contains("~((~(state = done)) \\U (~(state = active)))"));
        assert!(result.contains("<>(state = done)"));
    }

    #[test]
    fn test_temporal_to_tla_nested() {
        let gen = TlaGenerator::new("Test");
        // always(idle => eventually(done))
        let expr = TemporalExpr::Always(Box::new(TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::State("idle".to_string())),
            op: TemporalOp::Implies,
            rhs: Box::new(TemporalExpr::Eventually(Box::new(TemporalExpr::State(
                "done".to_string(),
            )))),
        }));
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "[]((state = idle) => (<>(state = done)))"
        );
    }
}
