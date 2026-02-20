//! State machine to TLA+ transpiler.
//!
//! Generates TLA+ specifications from Intent behaviors with full LTL support.

use std::path::Path;

use anyhow::Result;

use crate::behavioral::composition::{compose_behaviors, CompositionConfig};
use crate::parser::ast::{
    ArithOp, BehaviorDecl, ComparisonOp, EffectKind, Expr, FairnessKind, FairnessSpec,
    InvariantDecl, LogicalOp, Span, StateDecl, TemporalExpr, TemporalOp, TemporalProperty,
    TransitionDecl, TransitionSource, TransitionTarget, UnaryOp,
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
    /// Optional TLC configuration file.
    pub tlc_cfg: Option<TlcConfig>,
}

/// Configuration options for TLA+ generation.
#[derive(Debug, Clone, Default)]
pub struct TlaConfig {
    /// Generate Apalache-compatible type annotations
    pub apalache_types: bool,
    /// Include model checking configuration block
    pub include_mc_config: bool,
    /// Generate TLC-specific operators
    pub tlc_compat: bool,
    /// Generate TLC .cfg file content (returned separately)
    pub generate_cfg: bool,
}

/// A generated TLC configuration file.
#[derive(Debug, Clone, Default)]
pub struct TlcConfig {
    /// The .cfg file content
    pub content: String,
    /// The configuration filename
    pub filename: String,
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
    generate_single_with_config(behavior, system_name, &TlaConfig::default())
}

/// Generate TLA+ for a single behavior with configuration options.
fn generate_single_with_config(
    behavior: &BehaviorDecl,
    system_name: &str,
    config: &TlaConfig,
) -> Result<StateMachineTla> {
    let module_name = format!("{}_{}", system_name, behavior.name);
    let mut tla = TlaGenerator::new(&module_name);
    tla.config = config.clone();
    tla.nodes = behavior.nodes.clone();

    // Pre-scan for variables and events
    tla.extract_symbols(behavior);

    tla.generate_header();
    if config.apalache_types {
        tla.generate_apalache_types(&behavior.states);
    }
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
    if config.include_mc_config {
        tla.generate_model_check_config(&behavior.states);
    }
    tla.generate_footer();

    let invariants: Vec<String> = behavior
        .invariants
        .iter()
        .map(|i| format!("Inv_{}", i.name))
        .chain(std::iter::once("TypeOK".to_string()))
        .collect();

    let properties: Vec<String> = behavior
        .properties
        .iter()
        .map(|p| format!("Prop_{}", p.name))
        .collect();

    // Generate TLC .cfg file if requested
    let tlc_cfg = if config.generate_cfg {
        Some(generate_tlc_cfg(
            &module_name,
            &behavior.states,
            &invariants,
            &properties,
        ))
    } else {
        None
    };

    Ok(StateMachineTla {
        content: tla.output,
        module_name,
        invariants,
        properties,
        tlc_cfg,
    })
}

/// Generate a TLC configuration file for model checking.
fn generate_tlc_cfg(
    module_name: &str,
    states: &[StateDecl],
    invariants: &[String],
    properties: &[String],
) -> TlcConfig {
    let mut cfg = String::new();

    cfg.push_str("\\* TLC Configuration File\n");
    cfg.push_str(&format!("\\* Generated for module: {}\n\n", module_name));

    // Specification
    cfg.push_str("SPECIFICATION Spec\n\n");

    // Constants - assign string values to state constants
    cfg.push_str("CONSTANTS\n");
    for s in states {
        cfg.push_str(&format!("    {} = \"{}\"\n", s.name, s.name));
    }
    cfg.push('\n');

    // Invariants
    if !invariants.is_empty() {
        cfg.push_str("INVARIANTS\n");
        for inv in invariants {
            cfg.push_str(&format!("    {}\n", inv));
        }
        cfg.push('\n');
    }

    // Properties
    if !properties.is_empty() {
        cfg.push_str("PROPERTIES\n");
        for prop in properties {
            cfg.push_str(&format!("    {}\n", prop));
        }
        cfg.push('\n');
    }

    // Additional checking options
    cfg.push_str("\\* Checking options\n");
    cfg.push_str("CHECK_DEADLOCK FALSE\n");

    TlcConfig {
        content: cfg,
        filename: format!("{}.cfg", module_name),
    }
}

