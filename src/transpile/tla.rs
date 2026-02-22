//! State machine to TLA+ transpiler.
//!
//! Generates TLA+ specifications from Intent behaviors with full LTL support.

use std::path::Path;

use anyhow::Result;

use crate::behavioral::composition::{compose_behaviors, CompositionConfig};
use crate::parser::ast::{
    ArithOp, BehaviorDecl, ComparisonOp, EffectKind, EffectStmt, Expr, FairnessKind, FairnessSpec,
    InvariantDecl, LogicalOp, ParallelBranch, Span, StateDecl, TemporalExpr, TemporalOp,
    TemporalProperty, TransitionDecl, TransitionSource, TransitionTarget, UnaryOp,
};
use std::collections::{HashMap, HashSet};

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
    tla.generate_constants(&behavior.states, &behavior.parameters);
    tla.generate_variables_extended();
    tla.generate_functions(&behavior.functions);
    tla.generate_events();
    tla.generate_module_assumes();
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
    /// Explicit type declarations from VariableDecl (var_name -> type_name)
    explicit_var_types: HashMap<String, String>,
    /// State names (to avoid name collisions)
    state_names: HashSet<String>,
    /// ASSUME statements extracted from invariants (to place at module level)
    module_level_assumes: Vec<String>,
    /// Whether behavior has terminal states
    has_terminal_states: bool,
}

/// Convert a parameter value to a string for TLA+ comments.
fn param_value_to_str(val: &crate::parser::ast::ParamValue) -> String {
    use crate::parser::ast::ParamValue;
    match val {
        ParamValue::Int(n) => n.to_string(),
        ParamValue::Float(f) => f.to_string(),
        ParamValue::String(s) => format!("\"{}\"", s),
        ParamValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        ParamValue::Ident(s) => s.clone(),
        ParamValue::Duration(d) => format!("{}ms", d),
        ParamValue::List(items) => {
            let strs: Vec<String> = items.iter().map(param_value_to_str).collect();
            format!("[{}]", strs.join(", "))
        }
        ParamValue::Map(entries) => {
            let strs: Vec<String> = entries.iter()
                .map(|(k, v)| format!("{}: {}", k, param_value_to_str(v)))
                .collect();
            format!("{{{}}}", strs.join(", "))
        }
    }
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
            explicit_var_types: HashMap::new(),
            state_names: HashSet::new(),
            module_level_assumes: Vec::new(),
            has_terminal_states: false,
        }
    }

    /// Pre-scan behavior to extract all referenced variables and events
    fn extract_symbols(&mut self, behavior: &BehaviorDecl) {
        // Register state names to avoid collisions
        for state in &behavior.states {
            self.state_names.insert(state.name.clone());
            if state.terminal {
                self.has_terminal_states = true;
            }
        }

        // Register explicit variable type declarations
        for var in &behavior.variables {
            self.explicit_var_types.insert(var.name.clone(), var.type_name.clone());
            self.extracted_vars.insert(var.name.clone());
        }

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
            self.extract_assumes_from_expr(&inv.expr);
        }
    }

    fn extract_assumes_from_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Assume(pred) => {
                // Extract the ASSUME statement to module level
                let tla_pred = self.expr_to_tla(pred);
                self.module_level_assumes.push(tla_pred);
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.extract_assumes_from_expr(lhs);
                self.extract_assumes_from_expr(rhs);
            }
            Expr::CompOp { lhs, rhs, .. } => {
                self.extract_assumes_from_expr(lhs);
                self.extract_assumes_from_expr(rhs);
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                self.extract_assumes_from_expr(lhs);
                self.extract_assumes_from_expr(rhs);
            }
            Expr::UnaryOp { expr, .. } => {
                self.extract_assumes_from_expr(expr);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    self.extract_assumes_from_expr(arg);
                }
            }
            _ => {}
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
            EffectKind::Assign { var, value } => {
                self.extracted_vars.insert(var.clone());
                self.collect_vars_from_expr(value);
            }
        }
    }

    fn is_state_name(&self, name: &str) -> bool {
        self.state_names.contains(name)
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
        self.line("\\* AUTO-GENERATED by Intent compiler — do not edit by hand.");
        self.line("\\* Re-generate with: intent compile <source>.intent");
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
        self.line("VARIABLE event_queue");
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

    /// Infer Apalache type for a variable.
    ///
    /// Checks explicit type declarations first, then falls back to heuristics.
    fn infer_apalache_type(&self, var_name: &str) -> String {
        // Check explicit declaration first
        if let Some(type_name) = self.explicit_var_types.get(var_name) {
            return self.type_name_to_apalache(type_name);
        }

        // Fall back to heuristic based on variable name
        let lower = var_name.to_lowercase();
        if lower.contains("count") || lower.contains("num") || lower.contains("size") || lower.contains("level") {
            "Int".to_string()
        } else if lower.contains("enabled") || lower.contains("active") || lower.contains("valid") {
            "Bool".to_string()
        } else if lower.contains("list") || lower.contains("queue") || lower.contains("items") {
            "Seq(Int)".to_string()
        } else if lower.contains("set") || lower.contains("pool") {
            "Set(Int)".to_string()
        } else if lower.contains("id") || lower.contains("name") || lower.contains("address") {
            "Str".to_string()
        } else {
            "Int".to_string()  // Default to Int for symbolic
        }
    }

    /// Convert an Intent type name to an Apalache type.
    fn type_name_to_apalache(&self, type_name: &str) -> String {
        match type_name {
            "Int" | "Integer" => "Int".to_string(),
            "Bool" | "Boolean" => "Bool".to_string(),
            "String" | "Str" => "Str".to_string(),
            "Set" => "Set(Int)".to_string(),
            "List" | "Seq" => "Seq(Int)".to_string(),
            // Handle generic types like "Set<Int>" or "List<String>"
            s if s.starts_with("Set<") => {
                let inner = s.trim_start_matches("Set<").trim_end_matches('>');
                format!("Set({})", self.type_name_to_apalache(inner))
            }
            s if s.starts_with("List<") || s.starts_with("Seq<") => {
                let inner = s
                    .trim_start_matches("List<")
                    .trim_start_matches("Seq<")
                    .trim_end_matches('>');
                format!("Seq({})", self.type_name_to_apalache(inner))
            }
            // Default for unknown types
            _ => "Int".to_string(),
        }
    }

    fn generate_constants(&mut self, states: &[StateDecl], parameters: &[crate::parser::ast::PatternParam]) {
        self.line("\\* State constants");
        self.line("CONSTANTS");
        self.indent += 1;
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();

        // Add nodes constant if present
        if let Some(nodes) = &self.nodes {
            self.line("\\* @type: Set(Str);");
            self.line(&format!("{},", nodes));
        }

        // Add behavior parameters as constants
        if !parameters.is_empty() {
            let param_names: Vec<String> = parameters.iter().map(|p| p.name.clone()).collect();
            for (i, param) in parameters.iter().enumerate() {
                let suffix = if i == parameters.len() - 1 && state_names.is_empty() { "" } else { "," };
                // Add comment with default value if present
                let default_comment = param.constraints.iter()
                    .find_map(|c| match c {
                        crate::parser::ast::FieldConstraint::Default(v) => Some(format!(" \\* default: {}", param_value_to_str(v))),
                        _ => None,
                    })
                    .unwrap_or_default();
                self.line(&format!("{}{}{}", param.name, suffix, default_comment));
            }
        }

        if !state_names.is_empty() {
            // Add type annotations for each state constant
            for (i, state_name) in state_names.iter().enumerate() {
                self.line(&format!("\\* @type: Str;"));
                if i == state_names.len() - 1 {
                    self.line(state_name);
                } else {
                    self.line(&format!("{},", state_name));
                }
            }
        }
        self.indent -= 1;
        self.blank();

        // Add ConstInit for Apalache
        self.line("\\* Initialize constants as distinct model values for Apalache");
        self.line("ConstInit ==");
        self.indent += 1;
        if !state_names.is_empty() {
            for (i, state_name) in state_names.iter().enumerate() {
                let op = if i == 0 { "/\\" } else { "/\\" };
                self.line(&format!("{} {} = \"{}\"", op, state_name, state_name));
            }
        } else {
            self.line("TRUE");
        }
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
        self.line("\\* @type: Str;");
        self.line("state,      \\* Current state");
        self.line("\\* @type: Int;");
        self.line("pc,         \\* Program counter for step tracking");
        self.line("\\* @type: Seq(Str);");
        self.line("history,    \\* Sequence of visited states (for trace analysis)");
        if self.extracted_vars.is_empty() {
            self.line("\\* @type: Seq(Str);");
            self.line("event_queue     \\* Pending events/messages queue");
        } else {
            self.line("\\* @type: Seq(Str);");
            self.line("event_queue,    \\* Pending events/messages queue");
            // Add extracted vars as actual TLA+ variables
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for (i, var) in vars.iter().enumerate() {
                let safe_name = self.sanitize_var_name(var);
                // Add type annotation for Apalache compatibility
                self.line("\\* @type: Int;");
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
            self.line("vars == <<state, pc, history, event_queue>>");
        } else {
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            let sanitized: Vec<String> = vars.iter().map(|v| self.sanitize_var_name(v)).collect();
            self.line(&format!(
                "vars == <<state, pc, history, event_queue, {}>>",
                sanitized.join(", ")
            ));
        }
        self.blank();
    }

    /// Sanitize a variable name for TLA+ (replace dots with underscores)
    fn sanitize_var_name(&self, name: &str) -> String {
        name.replace('.', "_").replace('-', "_")
    }

    fn generate_functions(&mut self, functions: &[crate::parser::ast::FunctionDecl]) {
        if functions.is_empty() {
            return;
        }

        self.line("\\* Function declarations");
        for func in functions {
            let params_str = func.params
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            self.line(&format!("{}({}) ==", func.name, params_str));
            self.indent += 1;

            // Transpile the function body expression to TLA+
            let body_tla = self.expr_to_tla(&func.body);
            self.line(&body_tla);

            self.indent -= 1;
            self.blank();
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

    fn generate_module_assumes(&mut self) {
        if self.module_level_assumes.is_empty() {
            return;
        }

        self.line("\\* Assumptions extracted from invariants (must be at module level)");
        for assume in &self.module_level_assumes.clone() {
            self.line(&format!("ASSUME {}", assume));
        }
        self.blank();
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
        self.line("/\\ event_queue = <<>>");

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
    /// For unknown types, uses a bounded symbolic value rather than empty set.
    fn infer_initial_value(&self, var_name: &str) -> String {
        let lower = var_name.to_lowercase();
        if lower.contains("count") || lower.contains("num") || lower.contains("size") || lower.contains("level") || lower.contains("retry") {
            "0".to_string()
        } else if lower.contains("enabled") || lower.contains("active") || lower.contains("valid") || lower.contains("done") || lower.contains("complete") {
            "FALSE".to_string()
        } else if lower.contains("list") || lower.contains("queue") || lower.contains("items") || lower.contains("seq") {
            "<<>>".to_string()
        } else if lower.contains("set") || lower.contains("pool") || lower.contains("ids") {
            "{}".to_string()
        } else if lower.contains("id") || lower.contains("name") || lower.contains("key") || lower.contains("address") || lower.contains("token") {
            // String-like: use a symbolic constant
            format!("\"{}\"", var_name)
        } else {
            // Default: use 0 as a safe fallback for numeric types
            // This avoids the problematic CHOOSE x \in {} : TRUE
            "0".to_string()
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
            match (&t.from, &t.to) {
                // Simple state-to-state transition
                (TransitionSource::State(from), TransitionTarget::State(to)) => {
                    self.generate_simple_transition(t, from, to);
                }

                // Wildcard source: * -> state
                (TransitionSource::Wildcard, TransitionTarget::State(to)) => {
                    self.generate_wildcard_transition(t, to);
                }

                // Multi-source: [s1, s2] -> state
                (TransitionSource::States(from_states), TransitionTarget::State(to)) => {
                    self.generate_multi_source_transition(t, from_states, to);
                }

                // Self transition: state -> self
                (TransitionSource::State(from), TransitionTarget::Self_) => {
                    self.generate_self_transition(t, from);
                }

                // Multi-target: state -> [s1, s2]
                (TransitionSource::State(from), TransitionTarget::States(to_states)) => {
                    self.generate_multi_target_transition(t, from, to_states);
                }

                // Fork: state -> fork { branch1, branch2 }
                (TransitionSource::State(from), TransitionTarget::Fork { branches }) => {
                    self.generate_fork_transition(t, from, branches);
                }

                // Join: join { s1, s2 } -> target
                (TransitionSource::State(from), TransitionTarget::Join { sync_states, target }) => {
                    self.generate_join_transition(t, from, sync_states, target);
                }

                // Other combinations (less common)
                _ => {
                    self.line(&format!(
                        "\\* TODO: Complex transition {} -> {}",
                        t.from.to_string_repr(),
                        t.to.to_string_repr()
                    ));
                }
            }
        }
    }

    /// Generate a simple state-to-state transition.
    fn generate_simple_transition(&mut self, t: &TransitionDecl, from: &str, to: &str) {
        let action_name = format!("{}_{}", from, t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state = {}", from));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line(&format!("/\\ state' = {}", to));
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a wildcard transition (* -> state).
    fn generate_wildcard_transition(&mut self, t: &TransitionDecl, to: &str) {
        let action_name = format!("Any_{}", t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line("/\\ state \\in States"); // Enabled from any state

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line(&format!("/\\ state' = {}", to));
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a multi-source transition ([s1, s2] -> state).
    fn generate_multi_source_transition(&mut self, t: &TransitionDecl, from_states: &[String], to: &str) {
        let action_name = format!("Multi_{}", t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state \\in {{{}}}", from_states.join(", ")));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line(&format!("/\\ state' = {}", to));
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a self transition (state -> self).
    fn generate_self_transition(&mut self, t: &TransitionDecl, from: &str) {
        let action_name = format!("{}_{}_self", from, t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state = {}", from));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line("/\\ UNCHANGED state"); // State unchanged
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a multi-target transition (state -> [s1, s2]).
    fn generate_multi_target_transition(&mut self, t: &TransitionDecl, from: &str, to_states: &[String]) {
        let action_name = format!("{}_{}_nondet", from, t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state = {}", from));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        // Non-deterministic choice
        self.line(&format!("/\\ state' \\in {{{}}}", to_states.join(", ")));
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a fork transition (state -> fork { branch1, branch2 }).
    fn generate_fork_transition(&mut self, t: &TransitionDecl, from: &str, branches: &[ParallelBranch]) {
        let action_name = format!("{}_{}_fork", from, t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state = {}", from));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line("\\* Fork into parallel branches");
        // For fork, we need to track active branches
        // This is a simplification - a full implementation would need additional variables
        let targets: Vec<&str> = branches.iter()
            .filter(|b| b.condition.is_none()) // Only unconditional branches
            .map(|b| b.target.as_str())
            .collect();

        if targets.len() == 1 {
            self.line(&format!("/\\ state' = {}", targets[0]));
        } else if !targets.is_empty() {
            self.line(&format!("/\\ state' \\in {{{}}}", targets.join(", ")));
        }

        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        // Add comments for conditional branches
        for branch in branches {
            if let Some(ref _cond) = branch.condition {
                self.line(&format!(
                    "\\* Conditional branch to '{}' (condition not shown)",
                    branch.target
                ));
            }
        }

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate a join transition (state -> join { s1, s2 } -> target).
    fn generate_join_transition(
        &mut self,
        t: &TransitionDecl,
        from: &str,
        sync_states: &[String],
        target: &str,
    ) {
        let action_name = format!("{}_{}_join", from, t.on_event);
        self.line(&format!("{} ==", action_name));
        self.indent += 1;
        self.line(&format!("/\\ state = {}", from));

        if let Some(ref guard) = t.guard {
            self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
        }

        self.line("\\* Join synchronization - all branches must complete");
        // A full implementation would check that all sync_states have been visited
        // For now, we add a comment indicating the synchronization requirement
        self.line(&format!(
            "\\* Requires completion of: {}",
            sync_states.join(", ")
        ));

        self.line(&format!("/\\ state' = {}", target));
        self.line("/\\ pc' = pc + 1");
        self.line("/\\ history' = Append(history, state)");

        self.generate_pending_and_effects(&t.effects);
        self.indent -= 1;
        self.blank();
    }

    /// Generate pending queue update and data variable effects.
    fn generate_pending_and_effects(&mut self, effects: &[EffectStmt]) {
        // Handle effects - emit events go to pending queue
        let emits: Vec<_> = effects
            .iter()
            .filter_map(|e| match &e.kind {
                EffectKind::Emit { name, args } => Some((name, args)),
                _ => None,
            })
            .collect();

        if emits.is_empty() {
            self.line("/\\ event_queue' = event_queue");
        } else {
            // Build a sequence of emitted events
            let emit_strs: Vec<String> = emits
                .iter()
                .map(|(name, args)| {
                    let args_str: Vec<String> = args.iter().map(|a| self.expr_to_tla(a)).collect();
                    if args_str.is_empty() {
                        format!("[type |-> \"{}\"]", name)
                    } else {
                        format!(
                            "[type |-> \"{}\", args |-> <<{}>>]",
                            name,
                            args_str.join(", ")
                        )
                    }
                })
                .collect();
            self.line(&format!(
                "/\\ event_queue' = event_queue \\o <<{}>>",
                emit_strs.join(", ")
            ));
        }

        // Extract variable modifications from effects
        let var_updates = self.extract_var_updates(effects);

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
        for effect in effects {
            if !self.is_handled_effect(effect) {
                self.generate_effect_comment(effect);
            }
        }
    }

    /// Extract variable updates from effects.
    /// Returns a list of (variable_name, tla_update_expression) pairs.
    fn extract_var_updates(&self, effects: &[crate::parser::ast::EffectStmt]) -> Vec<(String, String)> {
        let mut updates = Vec::new();

        for effect in effects {
            match &effect.kind {
                EffectKind::Expr(expr) => {
                    if let Some((var, update)) = self.parse_var_update(expr) {
                        updates.push((var, update));
                    }
                }
                EffectKind::Assign { var, value } => {
                    let safe_var = self.sanitize_var_name(var);
                    updates.push((safe_var, self.expr_to_tla(value)));
                }
                EffectKind::If { cond, then_effects, else_effects } => {
                    // Generate conditional updates using IF-THEN-ELSE
                    let then_updates = self.extract_var_updates(then_effects);
                    let else_updates = else_effects.as_ref()
                        .map(|effs| self.extract_var_updates(effs))
                        .unwrap_or_default();
                    
                    // For each variable updated in then branch
                    for (var, then_val) in &then_updates {
                        let else_val = else_updates.iter()
                            .find(|(v, _)| v == var)
                            .map(|(_, val)| val.clone())
                            .unwrap_or_else(|| self.sanitize_var_name(var)); // Keep unchanged if no else
                        let cond_tla = self.expr_to_tla(cond);
                        updates.push((var.clone(), format!("IF {} THEN {} ELSE {}", cond_tla, then_val, else_val)));
                    }
                    
                    // Variables only updated in else branch
                    for (var, else_val) in &else_updates {
                        if !then_updates.iter().any(|(v, _)| v == var) {
                            let cond_tla = self.expr_to_tla(cond);
                            let then_val = self.sanitize_var_name(var); // Keep unchanged
                            updates.push((var.clone(), format!("IF {} THEN {} ELSE {}", cond_tla, then_val, else_val)));
                        }
                    }
                }
                EffectKind::Emit { .. } => {
                    // Handled separately in pending queue
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
            EffectKind::Assign { .. } => true, // Variable assignments are handled
            EffectKind::If { .. } => true, // Conditional effects now handled via IF-THEN-ELSE
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
            EffectKind::Assign { var, value } => {
                self.line(&format!("\\* ASSIGN: {} = {}", var, self.expr_to_tla(value)));
            }
        }
    }

    fn generate_next(&mut self, transitions: &[TransitionDecl]) {
        self.line("Next ==");
        self.indent += 1;

        if transitions.is_empty() {
            self.line("UNCHANGED vars");
        } else {
            let mut actions: Vec<String> = Vec::new();

            for t in transitions {
                let action = match (&t.from, &t.to) {
                    (TransitionSource::State(from), TransitionTarget::State(_)) => {
                        format!("{}_{}", from, t.on_event)
                    }
                    (TransitionSource::Wildcard, _) => {
                        format!("Any_{}", t.on_event)
                    }
                    (TransitionSource::States(_), _) => {
                        format!("Multi_{}", t.on_event)
                    }
                    (TransitionSource::State(from), TransitionTarget::Self_) => {
                        format!("{}_{}_self", from, t.on_event)
                    }
                    (TransitionSource::State(from), TransitionTarget::States(_)) => {
                        format!("{}_{}_nondet", from, t.on_event)
                    }
                    (TransitionSource::State(from), TransitionTarget::Fork { .. }) => {
                        format!("{}_{}_fork", from, t.on_event)
                    }
                    (TransitionSource::State(from), TransitionTarget::Join { .. }) => {
                        format!("{}_{}_join", from, t.on_event)
                    }
                    _ => {
                        // Unknown combination, generate a generic name
                        format!("{}_{}", t.from.to_string_repr(), t.on_event)
                    }
                };
                actions.push(action);
            }

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
        self.line("\\* history: checked via HistoryConsistent (Seq(States) unsupported by Apalache)");
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
        if self.has_terminal_states {
            self.line("[](state \\notin TerminalStates => ENABLED(Next))");
        } else {
            self.line("[](ENABLED(Next))");
        }
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

    fn temporal_to_tla_action(&self, expr: &TemporalExpr) -> String {
        // Converts temporal expressions with Next into action predicates
        // This strips away Next operators and returns expressions with primed variables
        match expr {
            TemporalExpr::Next(inner) => {
                // Recursively convert, which will handle State -> state'
                self.temporal_to_tla(expr)
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
                    self.temporal_to_tla_action(lhs),
                    op_str,
                    self.temporal_to_tla_action(rhs)
                )
            }
            TemporalExpr::Not(inner) => {
                format!("~({})", self.temporal_to_tla_action(inner))
            }
            TemporalExpr::State(name) => {
                // In action context without Next, this is current state
                format!("state = {}", name)
            }
            _ => {
                // For other cases, fall back to regular temporal conversion
                self.temporal_to_tla(expr)
            }
        }
    }

    fn contains_next(expr: &TemporalExpr) -> bool {
        match expr {
            TemporalExpr::Next(_) => true,
            TemporalExpr::Always(inner) | TemporalExpr::Eventually(inner) | TemporalExpr::Not(inner) => {
                Self::contains_next(inner)
            }
            TemporalExpr::Until { lhs, rhs }
            | TemporalExpr::Release { lhs, rhs }
            | TemporalExpr::WeakUntil { lhs, rhs }
            | TemporalExpr::StrongRelease { lhs, rhs }
            | TemporalExpr::AlwaysImplies { premise: lhs, conclusion: rhs } => {
                Self::contains_next(lhs) || Self::contains_next(rhs)
            }
            TemporalExpr::BinOp { lhs, rhs, .. } => {
                Self::contains_next(lhs) || Self::contains_next(rhs)
            }
            TemporalExpr::State(_) | TemporalExpr::Count(_) | TemporalExpr::Int(_) => false,
        }
    }

    fn temporal_to_tla(&self, expr: &TemporalExpr) -> String {
        match expr {
            TemporalExpr::Always(inner) => {
                // Check if inner contains Next - must use action form []A_vars
                if Self::contains_next(inner) {
                    // Convert to action form: [][(inner)]_vars
                    let action_expr = self.temporal_to_tla_action(inner);
                    return format!("[][{}]_vars", action_expr)
                }
                format!("[]({})", self.temporal_to_tla(inner))
            }
            TemporalExpr::Eventually(inner) => {
                format!("<>({})", self.temporal_to_tla(inner))
            }
            TemporalExpr::Next(inner) => {
                match &**inner {
                    // For BinOp with Or/And, distribute Next over both operands
                    TemporalExpr::BinOp { lhs, op, rhs } if matches!(op, TemporalOp::Or | TemporalOp::And) => {
                        let op_str = match op {
                            TemporalOp::And => "/\\",
                            TemporalOp::Or => "\\/",
                            _ => unreachable!(),
                        };
                        format!(
                            "({}) {} ({})",
                            self.temporal_to_tla(&TemporalExpr::Next(lhs.clone())),
                            op_str,
                            self.temporal_to_tla(&TemporalExpr::Next(rhs.clone()))
                        )
                    }
                    // For State, generate state' = statename
                    TemporalExpr::State(name) => {
                        format!("state' = {}", name)
                    }
                    // For Not, distribute Next inside
                    TemporalExpr::Not(inner_not) => {
                        format!("~({})", self.temporal_to_tla(&TemporalExpr::Next(inner_not.clone())))
                    }
                    // For other cases, keep existing behavior (may need refinement)
                    _ => {
                        let inner_tla = self.temporal_to_tla(inner);
                        format!("({})'", inner_tla)
                    }
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
            Expr::Count(state) => {
                format!("Cardinality({}_nodes)", state)
            }
            Expr::Choose { var, domain, predicate } => {
                format!("CHOOSE {} \\in {} : {}", var, self.expr_to_tla(domain), self.expr_to_tla(predicate))
            }
            Expr::Let { bindings, body } => {
                let binds: Vec<String> = bindings.iter()
                    .map(|(name, expr)| format!("{} == {}", name, self.expr_to_tla(expr)))
                    .collect();
                format!("LET {} IN {}", binds.join(" "), self.expr_to_tla(body))
            }
            Expr::IfThenElse { cond, then_expr, else_expr } => {
                format!("IF {} THEN {} ELSE {}", self.expr_to_tla(cond), self.expr_to_tla(then_expr), self.expr_to_tla(else_expr))
            }
            Expr::Case { arms, default } => {
                let arms_str: Vec<String> = arms.iter()
                    .map(|(cond, val)| format!("{} -> {}", self.expr_to_tla(cond), self.expr_to_tla(val)))
                    .collect();
                let default_str = default.as_ref()
                    .map(|d| format!(" [] OTHER -> {}", self.expr_to_tla(d)))
                    .unwrap_or_default();
                format!("CASE {}{}", arms_str.join(" [] "), default_str)
            }
            Expr::Subset(s) => format!("SUBSET {}", self.expr_to_tla(s)),
            Expr::BigUnion(s) => format!("UNION {}", self.expr_to_tla(s)),
            Expr::Domain(f) => format!("DOMAIN {}", self.expr_to_tla(f)),
            Expr::Except { base, updates } => {
                let upds: Vec<String> = updates.iter()
                    .map(|(path, val)| {
                        let path_str: Vec<String> = path.iter().map(|e| format!("[{}]", self.expr_to_tla(e))).collect();
                        format!("!{} = {}", path_str.join(""), self.expr_to_tla(val))
                    })
                    .collect();
                format!("[{} EXCEPT {}]", self.expr_to_tla(base), upds.join(", "))
            }
            Expr::FunctionLiteral { var, domain, body } => {
                format!("[{} \\in {} |-> {}]", var, self.expr_to_tla(domain), self.expr_to_tla(body))
            }
            Expr::Record(fields) => {
                let fields_str: Vec<String> = fields.iter()
                    .map(|(name, val)| format!("{} |-> {}", name, self.expr_to_tla(val)))
                    .collect();
                format!("[{}]", fields_str.join(", "))
            }
            Expr::FieldAccess { record, field } => {
                format!("{}.{}", self.expr_to_tla(record), field)
            }
            Expr::Tuple(elems) => {
                let elems_str: Vec<String> = elems.iter().map(|e| self.expr_to_tla(e)).collect();
                format!("<<{}>>", elems_str.join(", "))
            }
            Expr::SetLiteral(elems) => {
                let elems_str: Vec<String> = elems.iter().map(|e| self.expr_to_tla(e)).collect();
                format!("{{{}}}", elems_str.join(", "))
            }
            Expr::Index { base, index } => {
                format!("{}[{}]", self.expr_to_tla(base), self.expr_to_tla(index))
            }
            Expr::SetDiff { lhs, rhs } => {
                format!("({} \\ {})", self.expr_to_tla(lhs), self.expr_to_tla(rhs))
            }
            Expr::SetUnion { lhs, rhs } => {
                format!("({} \\union {})", self.expr_to_tla(lhs), self.expr_to_tla(rhs))
            }
            Expr::SetIntersect { lhs, rhs } => {
                format!("({} \\intersect {})", self.expr_to_tla(lhs), self.expr_to_tla(rhs))
            }
            Expr::In { element, set } => {
                format!("({} \\in {})", self.expr_to_tla(element), self.expr_to_tla(set))
            }
            Expr::Forall { var, domain, body } => {
                format!("\\A {} \\in {} : {}", var, self.expr_to_tla(domain), self.expr_to_tla(body))
            }
            Expr::Exists { var, domain, body } => {
                format!("\\E {} \\in {} : {}", var, self.expr_to_tla(domain), self.expr_to_tla(body))
            }
            Expr::Assume(pred) => {
                // ASSUME statements are extracted to module level, so just return TRUE here
                // The actual assumption is enforced at module level
                "TRUE".to_string()
            }
            Expr::TlaInline { code } => {
                // Return the inline TLA+ code as-is
                code.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::*;

    fn make_test_state(name: &str, initial: bool, terminal: bool) -> StateDecl {
        StateDecl {
            name: name.to_string(),
            initial,
            terminal,
            parent: None,
            substates: Vec::new(),
            entry_actions: Vec::new(),
            exit_actions: Vec::new(),
        }
    }

    fn make_test_behavior() -> BehaviorDecl {
        BehaviorDecl {
            name: "TestMachine".to_string(),
            states: vec![
                make_test_state("idle", true, false),
                make_test_state("active", false, false),
                make_test_state("done", false, true),
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
        assert_eq!(gen.temporal_to_tla(&expr), "state' = active");
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