/// Generate TLA+ with Apalache type annotations for symbolic model checking.
pub fn generate_for_apalache(
    behavior: &BehaviorDecl,
    system_name: &str,
    _project_root: &Path,
) -> Result<StateMachineTla> {
    let config = TlaConfig {
        apalache_types: true,
        include_mc_config: true,
        tlc_compat: false,
        generate_cfg: false,
    };
    generate_single_with_config(behavior, system_name, &config)
}

/// Generate TLA+ with TLC configuration file for model checking.
pub fn generate_with_tlc_config(
    behavior: &BehaviorDecl,
    system_name: &str,
    _project_root: &Path,
) -> Result<StateMachineTla> {
    let config = TlaConfig {
        apalache_types: false,
        include_mc_config: true,
        tlc_compat: true,
        generate_cfg: true,
    };
    generate_single_with_config(behavior, system_name, &config)
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
    /// Generation configuration
    config: TlaConfig,
    /// Optional node set name for distributed systems
    nodes: Option<String>,
}

impl TlaGenerator {
    fn new(module_name: &str) -> Self {
        Self {
            module_name: module_name.to_string(),
            output: String::new(),
            indent: 0,
            extracted_vars: HashSet::new(),
            events: HashSet::new(),
            config: TlaConfig::default(),
            nodes: None,
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
        if self.config.apalache_types {
            let extensions = if self.nodes.is_some() {
                "EXTENDS Naturals, Sequences, Apalache, Variants, FiniteSets"
            } else {
                "EXTENDS Naturals, Sequences, Apalache, Variants"
            };
            self.line(extensions);
        } else {
            let extensions = if self.nodes.is_some() {
                "EXTENDS Naturals, Sequences, TLC, FiniteSets"
            } else {
                "EXTENDS Naturals, Sequences, TLC"
            };
            self.line(extensions);
        }
        self.blank();
    }

    fn generate_footer(&mut self) {
        let dashes = "=".repeat(self.module_name.len() + 20);
        self.line(&dashes);
    }

    /// Generate Apalache type annotations for symbolic model checking.
    fn generate_apalache_types(&mut self, states: &[StateDecl]) {
        self.line("\\* ═══════════════════════════════════════════════════════════════════════════");
        self.line("\\* APALACHE TYPE ANNOTATIONS");
        self.line("\\* ═══════════════════════════════════════════════════════════════════════════");
        self.blank();

        // State type as a variant/enum
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
        self.line("\\* @typeAlias: STATE = Str;");
        self.line(&format!(
            "\\* @typeAlias: STATES = Set(STATE);  \\* {{ {} }}",
            state_names.join(", ")
        ));
        self.blank();

        // Event type
        self.line("\\* @typeAlias: EVENT = [type: Str, args: Seq(Int)];");
        self.line("\\* @typeAlias: EVENT_QUEUE = Seq(EVENT);");
        self.blank();

        // History type
        self.line("\\* @typeAlias: HISTORY = Seq(STATE);");
        self.blank();

        // Variable type annotations
        self.line("\\* @type: STATE;");
        self.line("VARIABLE state");
        self.blank();
        self.line("\\* @type: Int;");
        self.line("VARIABLE pc");
        self.blank();
        self.line("\\* @type: HISTORY;");
        self.line("VARIABLE history");
        self.blank();
        self.line("\\* @type: EVENT_QUEUE;");
        self.line("VARIABLE pending");
        self.blank();

        // Extracted variable types (inferred)
        if !self.extracted_vars.is_empty() {
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for var in &vars {
                let safe_name = self.sanitize_var_name(var);
                let type_hint = self.infer_apalache_type(var);
                self.line(&format!("\\* @type: {};", type_hint));
                self.line(&format!("VARIABLE {}", safe_name));
                self.blank();
            }
        }
    }

    /// Infer Apalache type based on variable name patterns.
    fn infer_apalache_type(&self, var_name: &str) -> &'static str {
        let lower = var_name.to_lowercase();
        if lower.contains("count") || lower.contains("num") || lower.contains("size") || lower.contains("level") {
            "Int"
        } else if lower.contains("enabled") || lower.contains("active") || lower.contains("valid") {
            "Bool"
        } else if lower.contains("list") || lower.contains("queue") || lower.contains("items") {
            "Seq(Int)"
        } else if lower.contains("set") || lower.contains("pool") {
            "Set(Int)"
        } else if lower.contains("id") || lower.contains("name") || lower.contains("address") {
            "Str"
        } else {
            "Int"  // Default to Int for symbolic
        }
    }

    fn generate_constants(&mut self, states: &[StateDecl]) {
        self.line("\\* State constants");
        self.line("CONSTANTS");
        self.indent += 1;
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();

        // Add nodes constant if present
        if let Some(nodes) = &self.nodes {
            self.line(&format!("{},", nodes));
        }

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
        if self.extracted_vars.is_empty() {
            self.line("pending     \\* Pending events/messages queue");
        } else {
            self.line("pending,    \\* Pending events/messages queue");
            // Add extracted vars as actual TLA+ variables
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for (i, var) in vars.iter().enumerate() {
                let safe_name = self.sanitize_var_name(var);
                if i == vars.len() - 1 {
                    self.line(&format!("{}     \\* Data variable (extracted)", safe_name));
                } else {
                    self.line(&format!("{},    \\* Data variable (extracted)", safe_name));
                }
            }
        }
        self.indent -= 1;
        self.blank();

        // Build vars tuple
        if self.extracted_vars.is_empty() {
            self.line("vars == <<state, pc, history, pending>>");
        } else {
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            let sanitized: Vec<String> = vars.iter().map(|v| self.sanitize_var_name(v)).collect();
            self.line(&format!(
                "vars == <<state, pc, history, pending, {}>>",
                sanitized.join(", ")
            ));
        }
        self.blank();
    }

    /// Sanitize a variable name for TLA+ (replace dots with underscores)
    fn sanitize_var_name(&self, name: &str) -> String {
        name.replace('.', "_").replace('-', "_")
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

        // Initialize extracted data variables
        // Use a symbolic "Any" value that can be constrained in model checking
        if !self.extracted_vars.is_empty() {
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for var in &vars {
                let safe_name = self.sanitize_var_name(var);
                // Infer type hint from variable name for better defaults
                let init_value = self.infer_initial_value(var);
                self.line(&format!("/\\ {} = {}", safe_name, init_value));
            }
        }

        self.indent -= 1;
        self.blank();
    }

    /// Infer a reasonable initial value based on variable name patterns.
    fn infer_initial_value(&self, var_name: &str) -> &'static str {
        let lower = var_name.to_lowercase();
        if lower.contains("count") || lower.contains("num") || lower.contains("size") {
            "0"
        } else if lower.contains("enabled") || lower.contains("active") || lower.contains("valid") {
            "FALSE"
        } else if lower.contains("list") || lower.contains("queue") || lower.contains("items") {
            "<<>>"
        } else if lower.contains("set") || lower.contains("pool") {
            "{}"
        } else {
            // Default: use a CHOOSE expression for symbolic value
            "CHOOSE x \\in {} : TRUE"
        }
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
            // Only handle simple state-to-state transitions
            let from_state = match t.from.as_state() {
                Some(s) => s,
                None => continue, // Skip wildcards and multi-state sources
            };
            let to_state = match t.to.as_state() {
                Some(s) => s,
                None => continue, // Skip self and multi-state targets
            };

            let action_name = format!("{}_{}", from_state, t.on_event);
            self.line(&format!("{} ==", action_name));
            self.indent += 1;
            self.line(&format!("/\\ state = {}", from_state));

            if let Some(ref guard) = t.guard {
                self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
            }

            self.line(&format!("/\\ state' = {}", to_state));
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

            // Extract variable modifications from effects
            let var_updates = self.extract_var_updates(&t.effects);

            // Handle data variable updates
            if !self.extracted_vars.is_empty() {
                let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
                vars.sort();

                // Separate modified and unchanged variables
                let mut modified_vars: HashSet<String> = HashSet::new();
                for (var, _) in &var_updates {
                    modified_vars.insert(var.clone());
                }

                // Output explicit updates for modified variables
                for (var, update_expr) in &var_updates {
                    let safe_name = self.sanitize_var_name(var);
                    self.line(&format!("/\\ {}' = {}", safe_name, update_expr));
                }

                // Mark remaining vars as UNCHANGED
                let unchanged: Vec<String> = vars
                    .iter()
                    .filter(|v| !modified_vars.contains(*v))
                    .map(|v| self.sanitize_var_name(v))
                    .collect();

                if !unchanged.is_empty() {
                    self.line(&format!("/\\ UNCHANGED <<{}>>", unchanged.join(", ")));
                }
            }

            // Generate comments for non-emit effects that weren't handled
            for effect in &t.effects {
                if !self.is_handled_effect(effect) {
                    self.generate_effect_comment(effect);
                }
            }

            self.indent -= 1;
            self.blank();
        }
    }

    /// Extract variable updates from effects.
    /// Returns a list of (variable_name, tla_update_expression) pairs.
    fn extract_var_updates(&self, effects: &[crate::parser::ast::EffectStmt]) -> Vec<(String, String)> {
        let mut updates = Vec::new();

        for effect in effects {
            if let EffectKind::Expr(expr) = &effect.kind {
                if let Some((var, update)) = self.parse_var_update(expr) {
                    updates.push((var, update));
                }
            }
        }

        updates
    }

    /// Try to parse a variable update pattern from an expression.
    /// Recognizes patterns like:
    /// - `set(var, value)` -> var' = value
    /// - `increment(var)` -> var' = var + 1
    /// - `decrement(var)` -> var' = var - 1
    /// - `append(list, item)` -> list' = Append(list, item)
    /// - `add(set, elem)` -> set' = set \\union {elem}
    /// - `remove(set, elem)` -> set' = set \\ {elem}
    fn parse_var_update(&self, expr: &Expr) -> Option<(String, String)> {
        match expr {
            Expr::Call { name, args } => {
                let func = name.to_lowercase();
                match func.as_str() {
                    "set" | "assign" if args.len() == 2 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let value = self.expr_to_tla(&args[1]);
                        Some((var, value))
                    }
                    "increment" | "inc" if args.len() >= 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let amount = if args.len() > 1 {
                            self.expr_to_tla(&args[1])
                        } else {
                            "1".to_string()
                        };
                        Some((var, format!("{} + {}", safe, amount)))
                    }
                    "decrement" | "dec" if args.len() >= 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let amount = if args.len() > 1 {
                            self.expr_to_tla(&args[1])
                        } else {
                            "1".to_string()
                        };
                        Some((var, format!("{} - {}", safe, amount)))
                    }
                    "append" | "push" if args.len() == 2 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let item = self.expr_to_tla(&args[1]);
                        Some((var, format!("Append({}, {})", safe, item)))
                    }
                    "prepend" if args.len() == 2 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let item = self.expr_to_tla(&args[1]);
                        Some((var, format!("<<{}>> \\o {}", item, safe)))
                    }
                    "add" | "insert" if args.len() == 2 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let elem = self.expr_to_tla(&args[1]);
                        Some((var, format!("{} \\union {{{}}}", safe, elem)))
                    }
                    "remove" | "delete" if args.len() == 2 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        let elem = self.expr_to_tla(&args[1]);
                        Some((var, format!("{} \\ {{{}}}", safe, elem)))
                    }
                    "clear" if args.len() == 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        // Infer type from var name
                        let lower = var.to_lowercase();
                        let empty = if lower.contains("list") || lower.contains("queue") || lower.contains("seq") {
                            "<<>>"
                        } else {
                            "{}"
                        };
                        Some((var, empty.to_string()))
                    }
                    "toggle" if args.len() == 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        let safe = self.sanitize_var_name(&var);
                        Some((var, format!("~{}", safe)))
                    }
                    "enable" | "activate" if args.len() == 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        Some((var, "TRUE".to_string()))
                    }
                    "disable" | "deactivate" if args.len() == 1 => {
                        let var = self.expr_to_var_name(&args[0])?;
                        Some((var, "FALSE".to_string()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Extract variable name from an expression (for update patterns).
    fn expr_to_var_name(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(name) => Some(name.clone()),
            Expr::DottedName(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// Check if an effect was handled by the update extraction.
    fn is_handled_effect(&self, effect: &crate::parser::ast::EffectStmt) -> bool {
        match &effect.kind {
            EffectKind::Emit { .. } => true, // Handled in pending queue
            EffectKind::Expr(expr) => self.parse_var_update(expr).is_some(),
            EffectKind::If { .. } => false, // Conditional effects not yet handled
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
                .filter_map(|t| {
                    t.from.as_state().map(|from| {
                        format!("{}_{}", from, t.on_event)
                    })
                })
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
            let from_match = t.from.as_state() == Some(f.from.as_str());
            let to_match = t.to.as_state() == Some(f.to.as_str());
            if from_match && to_match {
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
        let abstract_module = refines.replace(".tla", "").replace('/', "_");

        self.line("\\* ═══════════════════════════════════════════════════════════════════════════");
        self.line("\\* REFINEMENT");
        self.line("\\* ═══════════════════════════════════════════════════════════════════════════");
        self.blank();

        self.line(&format!("\\* This behavior refines: {}", refines));
        self.blank();

        // Generate abstraction function if refinement map exists
        if let Some(ref map) = behavior.refinement_map {
            self.line("\\* Abstraction function: maps concrete state to abstract state");
            self.line("Abs ==");
            self.indent += 1;

            let mut first = true;
            for (abstract_state, concrete_states) in &map.mappings {
                if concrete_states.len() == 1 {
                    let prefix = if first { "CASE" } else { "  []" };
                    self.line(&format!(
                        "{} state = {} -> {}",
                        prefix, concrete_states[0], abstract_state
                    ));
                } else {
                    let prefix = if first { "CASE" } else { "  []" };
                    self.line(&format!(
                        "{} state \\in {{{}}} -> {}",
                        prefix,
                        concrete_states.join(", "),
                        abstract_state
                    ));
                }
                first = false;
            }
            self.indent -= 1;
            self.blank();

            // Generate abstract variable definitions
            self.line("\\* Abstract variables (for refinement proof)");
            self.line(&format!("{}State == Abs", abstract_module));
            self.blank();
        }

        // Generate refinement theorem
        self.line("\\* Refinement theorem: this spec implies the abstract spec");
        self.line(&format!("THEOREM RefinementCorrect == Spec => {}!Spec", abstract_module));
        self.blank();

        // Generate instance for refinement checking
        self.line("\\* Instance for refinement checking with TLC/Apalache");
        if behavior.refinement_map.is_some() {
            self.line(&format!(
                "\\* INSTANCE {} WITH state <- Abs",
                abstract_module
            ));
        } else {
            self.line(&format!(
                "\\* INSTANCE {} \\* (requires same state names)",
                abstract_module
            ));
        }
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
            if let Some(from) = t.from.as_state() {
                sources.insert(from);
            }
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
            TemporalExpr::Not(inner) => {
                format!("~({})", self.temporal_to_tla(inner))
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
            TemporalExpr::Count(state_name) => {
                // For distributed systems with nodes, use Cardinality
                // For single-state machines, count is either 0 or 1
                if let Some(nodes) = &self.nodes {
                    format!("Cardinality({{n \\in {} : n.state = {}}})", nodes, state_name)
                } else {
                    format!("IF state = {} THEN 1 ELSE 0", state_name)
                }
            }
            TemporalExpr::Int(n) => {
                n.to_string()
            }
            TemporalExpr::BinOp { lhs, op, rhs } => {
                let op_str = match op {
                    TemporalOp::And => "/\\",
                    TemporalOp::Or => "\\/",
                    TemporalOp::Implies => "=>",
                    TemporalOp::Iff => "<=>",
                    TemporalOp::Lt => "<",
                    TemporalOp::Le => "<=",
                    TemporalOp::Gt => ">",
                    TemporalOp::Ge => ">=",
                    TemporalOp::Eq => "=",
                    TemporalOp::Ne => "/=",
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
                    from: TransitionSource::State("idle".to_string()),
                    to: TransitionTarget::State("active".to_string()),
                    on_event: "start".to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: Span::synthetic(),
                },
                TransitionDecl {
                    from: TransitionSource::State("active".to_string()),
                    to: TransitionTarget::State("done".to_string()),
                    on_event: "finish".to_string(),
                    guard: None,
                    effects: vec![],
                    timing: None,
                    span: Span::synthetic(),
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

    #[test]
    fn test_generate_for_apalache() {
        let behavior = make_test_behavior();
        let result = generate_for_apalache(&behavior, "TestSystem", Path::new(".")).unwrap();

        // Should have Apalache extensions
        assert!(result.content.contains("EXTENDS Naturals, Sequences, Apalache, Variants"));

        // Should have type annotations
        assert!(result.content.contains("@typeAlias: STATE"));
        assert!(result.content.contains("@type: STATE"));
        assert!(result.content.contains("@type: Int"));
        assert!(result.content.contains("@type: HISTORY"));
    }

    #[test]
    fn test_generate_with_tlc_config() {
        let behavior = make_test_behavior();
        let result = generate_with_tlc_config(&behavior, "TestSystem", Path::new(".")).unwrap();

        // Should have tlc_cfg
        assert!(result.tlc_cfg.is_some());
        let cfg = result.tlc_cfg.as_ref().unwrap();

        // Check cfg content
        assert!(cfg.content.contains("SPECIFICATION Spec"));
        assert!(cfg.content.contains("CONSTANTS"));
        assert!(cfg.content.contains("idle = \"idle\""));
        assert!(cfg.content.contains("INVARIANTS"));
        assert!(cfg.content.contains("TypeOK"));
        assert!(cfg.content.contains("PROPERTIES"));
        assert!(cfg.content.contains("Prop_eventually_done"));

        // Check filename
        assert_eq!(cfg.filename, "TestSystem_TestMachine.cfg");
    }

    #[test]
    fn test_parse_var_update_set() {
        let gen = TlaGenerator::new("Test");
        let expr = Expr::Call {
            name: "set".to_string(),
            args: vec![
                Expr::Ident("counter".to_string()),
                Expr::Int(42),
            ],
        };
        let result = gen.parse_var_update(&expr);
        assert!(result.is_some());
        let (var, update) = result.unwrap();
        assert_eq!(var, "counter");
        assert_eq!(update, "42");
    }

    #[test]
    fn test_parse_var_update_increment() {
        let gen = TlaGenerator::new("Test");
        let expr = Expr::Call {
            name: "increment".to_string(),
            args: vec![Expr::Ident("count".to_string())],
        };
        let result = gen.parse_var_update(&expr);
        assert!(result.is_some());
        let (var, update) = result.unwrap();
        assert_eq!(var, "count");
        assert_eq!(update, "count + 1");
    }

    #[test]
    fn test_parse_var_update_append() {
        let gen = TlaGenerator::new("Test");
        let expr = Expr::Call {
            name: "append".to_string(),
            args: vec![
                Expr::Ident("items".to_string()),
                Expr::Int(5),
            ],
        };
        let result = gen.parse_var_update(&expr);
        assert!(result.is_some());
        let (var, update) = result.unwrap();
        assert_eq!(var, "items");
        assert_eq!(update, "Append(items, 5)");
    }

    #[test]
    fn test_parse_var_update_add_to_set() {
        let gen = TlaGenerator::new("Test");
        let expr = Expr::Call {
            name: "add".to_string(),
            args: vec![
                Expr::Ident("members".to_string()),
                Expr::Ident("newMember".to_string()),
            ],
        };
        let result = gen.parse_var_update(&expr);
        assert!(result.is_some());
        let (var, update) = result.unwrap();
        assert_eq!(var, "members");
        assert_eq!(update, "members \\union {newMember}");
    }

    #[test]
    fn test_parse_var_update_enable() {
        let gen = TlaGenerator::new("Test");
        let expr = Expr::Call {
            name: "enable".to_string(),
            args: vec![Expr::Ident("isActive".to_string())],
        };
        let result = gen.parse_var_update(&expr);
        assert!(result.is_some());
        let (var, update) = result.unwrap();
        assert_eq!(var, "isActive");
        assert_eq!(update, "TRUE");
    }

    #[test]
    fn test_refinement_with_map() {
        let mut behavior = make_test_behavior();
        behavior.refines = Some("AbstractSpec".to_string());
        behavior.refinement_map = Some(RefinementMap {
            mappings: vec![
                ("Abstract_idle".to_string(), vec!["idle".to_string()]),
                ("Abstract_active".to_string(), vec!["active".to_string()]),
                ("Abstract_done".to_string(), vec!["done".to_string()]),
            ],
        });

        let result = generate(&behavior, "TestSystem", Path::new(".")).unwrap();

        // Should have refinement section
        assert!(result.content.contains("REFINEMENT"));
        assert!(result.content.contains("Abs =="));
        assert!(result.content.contains("THEOREM RefinementCorrect"));
        assert!(result.content.contains("AbstractSpec!Spec"));
    }

    #[test]
    fn test_temporal_to_tla_count_without_nodes() {
        let gen = TlaGenerator::new("Test");
        let expr = TemporalExpr::Count("leader".to_string());
        assert_eq!(gen.temporal_to_tla(&expr), "IF state = leader THEN 1 ELSE 0");
    }

    #[test]
    fn test_temporal_to_tla_count_with_nodes() {
        let mut gen = TlaGenerator::new("Test");
        gen.nodes = Some("replicas".to_string());
        let expr = TemporalExpr::Count("leader".to_string());
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "Cardinality({n \\in replicas : n.state = leader})"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_comparison() {
        let gen = TlaGenerator::new("Test");
        // count(leader) <= 1
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("leader".to_string())),
            op: TemporalOp::Le,
            rhs: Box::new(TemporalExpr::Int(1)),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(IF state = leader THEN 1 ELSE 0) <= (1)"
        );
    }

    #[test]
    fn test_temporal_to_tla_always_count() {
        let gen = TlaGenerator::new("Test");
        // always(count(leader) <= 1)
        let expr = TemporalExpr::Always(Box::new(TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("leader".to_string())),
            op: TemporalOp::Le,
            rhs: Box::new(TemporalExpr::Int(1)),
        }));
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "[]((IF state = leader THEN 1 ELSE 0) <= (1))"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_with_nodes_comparison() {
        let mut gen = TlaGenerator::new("Test");
        gen.nodes = Some("replicas".to_string());
        // count(leader) <= 1 with nodes
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("leader".to_string())),
            op: TemporalOp::Le,
            rhs: Box::new(TemporalExpr::Int(1)),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(Cardinality({n \\in replicas : n.state = leader})) <= (1)"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_ge() {
        let gen = TlaGenerator::new("Test");
        // count(voted) >= 3
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("voted".to_string())),
            op: TemporalOp::Ge,
            rhs: Box::new(TemporalExpr::Int(3)),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(IF state = voted THEN 1 ELSE 0) >= (3)"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_gt() {
        let gen = TlaGenerator::new("Test");
        // count(leader) > count(follower)
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("leader".to_string())),
            op: TemporalOp::Gt,
            rhs: Box::new(TemporalExpr::Count("follower".to_string())),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(IF state = leader THEN 1 ELSE 0) > (IF state = follower THEN 1 ELSE 0)"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_eq() {
        let gen = TlaGenerator::new("Test");
        // count(completed) == 1
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("completed".to_string())),
            op: TemporalOp::Eq,
            rhs: Box::new(TemporalExpr::Int(1)),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(IF state = completed THEN 1 ELSE 0) = (1)"
        );
    }

    #[test]
    fn test_temporal_to_tla_count_ne() {
        let mut gen = TlaGenerator::new("Test");
        gen.nodes = Some("replicas".to_string());
        // count(failed) != 0
        let expr = TemporalExpr::BinOp {
            lhs: Box::new(TemporalExpr::Count("failed".to_string())),
            op: TemporalOp::Ne,
            rhs: Box::new(TemporalExpr::Int(0)),
        };
        assert_eq!(
            gen.temporal_to_tla(&expr),
            "(Cardinality({n \\in replicas : n.state = failed})) /= (0)"
        );
    }

    #[test]
    fn test_generate_with_nodes() {
        let mut behavior = make_test_behavior();
        behavior.nodes = Some("replicas".to_string());

        let result = generate(&behavior, "TestSystem", Path::new(".")).unwrap();

        // Should have FiniteSets extension
        assert!(result.content.contains("FiniteSets"));
        // Should have replicas constant
        assert!(result.content.contains("replicas"));
    }

    #[test]
    fn test_generate_with_count_property() {
        let mut behavior = make_test_behavior();
        behavior.nodes = Some("replicas".to_string());
        behavior.properties.push(TemporalProperty {
            name: "single_leader".to_string(),
            expr: TemporalExpr::Always(Box::new(TemporalExpr::BinOp {
                lhs: Box::new(TemporalExpr::Count("leader".to_string())),
                op: TemporalOp::Le,
                rhs: Box::new(TemporalExpr::Int(1)),
            })),
        });

        let result = generate(&behavior, "TestSystem", Path::new(".")).unwrap();

        // Should have FiniteSets extension
        assert!(result.content.contains("FiniteSets"));
        // Should have Cardinality in property
        assert!(result.content.contains("Cardinality({n \\in replicas : n.state = leader})"));
        assert!(result.content.contains("Prop_single_leader"));
    }
}
