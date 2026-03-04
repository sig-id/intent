//! State machine to TLA+ transpiler.
//!
//! Generates TLA+ specifications from Intent behaviors with full LTL support.

use std::path::Path;

use anyhow::Result;

use crate::behavioral::composition::{compose_behaviors, CompositionConfig};
use crate::diagnostic::{Diagnostic, ErrorCode};
use crate::parser::ast::{
    ArithOp, BehaviorDecl, ComparisonOp, EffectKind, EffectStmt, Expr, FairnessKind, FairnessSpec,
    InvariantDecl, LogicalOp, ParallelBranch, Span, StateDecl, TemporalExpr, TemporalOp,
    TemporalProperty, TransitionDecl, TransitionSource, TransitionTarget, UnaryOp, ValueBounds,
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
    /// Diagnostics emitted during generation (e.g. heuristic inference warnings).
    pub diagnostics: Vec<Diagnostic>,
}

/// Configuration options for TLA+ generation.
#[derive(Debug, Clone)]
pub struct TlaConfig {
    /// Generate Apalache-compatible type annotations
    pub apalache_types: bool,
    /// Include model checking configuration block
    pub include_mc_config: bool,
    /// Generate TLC-specific operators
    pub tlc_compat: bool,
    /// Generate TLC .cfg file content (returned separately)
    pub generate_cfg: bool,
    /// Maximum queue size for message channels in composed systems
    pub max_queue_size: usize,
}

impl Default for TlaConfig {
    fn default() -> Self {
        Self {
            apalache_types: false,
            include_mc_config: false,
            tlc_compat: false,
            generate_cfg: false,
            max_queue_size: 10,
        }
    }
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
    behaviors: Option<&HashMap<String, &BehaviorDecl>>,
) -> Result<StateMachineTla> {
    // Check if this behavior composes others
    if !behavior.composes.is_empty() {
        let registry = behaviors.ok_or_else(|| {
            anyhow::anyhow!(
                "behavior '{}' composes [{}] but no behavior registry was provided",
                behavior.name,
                behavior.composes.join(", ")
            )
        })?;

        let mut source_behaviors = Vec::new();
        let mut missing = Vec::new();

        for name in &behavior.composes {
            if let Some(source) = registry.get(name.as_str()) {
                source_behaviors.push((name.as_str(), *source));
            } else {
                missing.push(name.as_str());
            }
        }

        if !missing.is_empty() {
            anyhow::bail!(
                "behavior '{}' composes unknown behaviors: [{}]",
                behavior.name,
                missing.join(", ")
            );
        }

        return generate_composed(behavior, &source_behaviors, system_name, None);
    }

    generate_single(behavior, system_name)
}

/// Generate TLA+ for a single behavior (no composition).
fn generate_single(behavior: &BehaviorDecl, system_name: &str) -> Result<StateMachineTla> {
    generate_single_with_config(behavior, system_name, &TlaConfig::default())
}

/// Generate TLA+ for a single behavior with configuration options.
pub fn generate_single_with_config(
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
    tla.generate_cinit();
    tla.generate_init_extended(&behavior.states);
    tla.compute_action_names(&behavior.transitions);
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

    let has_terminals = behavior.states.iter().any(|s| s.terminal);
    let invariants: Vec<String> = behavior
        .invariants
        .iter()
        .map(|i| format!("Inv_{}", i.name))
        .chain(std::iter::once("TypeOK".to_string()))
        .chain(if has_terminals { Some("NotTerminated".to_string()) } else { None })
        .collect();

    let properties: Vec<String> = behavior
        .properties
        .iter()
        .filter(|p| !(config.apalache_types && TlaGenerator::contains_until(&p.expr)))
        .map(|p| format!("Prop_{}", p.name.replace('<', "_").replace('>', "")))
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
        diagnostics: tla.diagnostics,
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
        max_queue_size: 10,
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
        max_queue_size: 10,
    };
    generate_single_with_config(behavior, system_name, &config)
}

/// Check if a behavior uses Send/Receive effects.
fn behavior_has_send_receive(behavior: &BehaviorDecl) -> bool {
    for transition in &behavior.transitions {
        for effect in &transition.effects {
            if matches!(effect.kind, EffectKind::Send { .. } | EffectKind::Receive { .. }) {
                return true;
            }
            // Check nested effects
            if let EffectKind::If { then_effects, else_effects, .. } = &effect.kind {
                for eff in then_effects {
                    if matches!(eff.kind, EffectKind::Send { .. } | EffectKind::Receive { .. }) {
                        return true;
                    }
                }
                if let Some(else_branch) = else_effects {
                    for eff in else_branch {
                        if matches!(eff.kind, EffectKind::Send { .. } | EffectKind::Receive { .. }) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
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
    // Check if behaviors use message passing
    let has_message_passing = source_behaviors.iter()
        .any(|(_, b)| behavior_has_send_receive(b));

    if has_message_passing {
        // Use parallel composition generator
        return generate_parallel_composed(behavior, source_behaviors, system_name);
    }

    // Fallback to sequential merge (existing code)
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

/// Generate TLA+ for parallel composition with message passing.
fn generate_parallel_composed(
    behavior: &BehaviorDecl,
    source_behaviors: &[(&str, &BehaviorDecl)],
    system_name: &str,
) -> Result<StateMachineTla> {
    let module_name = format!("{}_{}", system_name, behavior.name);
    let mut generator = ComposedTlaGenerator::new(&module_name, TlaConfig::default());

    // Add all source behaviors
    for (name, behavior_decl) in source_behaviors {
        generator.add_behavior(name, behavior_decl);
    }

    // Generate the TLA+ module
    let content = generator.generate();

    // Collect invariants to report back
    let mut invariants = vec!["TypeOK".to_string(), "HistoryConsistent".to_string()];

    // Add terminal stability and NotTerminated invariants
    for (name, behavior_decl) in source_behaviors {
        let has_terminals = behavior_decl.states.iter().any(|s| s.terminal);
        if has_terminals {
            invariants.push(format!("{}_TerminalStable", name));
            invariants.push(format!("{}_NotTerminated", name));
        }
    }

    // Add user-defined invariants
    for (name, behavior_decl) in source_behaviors {
        for inv in &behavior_decl.invariants {
            invariants.push(format!("{}_Inv_{}", name, inv.name));
        }
    }

    Ok(StateMachineTla {
        content,
        module_name,
        invariants,
        properties: vec![],
        tlc_cfg: None,
        diagnostics: vec![],
    })
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
    /// Variable bounds from VariableDecl (var_name -> ValueBounds)
    variable_bounds: HashMap<String, ValueBounds>,
    /// Message channels (channel_name -> set of message types)
    message_channels: HashMap<String, HashSet<String>>,
    /// State names (to avoid name collisions)
    state_names: HashSet<String>,
    /// ASSUME statements extracted from invariants (to place at module level)
    module_level_assumes: Vec<String>,
    /// Whether behavior has terminal states
    has_terminal_states: bool,
    /// Resolved Apalache types per arg position for each channel
    channel_arg_types: HashMap<String, Vec<String>>,
    /// Diagnostics collected during generation (heuristic inference warnings, etc.)
    diagnostics: Vec<Diagnostic>,
    /// Pre-computed action names per transition index (disambiguated for collisions).
    action_names: Vec<String>,
    /// Functions called in guards/effects that are not TLA+ built-ins.
    /// Maps function name → arity (max arg count seen).
    /// In Apalache mode we emit stubs for these so the module parses.
    called_functions: HashMap<String, usize>,
    /// Functions that are used in a Boolean context (as a bare guard conjunct).
    /// These stubs must return Bool (emitted as `== TRUE`); value-context stubs use identity.
    bool_functions: HashSet<String>,
}

/// Context for a single behavior in parallel composition.
struct BehaviorContext<'a> {
    name: String,
    behavior: &'a BehaviorDecl,
    extracted_vars: HashSet<String>,
    initial_state: String,
    /// Message channels (channel_name -> set of message types)
    message_channels: HashMap<String, HashSet<String>>,
    /// Variable bounds
    variable_bounds: HashMap<String, ValueBounds>,
    /// Explicit variable types
    explicit_var_types: HashMap<String, String>,
}

/// Generator for parallel composed behaviors with message passing.
struct ComposedTlaGenerator<'a> {
    module_name: String,
    output: String,
    indent: usize,
    behaviors: Vec<BehaviorContext<'a>>,
    shared_message_channels: HashMap<String, HashSet<String>>,
    /// Resolved Apalache types per arg position for each channel (channel -> [type_at_pos0, type_at_pos1, ...])
    channel_arg_types: HashMap<String, Vec<String>>,
    config: TlaConfig,
}

impl<'a> ComposedTlaGenerator<'a> {
    fn new(module_name: &str, config: TlaConfig) -> Self {
        Self {
            module_name: module_name.to_string(),
            output: String::new(),
            indent: 0,
            behaviors: Vec::new(),
            shared_message_channels: HashMap::new(),
            channel_arg_types: HashMap::new(),
            config,
        }
    }

    fn emit(&mut self, s: &str) {
        let indent_str = "    ".repeat(self.indent);
        self.output.push_str(&format!("{}{}\n", indent_str, s));
    }

    fn emit_blank(&mut self) {
        self.output.push('\n');
    }

    /// Add a behavior to the composition.
    fn add_behavior(&mut self, name: &str, behavior: &'a BehaviorDecl) {
        let mut context = BehaviorContext {
            name: name.to_string(),
            behavior,
            extracted_vars: HashSet::new(),
            initial_state: String::new(),
            message_channels: HashMap::new(),
            variable_bounds: HashMap::new(),
            explicit_var_types: HashMap::new(),
        };

        // Extract variables from this behavior
        self.extract_behavior_symbols(&mut context);

        // Find initial state
        for state in &behavior.states {
            if state.initial {
                context.initial_state = state.name.clone();
                break;
            }
        }

        self.behaviors.push(context);
    }

    /// Extract variables and message channels from a behavior.
    fn extract_behavior_symbols(&mut self, context: &mut BehaviorContext<'a>) {
        // Copy the behavior reference so we can read from it while mutating other context fields
        let behavior = context.behavior;

        // Extract from variables
        for var in &behavior.variables {
            context.extracted_vars.insert(var.name.clone());
            context.explicit_var_types.insert(var.name.clone(), var.type_name.clone());

            if let Some(ref bounds) = var.bounds {
                context.variable_bounds.insert(var.name.clone(), bounds.clone());
            }
        }

        // Extract from transitions (no clone needed: behavior ref is independent of context fields)
        for transition in &behavior.transitions {
            // Extract from guard
            if let Some(ref guard) = transition.guard {
                self.extract_vars_from_expr(guard, &mut context.extracted_vars);
            }

            // Extract from effects
            for effect in &transition.effects {
                self.extract_vars_from_effect(&effect.kind, context);
            }
        }
    }

    /// Extract variables from an expression.
    fn extract_vars_from_expr(&self, expr: &Expr, vars: &mut HashSet<String>) {
        match expr {
            Expr::Ident(name) => {
                vars.insert(name.clone());
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.extract_vars_from_expr(lhs, vars);
                self.extract_vars_from_expr(rhs, vars);
            }
            Expr::CompOp { lhs, rhs, .. } => {
                self.extract_vars_from_expr(lhs, vars);
                self.extract_vars_from_expr(rhs, vars);
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                self.extract_vars_from_expr(lhs, vars);
                self.extract_vars_from_expr(rhs, vars);
            }
            Expr::UnaryOp { expr, .. } => {
                self.extract_vars_from_expr(expr, vars);
            }
            Expr::FieldAccess { record, .. } => {
                self.extract_vars_from_expr(record, vars);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    self.extract_vars_from_expr(arg, vars);
                }
            }
            Expr::Record(fields) => {
                for (_, val) in fields {
                    self.extract_vars_from_expr(val, vars);
                }
            }
            Expr::SetLiteral(items) | Expr::Tuple(items) => {
                for item in items {
                    self.extract_vars_from_expr(item, vars);
                }
            }
            Expr::IfThenElse { cond, then_expr, else_expr } => {
                self.extract_vars_from_expr(cond, vars);
                self.extract_vars_from_expr(then_expr, vars);
                self.extract_vars_from_expr(else_expr, vars);
            }
            Expr::Index { base, index } => {
                self.extract_vars_from_expr(base, vars);
                self.extract_vars_from_expr(index, vars);
            }
            _ => {}
        }
    }

    /// Extract variables and message channels from an effect.
    fn extract_vars_from_effect(&self, effect: &EffectKind, context: &mut BehaviorContext<'a>) {
        match effect {
            EffectKind::Send { channel, message, args } => {
                context.message_channels
                    .entry(channel.clone())
                    .or_default()
                    .insert(message.clone());
                for arg in args {
                    self.extract_vars_from_expr(arg, &mut context.extracted_vars);
                }
            }
            EffectKind::Receive { channel, message, filter } => {
                context.message_channels
                    .entry(channel.clone())
                    .or_default()
                    .insert(message.clone());
                if let Some(filter_expr) = filter {
                    self.extract_vars_from_expr(filter_expr, &mut context.extracted_vars);
                }
            }
            EffectKind::Assign { value, .. } => {
                self.extract_vars_from_expr(value, &mut context.extracted_vars);
            }
            EffectKind::Expr(e) => {
                self.extract_vars_from_expr(e, &mut context.extracted_vars);
            }
            EffectKind::If { cond, then_effects, else_effects } => {
                self.extract_vars_from_expr(cond, &mut context.extracted_vars);
                for eff in then_effects {
                    self.extract_vars_from_effect(&eff.kind, context);
                }
                if let Some(else_branch) = else_effects {
                    for eff in else_branch {
                        self.extract_vars_from_effect(&eff.kind, context);
                    }
                }
            }
            _ => {}
        }
    }

    /// Collect shared message channels across all behaviors.
    fn collect_shared_channels(&mut self) {
        for context in &self.behaviors {
            for (channel, messages) in &context.message_channels {
                self.shared_message_channels
                    .entry(channel.clone())
                    .or_default()
                    .extend(messages.clone());
            }
        }
        // Compute arg types per channel by scanning all Send effects
        self.compute_channel_arg_types();
    }

    /// Infer the Apalache type string from an expression.
    fn infer_expr_type(expr: &Expr) -> &'static str {
        match expr {
            Expr::Int(_) => "Int",
            Expr::Bool(_) => "Bool",
            Expr::String(_) => "Str",
            _ => "Int",
        }
    }

    /// Scan all Send effects to determine the actual arg types for each channel.
    /// Builds a flattened record type with all fields across all message types.
    fn compute_channel_arg_types(&mut self) {
        // channel -> (max_args, per-position types)
        let mut channel_info: HashMap<String, (usize, HashMap<usize, HashSet<&'static str>>)> = HashMap::new();

        for context in &self.behaviors {
            for transition in &context.behavior.transitions {
                for effect in &transition.effects {
                    if let EffectKind::Send { channel, args, .. } = &effect.kind {
                        let entry = channel_info.entry(channel.clone()).or_insert_with(|| (0, HashMap::new()));
                        if args.len() > entry.0 {
                            entry.0 = args.len();
                        }
                        for (i, arg) in args.iter().enumerate() {
                            entry.1.entry(i).or_default().insert(Self::infer_expr_type(arg));
                        }
                    }
                }
            }
        }

        // Resolve types per position: if only one type seen, use it;
        // if mixed Int/Bool, use Int (booleans are encoded as 0/1 in the output).
        for (channel, (max_args, pos_types)) in &channel_info {
            let mut resolved = Vec::with_capacity(*max_args);
            for i in 0..*max_args {
                let types = pos_types.get(&i);
                let resolved_type = match types {
                    Some(ts) if ts.len() == 1 => ts.iter().next().unwrap().to_string(),
                    Some(ts) if ts.contains("Str") => "Str".to_string(),
                    Some(_) => "Int".to_string(), // Int/Bool mix → Int
                    None => "Int".to_string(),
                };
                resolved.push(resolved_type);
            }
            self.channel_arg_types.insert(channel.clone(), resolved);
        }
    }

    /// Generate TLA+ module header.
    fn generate_header(&mut self) {
        self.emit(&format!("---- MODULE {} ----", self.module_name));
        self.emit("EXTENDS Naturals, Sequences, FiniteSets, Apalache, TLC");
        self.emit_blank();
    }

    /// Generate VARIABLES declaration with namespaced variables.
    fn generate_composed_variables(&mut self) {
        // Note: We don't emit type alias for messages because Apalache
        // will infer the union type from usage
        // self.emit("\\* @typeAlias: MSG = [type: Str];");
        // self.emit_blank();

        self.emit("VARIABLES");
        self.indent += 1;

        let mut all_vars = Vec::new();
        let mut var_types: Vec<String> = Vec::new();

        // Add per-behavior variables
        for context in &self.behaviors {
            // State variable
            all_vars.push(format!("{}_state", context.name));
            var_types.push("Str".to_string());

            // PC (program counter for history tracking)
            all_vars.push(format!("{}_pc", context.name));
            var_types.push("Int".to_string());

            // History
            all_vars.push(format!("{}_history", context.name));
            var_types.push("Seq(Str)".to_string());

            // Extracted variables from behavior
            for var in &context.extracted_vars {
                all_vars.push(format!("{}_{}", context.name, var));
                // Get type from context, default to Int
                let var_type = context.explicit_var_types.get(var)
                    .map(|t| match t.as_str() {
                        "Int" => "Int",
                        "String" => "Str",
                        "Bool" => "Bool",
                        _ => "Int",
                    })
                    .unwrap_or("Int");
                var_types.push(var_type.to_string());
            }
        }

        // Add shared message queue variables with precise record types
        let channels_sorted: Vec<_> = self.shared_message_channels.keys().cloned().collect();
        for channel in &channels_sorted {
            all_vars.push(format!("{}_queue", channel));
            // Build precise record type from computed arg types
            let mut fields = vec!["type: Str".to_string()];
            if let Some(arg_types) = self.channel_arg_types.get(channel) {
                for (i, atype) in arg_types.iter().enumerate() {
                    fields.push(format!("arg{}: {}", i, atype));
                }
            }
            var_types.push(format!("Seq({{ {} }})", fields.join(", ")));
        }

        // Emit variables with type annotations
        for (i, (var, typ)) in all_vars.iter().zip(var_types.iter()).enumerate() {
            self.emit(&format!("\\* @type: {};", typ));
            if i == all_vars.len() - 1 {
                self.emit(var);
            } else {
                self.emit(&format!("{},", var));
            }
        }

        self.indent -= 1;
        self.emit_blank();

        // Generate vars tuple for convenience
        self.generate_vars_tuple(&all_vars);
    }

    /// Generate vars tuple helper.
    fn generate_vars_tuple(&mut self, all_vars: &[String]) {
        self.emit(&format!(
            "vars == <<{}>>",
            all_vars.join(", ")
        ));
        self.emit_blank();
    }

    /// Generate Init predicate for parallel composition.
    fn generate_composed_init(&mut self) {
        // Collect all init statements first to avoid borrow issues
        let mut init_statements = Vec::new();

        for context in &self.behaviors {
            // Initialize state
            init_statements.push(format!("{}_state = \"{}\"", context.name, context.initial_state));

            // Initialize PC
            init_statements.push(format!("{}_pc = 0", context.name));

            // Initialize history
            init_statements.push(format!("{}_history = <<>>", context.name));

            // Initialize extracted variables with default values
            for var in &context.extracted_vars {
                let default_value = if let Some(type_name) = context.explicit_var_types.get(var) {
                    match type_name.as_str() {
                        "Int" => "0",
                        "String" => "\"\"",
                        "Bool" => "FALSE",
                        _ => "0",
                    }
                } else {
                    "0"
                };

                init_statements.push(format!("{}_{} = {}", context.name, var, default_value));
            }
        }

        // Initialize shared message queues
        for (channel, _) in &self.shared_message_channels {
            init_statements.push(format!("{}_queue = <<>>", channel));
        }

        // Emit Init
        self.emit("Init ==");
        self.indent += 1;
        for stmt in init_statements.iter() {
            self.emit(&format!("/\\ {}", stmt));
        }
        self.indent -= 1;
        self.emit_blank();
    }

    /// Generate all transitions for all behaviors.
    fn generate_composed_transitions(&mut self) {
        self.emit("\\* Transition actions");
        self.emit_blank();

        // Collect all transition info to avoid borrow issues
        let mut transitions_info = Vec::new();

        for context in &self.behaviors {
            for transition in &context.behavior.transitions {
                if let (Some(from), Some(to)) = (transition.from.as_state(), transition.to.as_state()) {
                    transitions_info.push((
                        context.name.clone(),
                        from.to_string(),
                        to.to_string(),
                        transition.on_event.clone(),
                        transition.guard.clone(),
                        transition.effects.clone(),
                    ));
                }
            }
        }

        // Generate each transition
        for (behavior_name, from, to, event, guard, effects) in transitions_info {
            self.generate_behavior_transition(&behavior_name, &from, &to, &event, guard.as_ref(), &effects);
        }
    }

    /// Generate a single transition for a specific behavior.
    fn generate_behavior_transition(
        &mut self,
        behavior_name: &str,
        from: &str,
        to: &str,
        event: &str,
        guard: Option<&Expr>,
        effects: &[EffectStmt],
    ) {
        let action_name = format!("{}_{}_{}", behavior_name, from, event);

        self.emit(&format!("{} ==", action_name));
        self.indent += 1;

        // Precondition: behavior in correct state
        self.emit(&format!("/\\ {}_state = \"{}\"", behavior_name, from));

        // Guard if present
        if let Some(guard_expr) = guard {
            let guard_tla = self.expr_to_tla_scoped(guard_expr, behavior_name);
            self.emit(&format!("/\\ {}", guard_tla));
        }

        // State update
        self.emit(&format!("/\\ {}_state' = \"{}\"", behavior_name, to));

        // PC update
        self.emit(&format!("/\\ {}_pc' = {}_pc + 1", behavior_name, behavior_name));

        // History update
        self.emit(&format!("/\\ {}_history' = Append({}_history, {}_state)", behavior_name, behavior_name, behavior_name));

        // Effects
        self.generate_composed_effects(behavior_name, effects);

        // UNCHANGED clause for other behaviors' variables
        self.generate_unchanged_clause(behavior_name, effects);

        self.indent -= 1;
        self.emit_blank();
    }

    /// Generate effects with behavior scoping.
    fn generate_composed_effects(&mut self, behavior_name: &str, effects: &[EffectStmt]) {
        // Group Send effects by channel to avoid multiple primed assignments to the same variable.
        // TLA+ only allows one assignment per primed variable per action.
        let mut sends_by_channel: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        let mut receive_channels: Vec<String> = Vec::new();

        for effect in effects {
            match &effect.kind {
                EffectKind::Send { channel, message, args } => {
                    let channel_types = self.channel_arg_types.get(channel).cloned().unwrap_or_default();
                    let max_args = channel_types.len();

                    let mut fields = vec![format!("type |-> \"{}\"", message)];
                    for i in 0..max_args {
                        if i < args.len() {
                            let resolved_type = channel_types.get(i).map(|s| s.as_str()).unwrap_or("Int");
                            let arg_tla = self.expr_to_tla_scoped(&args[i], behavior_name);
                            // Normalize Bool→Int when the resolved type is Int but value is Bool
                            let arg_val = if resolved_type == "Int" && matches!(&args[i], Expr::Bool(_)) {
                                match &args[i] {
                                    Expr::Bool(true) => "1".to_string(),
                                    Expr::Bool(false) => "0".to_string(),
                                    _ => arg_tla,
                                }
                            } else {
                                arg_tla
                            };
                            fields.push(format!("arg{} |-> {}", i, arg_val));
                        } else {
                            // Pad missing fields with default values for uniform record shape
                            let default = match channel_types.get(i).map(|s| s.as_str()) {
                                Some("Bool") => "FALSE",
                                Some("Str") => "\"\"",
                                _ => "0",
                            };
                            fields.push(format!("arg{} |-> {}", i, default));
                        }
                    }
                    let record = format!("[{}]", fields.join(", "));
                    sends_by_channel.entry(channel.clone()).or_default().push(record);
                }
                EffectKind::Receive { channel, .. } => {
                    receive_channels.push(channel.clone());
                }
                EffectKind::Assign { var, value } => {
                    let value_tla = self.expr_to_tla_scoped(value, behavior_name);
                    self.emit(&format!("/\\ {}_{}'  = {}", behavior_name, var, value_tla));
                }
                _ => {}
            }
        }

        // Emit one primed assignment per send channel, concatenating all messages
        for (channel, records) in &sends_by_channel {
            if records.len() == 1 {
                self.emit(&format!("/\\ {}_queue' = Append({}_queue, {})", channel, channel, records[0]));
            } else {
                // Multiple messages: use sequence concatenation
                let seq_items = records.join(", ");
                self.emit(&format!("/\\ {}_queue' = {}_queue \\o <<{}>>", channel, channel, seq_items));
            }
        }

        // Emit receive effects (each consume from their queue)
        for channel in &receive_channels {
            self.emit(&format!("/\\ Len({}_queue) > 0", channel));
            self.emit(&format!("/\\ {}_queue' = Tail({}_queue)", channel, channel));
        }
    }

    /// Generate UNCHANGED clause for variables not modified by this transition.
    fn generate_unchanged_clause(&mut self, current_behavior: &str, effects: &[EffectStmt]) {
        // Collect all variables across all behaviors
        let mut all_vars = HashSet::new();
        let mut modified_vars = HashSet::new();

        for context in &self.behaviors {
            all_vars.insert(format!("{}_state", context.name));
            all_vars.insert(format!("{}_pc", context.name));
            all_vars.insert(format!("{}_history", context.name));
            for var in &context.extracted_vars {
                all_vars.insert(format!("{}_{}", context.name, var));
            }
        }

        for (channel, _) in &self.shared_message_channels {
            all_vars.insert(format!("{}_queue", channel));
        }

        // Mark modified variables
        modified_vars.insert(format!("{}_state", current_behavior));
        modified_vars.insert(format!("{}_pc", current_behavior));
        modified_vars.insert(format!("{}_history", current_behavior));

        for effect in effects {
            match &effect.kind {
                EffectKind::Send { channel, .. } => {
                    modified_vars.insert(format!("{}_queue", channel));
                }
                EffectKind::Receive { channel, .. } => {
                    modified_vars.insert(format!("{}_queue", channel));
                }
                EffectKind::Assign { var, .. } => {
                    modified_vars.insert(format!("{}_{}", current_behavior, var));
                }
                _ => {}
            }
        }

        // UNCHANGED = all_vars - modified_vars
        let unchanged_vars: Vec<String> = all_vars
            .difference(&modified_vars)
            .map(|s| s.to_string())
            .collect();

        if !unchanged_vars.is_empty() {
            self.emit(&format!("/\\ UNCHANGED <<{}>>", unchanged_vars.join(", ")));
        }
    }

    /// Convert expression to TLA+ with behavior-scoped variables.
    fn expr_to_tla_scoped(&self, expr: &Expr, behavior_name: &str) -> String {
        match expr {
            Expr::Ident(name) => format!("{}_{}", behavior_name, name),
            Expr::Int(n) => n.to_string(),
            Expr::String(s) => format!("\"{}\"", s),
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Expr::BinOp { lhs, rhs, op } => {
                let lhs_tla = self.expr_to_tla_scoped(lhs, behavior_name);
                let rhs_tla = self.expr_to_tla_scoped(rhs, behavior_name);
                let op_str = match op {
                    ArithOp::Add => "+",
                    ArithOp::Sub => "-",
                    ArithOp::Mul => "*",
                    ArithOp::Div => "/",
                };
                format!("({} {} {})", lhs_tla, op_str, rhs_tla)
            }
            Expr::CompOp { lhs, rhs, op } => {
                let lhs_tla = self.expr_to_tla_scoped(lhs, behavior_name);
                let rhs_tla = self.expr_to_tla_scoped(rhs, behavior_name);
                let op_str = match op {
                    ComparisonOp::Eq => "=",
                    ComparisonOp::Ne => "/=",
                    ComparisonOp::Lt => "<",
                    ComparisonOp::Le => "<=",
                    ComparisonOp::Gt => ">",
                    ComparisonOp::Ge => ">=",
                };
                format!("{} {} {}", lhs_tla, op_str, rhs_tla)
            }
            Expr::LogicalOp { lhs, rhs, op } => {
                let lhs_tla = self.expr_to_tla_scoped(lhs, behavior_name);
                let rhs_tla = self.expr_to_tla_scoped(rhs, behavior_name);
                let op_str = match op {
                    LogicalOp::And => "/\\",
                    LogicalOp::Or => "\\/",
                };
                format!("({} {} {})", lhs_tla, op_str, rhs_tla)
            }
            _ => "0".to_string(), // Fallback for unsupported expressions
        }
    }

    /// Generate Next predicate.
    fn generate_composed_next(&mut self) {
        // Collect all action names
        let mut action_names = Vec::new();

        for context in &self.behaviors {
            for transition in &context.behavior.transitions {
                if let (Some(from), Some(_to)) = (transition.from.as_state(), transition.to.as_state()) {
                    let action_name = format!("{}_{}_{}", context.name, from, transition.on_event);
                    action_names.push(action_name);
                }
            }
        }

        self.emit("Next ==");
        self.indent += 1;

        for (i, action) in action_names.iter().enumerate() {
            let prefix = if i == 0 { "\\/" } else { "\\/" };
            self.emit(&format!("{} {}", prefix, action));
        }

        // Add stuttering option
        self.emit("\\/ UNCHANGED vars");

        self.indent -= 1;
        self.emit_blank();
    }

    /// Generate Spec formula.
    fn generate_spec(&mut self) {
        self.emit("Spec == Init /\\ [][Next]_vars");
        self.emit_blank();
    }

    /// Generate TypeOK invariant for composed system.
    fn generate_composed_type_invariant(&mut self, max_queue_size: usize) {
        self.emit("\\* Type invariant");
        self.emit("TypeOK ==");
        self.indent += 1;

        // Collect lines to emit (to avoid borrow checker issues)
        let mut lines = Vec::new();

        // Check each behavior's state and pc
        for context in &self.behaviors {
            let state_names: Vec<String> = context.behavior.states
                .iter()
                .map(|s| format!("\"{}\"", s.name))
                .collect();

            lines.push(format!("/\\ {}_{} \\in {{{}}}",
                context.name, "state", state_names.join(", ")));
            lines.push(format!("/\\ {}_{} \\in Nat", context.name, "pc"));

            // Check extracted variables
            for var_name in &context.extracted_vars {
                if let Some(type_name) = context.explicit_var_types.get(var_name) {
                    match type_name.as_str() {
                        "Int" => {
                            // Check if there are bounds defined
                            if let Some(bounds) = context.variable_bounds.get(var_name) {
                                if let (Some(min), Some(max)) = (&bounds.min, &bounds.max) {
                                    // Extract numeric values for bounds
                                    let min_val = if let Expr::Int(n) = min { n.to_string() } else { "0".to_string() };
                                    let max_val = if let Expr::Int(n) = max { n.to_string() } else { "1000".to_string() };
                                    lines.push(format!("/\\ {}_{} \\in {}..{}",
                                        context.name, var_name, min_val, max_val));
                                }
                            }
                            // If no bounds, skip - checked via type annotations
                        }
                        "Bool" => {
                            lines.push(format!("/\\ {}_{} \\in BOOLEAN",
                                context.name, var_name));
                        }
                        _ => {} // Skip other types (Str, complex types, etc.)
                    }
                }
            }
        }

        // Check message queues
        for (channel_name, _) in &self.shared_message_channels {
            // Just check it's a sequence - detailed record type checked via Apalache type annotations
            lines.push(format!("/\\ Len({}_queue) <= {}",
                channel_name, max_queue_size));
        }

        // Emit all lines
        for line in lines {
            self.emit(&line);
        }

        self.indent -= 1;
        self.emit_blank();
    }

    /// Generate HistoryConsistent invariant for composed system.
    fn generate_composed_history_consistent(&mut self) {
        self.emit("\\* History length matches step count");
        self.emit("HistoryConsistent ==");
        self.indent += 1;

        // Collect lines to emit
        let lines: Vec<String> = self.behaviors.iter().enumerate().map(|(i, context)| {
            let prefix = if i == 0 { "/\\" } else { "/\\" };
            format!("{} Len({}_{}) = {}_{}",
                prefix, context.name, "history", context.name, "pc")
        }).collect();

        for line in lines {
            self.emit(&line);
        }

        self.indent -= 1;
        self.emit_blank();
    }

    /// Generate terminal state properties for behaviors with terminal states.
    fn generate_composed_terminal_properties(&mut self) {
        // Collect terminal state info to avoid borrow checker issues
        let terminal_info: Vec<(String, Vec<String>)> = self.behaviors.iter()
            .filter_map(|context| {
                let terminals: Vec<String> = context.behavior.states
                    .iter()
                    .filter(|s| s.terminal)
                    .map(|s| format!("\"{}\"", s.name))
                    .collect();

                if terminals.is_empty() {
                    None
                } else {
                    Some((context.name.clone(), terminals))
                }
            })
            .collect();

        for (behavior_name, terminals) in terminal_info {
            self.emit(&format!("\\* Terminal states for {}", behavior_name));
            self.emit(&format!("{}_TerminalStates == {{{}}}",
                behavior_name, terminals.join(", ")));
            self.emit_blank();

            self.emit(&format!("\\* Once {} enters terminal state, cannot leave", behavior_name));
            self.emit(&format!("{}_TerminalStable ==", behavior_name));
            self.indent += 1;
            // Action invariant (no temporal operators) so Apalache can check it with --inv.
            self.emit(&format!("({}_state \\in {}_TerminalStates) => ({}_state' \\in {}_TerminalStates)",
                behavior_name, behavior_name, behavior_name, behavior_name));
            self.indent -= 1;
            self.emit_blank();

            self.emit(&format!("\\* State invariant for Apalache trace generation ({})", behavior_name));
            self.emit(&format!("{}_NotTerminated ==", behavior_name));
            self.indent += 1;
            self.emit(&format!("{}_state \\notin {}_TerminalStates",
                behavior_name, behavior_name));
            self.indent -= 1;
            self.emit_blank();
        }
    }

    /// Generate user-defined invariants with namespacing.
    fn generate_composed_user_invariants(&mut self, behavior_name: &str, invariants: &[InvariantDecl]) {
        if invariants.is_empty() {
            return;
        }

        let boolean_invs: Vec<_> = invariants.iter()
            .filter(|inv| !TlaGenerator::is_non_boolean_expr(&inv.expr))
            .collect();

        if boolean_invs.is_empty() {
            return;
        }

        self.emit(&format!("\\* User-defined invariants for {}", behavior_name));
        for inv in &boolean_invs {
            self.emit(&format!("{}_Inv_{} ==", behavior_name, inv.name));
            self.indent += 1;
            self.emit(&self.expr_to_tla_scoped(&inv.expr, behavior_name));
            self.indent -= 1;
            self.emit_blank();
        }
    }

    /// Generate a complete TLA+ module for parallel composition.
    fn generate(&mut self) -> String {
        let max_queue_size = self.config.max_queue_size;

        self.generate_header();
        self.collect_shared_channels();
        self.generate_composed_variables();
        self.generate_composed_init();
        self.generate_composed_transitions();
        self.generate_composed_next();
        self.generate_spec();

        // Generate invariants
        self.generate_composed_type_invariant(max_queue_size);
        self.generate_composed_history_consistent();
        self.generate_composed_terminal_properties();

        // Generate user invariants for each behavior
        // Collect info first to avoid borrow conflict with self.emit()
        let invariant_info: Vec<_> = self.behaviors.iter()
            .map(|ctx| (ctx.name.clone(), ctx.behavior.invariants.clone()))
            .collect();
        for (behavior_name, invariants) in &invariant_info {
            self.generate_composed_user_invariants(behavior_name, invariants);
        }

        // Add module footer
        self.emit(&format!("===="));

        self.output.clone()
    }
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
            variable_bounds: HashMap::new(),
            message_channels: HashMap::new(),
            state_names: HashSet::new(),
            module_level_assumes: Vec::new(),
            has_terminal_states: false,
            channel_arg_types: HashMap::new(),
            diagnostics: Vec::new(),
            action_names: Vec::new(),
            called_functions: HashMap::new(),
            bool_functions: HashSet::new(),
        }
    }

    /// Resolve a state name used in temporal properties.
    ///
    /// If the name matches a known state constant directly, return it as-is.
    /// Otherwise, look for a prefixed variant (e.g., "open" -> "circuitbreaker_open")
    /// since pattern expansion prefixes state names but temporal properties may reference
    /// the unprefixed name.
    ///
    /// Returns None if the name cannot be resolved to any known state.
    fn resolve_state_name(&self, name: &str) -> Option<String> {
        if self.state_names.contains(name) {
            return Some(name.to_string());
        }
        // Search for a state whose name ends with _<name>
        let suffix = format!("_{}", name);
        for known in &self.state_names {
            if known.ends_with(&suffix) {
                return Some(known.clone());
            }
        }
        None
    }

    /// Pre-compute disambiguated action names for all transitions.
    ///
    /// When multiple transitions share the same (from, event), suffixes their
    /// target state to avoid duplicate TLA+ operator definitions.
    fn compute_action_names(&mut self, transitions: &[TransitionDecl]) {
        // Count occurrences of each base name
        let mut base_counts: HashMap<String, usize> = HashMap::new();
        let mut base_names: Vec<String> = Vec::new();

        for t in transitions {
            let base = match (&t.from, &t.to) {
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
            };
            *base_counts.entry(base.clone()).or_insert(0) += 1;
            base_names.push(base);
        }

        // Assign final names: disambiguate duplicates by appending target state
        let mut seen: HashMap<String, usize> = HashMap::new();
        self.action_names = Vec::with_capacity(transitions.len());

        for (i, base) in base_names.iter().enumerate() {
            let name = if base_counts[base] > 1 {
                // Disambiguate by appending target state name
                let suffix = transitions[i].to.as_state().unwrap_or("unknown");
                let candidate = format!("{}_{}", base, suffix);
                let count = seen.entry(candidate.clone()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    format!("{}_{}", candidate, count)
                } else {
                    candidate
                }
            } else {
                base.clone()
            };
            self.action_names.push(name);
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
            if let Some(ref bounds) = var.bounds {
                self.variable_bounds.insert(var.name.clone(), bounds.clone());
            }
        }

        for t in &behavior.transitions {
            if let Some(ref guard) = t.guard {
                // Mark bare Ident operands in guard as Bool before general var collection
                self.mark_bool_guard_vars(guard);
                self.collect_vars_from_expr(guard);
            }
            for effect in &t.effects {
                self.collect_from_effect(effect);
            }
        }
        for inv in &behavior.invariants {
            self.collect_vars_from_expr(&inv.expr);
            self.extract_assumes_from_expr(&inv.expr);
            // Extract variable bounds from invariant comparisons so Init can use valid defaults
            self.extract_bounds_from_invariant_expr(&inv.expr);
        }

        // Compute per-channel arg types from Send effects
        self.compute_channel_arg_types(behavior);
    }

    /// Scan Send effects to determine actual arg types per position for each channel.
    fn compute_channel_arg_types(&mut self, behavior: &BehaviorDecl) {
        let mut channel_info: HashMap<String, (usize, HashMap<usize, HashSet<&'static str>>)> = HashMap::new();
        for t in &behavior.transitions {
            for effect in &t.effects {
                if let EffectKind::Send { channel, args, .. } = &effect.kind {
                    let entry = channel_info.entry(channel.clone()).or_insert_with(|| (0, HashMap::new()));
                    if args.len() > entry.0 { entry.0 = args.len(); }
                    for (i, arg) in args.iter().enumerate() {
                        let t = match arg {
                            Expr::Int(_) => "Int",
                            Expr::Bool(_) => "Bool",
                            Expr::String(_) => "Str",
                            Expr::Ident(name) => {
                                self.explicit_var_types.get(name).map(|s| match s.as_str() {
                                    "String" | "Str" => "Str",
                                    "Bool" => "Bool",
                                    _ => "Int",
                                }).unwrap_or("Int")
                            }
                            _ => "Int",
                        };
                        entry.1.entry(i).or_default().insert(t);
                    }
                }
            }
        }
        for (channel, (max_args, pos_types)) in &channel_info {
            let mut resolved = Vec::with_capacity(*max_args);
            for i in 0..*max_args {
                let types = pos_types.get(&i);
                let rt = match types {
                    Some(ts) if ts.len() == 1 => ts.iter().next().unwrap().to_string(),
                    Some(ts) if ts.contains("Str") => "Str".to_string(),
                    Some(_) => "Int".to_string(),
                    None => "Int".to_string(),
                };
                resolved.push(rt);
            }
            self.channel_arg_types.insert(channel.clone(), resolved);
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

    /// Scan invariant expressions for `var >= min_val` / `var <= max_val` patterns
    /// and populate `variable_bounds` so Init can use a valid default value.
    fn extract_bounds_from_invariant_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::CompOp { lhs, op, rhs } => {
                // var >= val  →  lower bound on var
                // var <= val  →  upper bound on var
                // val >= var  →  upper bound on var
                // val <= var  →  lower bound on var
                let (var_name, bound_side, is_lower) = match (lhs.as_ref(), op, rhs.as_ref()) {
                    (Expr::Ident(n), ComparisonOp::Ge | ComparisonOp::Gt, val) => (n, val, true),
                    (Expr::Ident(n), ComparisonOp::Le | ComparisonOp::Lt, val) => (n, val, false),
                    (val, ComparisonOp::Ge | ComparisonOp::Gt, Expr::Ident(n)) => (n, val, false),
                    (val, ComparisonOp::Le | ComparisonOp::Lt, Expr::Ident(n)) => (n, val, true),
                    _ => return,
                };
                // Only extract bounds from literal values (Int, Float)
                match bound_side {
                    Expr::Int(_) | Expr::Float(_) => {
                        let bounds = self.variable_bounds.entry(var_name.clone()).or_insert_with(|| ValueBounds { min: None, max: None, values: None });
                        if is_lower && bounds.min.is_none() {
                            bounds.min = Some((*bound_side).clone());
                        } else if !is_lower && bounds.max.is_none() {
                            bounds.max = Some((*bound_side).clone());
                        }
                    }
                    _ => {}
                }
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                self.extract_bounds_from_invariant_expr(lhs);
                self.extract_bounds_from_invariant_expr(rhs);
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
            Expr::Call { name, args } => {
                // Track function names so we can emit stubs for undefined ones
                const TLA_BUILTINS: &[&str] = &[
                    "Append", "Head", "Tail", "Len", "SubSeq", "SelectSeq",
                    "Cardinality", "CHOOSE", "DOMAIN", "IF",
                    "Min", "Max",
                ];
                if !TLA_BUILTINS.contains(&name.as_str()) {
                    let entry = self.called_functions.entry(name.clone()).or_insert(0);
                    if args.len() > *entry { *entry = args.len(); }
                }
                for arg in args {
                    self.collect_vars_from_expr(arg);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::CompOp { lhs, rhs, .. } => {
                // If one side is a string literal and the other a bare Ident, the
                // variable is Str-typed (not Int). Record this before general collection.
                match (lhs.as_ref(), rhs.as_ref()) {
                    (Expr::Ident(name), Expr::String(_)) | (Expr::String(_), Expr::Ident(name)) => {
                        if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                            self.explicit_var_types.insert(name.clone(), "Str".to_string());
                        }
                    }
                    (Expr::Ident(name), Expr::Bool(_)) | (Expr::Bool(_), Expr::Ident(name)) => {
                        if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                            self.explicit_var_types.insert(name.clone(), "Bool".to_string());
                        }
                    }
                    _ => {}
                }
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                // Bare Ident operands to a logical operator are boolean flags
                if let Expr::Ident(name) = lhs.as_ref() {
                    if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                        self.explicit_var_types.insert(name.clone(), "Bool".to_string());
                    }
                }
                if let Expr::Ident(name) = rhs.as_ref() {
                    if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                        self.explicit_var_types.insert(name.clone(), "Bool".to_string());
                    }
                }
                self.collect_vars_from_expr(lhs);
                self.collect_vars_from_expr(rhs);
            }
            Expr::UnaryOp { op, expr } => {
                // Bare Ident operand to NOT is a boolean flag
                if matches!(op, UnaryOp::Not) {
                    if let Expr::Ident(name) = expr.as_ref() {
                        if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                            self.explicit_var_types.insert(name.clone(), "Bool".to_string());
                        }
                    }
                }
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
            EffectKind::Send { channel, message, args } => {
                // Track message channel and type
                self.message_channels.entry(channel.clone())
                    .or_insert_with(HashSet::new)
                    .insert(message.clone());
                for arg in args {
                    self.collect_vars_from_expr(arg);
                }
            }
            EffectKind::Receive { channel, message, filter } => {
                // Track message channel and type
                self.message_channels.entry(channel.clone())
                    .or_insert_with(HashSet::new)
                    .insert(message.clone());
                if let Some(filter_expr) = filter {
                    self.collect_vars_from_expr(filter_expr);
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

    /// Recursively mark bare Ident leaves of a boolean-context expression as Bool.
    /// Call this on guard expressions and other places where the top-level value must be Bool.
    fn mark_bool_guard_vars(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => {
                if !self.is_state_name(name) && !self.explicit_var_types.contains_key(name) {
                    self.explicit_var_types.insert(name.clone(), "Bool".to_string());
                }
            }
            Expr::Call { name, .. } => {
                // A function call used as a bare guard must return Bool
                self.bool_functions.insert(name.clone());
            }
            Expr::LogicalOp { lhs, rhs, .. } => {
                self.mark_bool_guard_vars(lhs);
                self.mark_bool_guard_vars(rhs);
            }
            Expr::UnaryOp { op, expr } if matches!(op, UnaryOp::Not) => {
                self.mark_bool_guard_vars(expr);
            }
            // CompOp, BinOp, etc. are already Bool-typed by their structure;
            // their sub-expressions are not in a Bool context so we leave them alone.
            _ => {}
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
        self.line("\\* AUTO-GENERATED by Intent compiler – do not edit by hand.");
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

        // Event type – args omitted in Apalache mode to avoid <<...>> tuple/Seq ambiguity
        self.line("\\* @typeAlias: EVENT = { type: Str };");
        self.line("\\* @typeAlias: EVENT_QUEUE = Seq(EVENT);");
        self.blank();

        // History type
        self.line("\\* @typeAlias: HISTORY = Seq(STATE);");
        self.blank();
        // Variable declarations follow in the VARIABLES block below.
        // Type aliases above are used as annotations there.
    }

    /// Infer Apalache type for a variable.
    ///
    /// Checks explicit type declarations first, then falls back to heuristics.
    fn infer_apalache_type(&mut self, var_name: &str) -> String {
        // Check explicit declaration first
        if let Some(type_name) = self.explicit_var_types.get(var_name) {
            return self.type_name_to_apalache(type_name);
        }

        // Fall back to heuristic based on variable name
        let lower = var_name.to_lowercase();
        let inferred = if lower.contains("count") || lower.contains("num") || lower.contains("size") || lower.contains("level") {
            "Int".to_string()
        } else if lower.contains("enabled") || lower.contains("active") || lower.contains("valid") {
            "Bool".to_string()
        } else if lower.contains("list") || lower.contains("queue") || lower.contains("items") || lower.contains("seq") {
            "Seq(Int)".to_string()
        } else if lower.contains("set") || lower.contains("pool") || lower.contains("ids") {
            // "ids" (plural) = a collection of identifiers; must come before the "id" check
            "Set(Str)".to_string()
        } else if lower.contains("id") || lower.contains("name") || lower.contains("address")
            || lower.contains("token") || lower.contains("key") {
            "Str".to_string()
        } else {
            "Int".to_string()  // Default to Int for symbolic
        };

        self.diagnostics.push(
            Diagnostic::warning(
                ErrorCode::E056_HeuristicTypeInference,
                format!(
                    "Variable '{}' has no explicit type annotation; inferred '{}' by name heuristic",
                    var_name, inferred
                ),
                Span::synthetic(),
            )
            .with_suggestion(format!(
                "Add an explicit type annotation: var {}: {}",
                var_name, inferred
            )),
        );

        inferred
    }

    /// Convert an Intent type name to an Apalache type.
    fn type_name_to_apalache(&self, type_name: &str) -> String {
        match type_name {
            "Int" | "Integer" => "Int".to_string(),
            "Nat" => "Int".to_string(), // Nat maps to Int in Apalache; constraint in TypeOK
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
            // Handle constrained types: subset T -> Set(T), [K -> V] -> [K -> V]
            s if s.starts_with("subset ") => {
                let inner = &s["subset ".len()..];
                format!("Set({})", self.type_name_to_apalache(inner))
            }
            // Default for unknown types
            _ => "Int".to_string(),
        }
    }

    fn generate_constants(&mut self, states: &[StateDecl], parameters: &[crate::parser::ast::PatternParam]) {
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
        let has_real_constants = self.nodes.is_some() || !parameters.is_empty();

        // Only generate CONSTANTS section if we have real constants (nodes or parameters)
        if has_real_constants {
            self.line("\\* Constants");
            self.line("CONSTANTS");
            self.indent += 1;

            // Add nodes constant if present
            if let Some(nodes) = &self.nodes.clone() {
                let suffix = if parameters.is_empty() { "" } else { "," };
                self.line("\\* @type: Set(Str);");
                self.line(&format!("{}{}", nodes, suffix));
            }

            // Add behavior parameters as constants
            if !parameters.is_empty() {
                for (i, param) in parameters.iter().enumerate() {
                    let suffix = if i == parameters.len() - 1 { "" } else { "," };
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

            self.indent -= 1;
            self.blank();
        }

        // Define state values directly (not as CONSTANTS) for Apalache compatibility
        if !state_names.is_empty() {
            self.line("\\* State values (defined as strings for Apalache)");
            for state_name in &state_names {
                self.line(&format!("{} == \"{}\"", state_name, state_name));
            }
            self.blank();
        }

        self.line("States == {");
        self.indent += 1;
        self.line(&state_names.join(", "));
        self.indent -= 1;
        self.line("}");
        self.blank();
    }

    fn generate_variables_extended(&mut self) {
        // Emit the VARIABLES block with type annotations.
        // In Apalache mode, use the type aliases defined in generate_apalache_types
        // (STATE, HISTORY, EVENT_QUEUE). The vars tuple below is also annotated
        // to resolve tuple/Seq ambiguity in Snowcat.
        self.line("VARIABLES");
        self.indent += 1;

        // Use concrete types (not aliases) in variable annotations so that
        // Apalache's Snowcat type-checker can unify with Init expressions
        // like <<>> and [nodes -> {values}].
        let (state_annot, state_comment) = if self.nodes.is_some() {
            ("\\* @type: Str -> Str;", "state,      \\* Per-node state (function from nodes to States)")
        } else {
            ("\\* @type: Str;", "state,      \\* Current state")
        };
        self.line(state_annot);
        self.line(state_comment);
        self.line("\\* @type: Int;");
        self.line("pc,         \\* Program counter for step tracking");

        self.line("\\* @type: Seq(Str);");
        self.line("history,    \\* Sequence of visited states (for trace analysis)");

        // Event queue
        let has_message_queues = !self.message_channels.is_empty();
        let has_extracted_vars = !self.extracted_vars.is_empty();
        let eq_annot = "\\* @type: Seq({ type: Str });";

        if !has_message_queues && !has_extracted_vars {
            self.line(eq_annot);
            self.line("event_queue     \\* Pending events/messages queue");
        } else {
            self.line(eq_annot);
            self.line("event_queue,    \\* Pending events/messages queue");
        }

        // Message queues for typed message passing
        if has_message_queues {
            let mut channels: Vec<_> = self.message_channels.keys().cloned().collect();
            channels.sort();
            for (idx, channel) in channels.iter().enumerate() {
                let queue_name = format!("{}_queue", self.sanitize_var_name(channel));
                // Build precise record type from computed arg types
                let mut fields = vec!["type: Str".to_string()];
                if let Some(arg_types) = self.channel_arg_types.get(channel) {
                    for (i, atype) in arg_types.iter().enumerate() {
                        fields.push(format!("arg{}: {}", i, atype));
                    }
                }
                self.line(&format!("\\* @type: Seq({{ {} }});", fields.join(", ")));
                if idx == channels.len() - 1 && !has_extracted_vars {
                    self.line(&format!("{}     \\* Message queue for {}", queue_name, channel));
                } else {
                    self.line(&format!("{},    \\* Message queue for {}", queue_name, channel));
                }
            }
        }

        // Extracted data variables
        if has_extracted_vars {
            let mut vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
            vars.sort();
            for (i, var) in vars.iter().enumerate() {
                let safe_name = self.sanitize_var_name(var);
                let type_hint = self.infer_apalache_type(var);
                self.line(&format!("\\* @type: {};", type_hint));
                if i == vars.len() - 1 {
                    self.line(&format!("{}     \\* Data variable (extracted)", safe_name));
                } else {
                    self.line(&format!("{},    \\* Data variable (extracted)", safe_name));
                }
            }
        }
        self.indent -= 1;
        self.blank();

        // Build vars tuple (and a parallel type list for the Apalache @type annotation).
        let state_type = if self.nodes.is_some() { "Str -> Str" } else { "Str" };
        let mut all_vars = vec!["state".to_string(), "pc".to_string(), "history".to_string(), "event_queue".to_string()];
        let mut all_types = vec![state_type.to_string(), "Int".to_string(), "Seq(Str)".to_string(), "Seq({ type: Str })".to_string()];

        // Add message queue variables
        let mut channels: Vec<_> = self.message_channels.keys().cloned().collect();
        channels.sort();
        for channel in &channels {
            all_vars.push(format!("{}_queue", self.sanitize_var_name(channel)));
            let mut fields = vec!["type: Str".to_string()];
            if let Some(arg_types) = self.channel_arg_types.get(channel) {
                for (i, atype) in arg_types.iter().enumerate() {
                    fields.push(format!("arg{}: {}", i, atype));
                }
            }
            all_types.push(format!("Seq({{ {} }})", fields.join(", ")));
        }

        // Add extracted data variables
        let mut extracted: Vec<String> = self.extracted_vars.iter().cloned().collect();
        extracted.sort();
        for var in &extracted {
            all_vars.push(self.sanitize_var_name(var));
            all_types.push(self.infer_apalache_type(var));
        }

        // In Apalache mode, annotate vars with its tuple type so Snowcat can resolve
        // the <<T, T, ...>> ambiguity between tuples and sequences.
        if self.config.apalache_types {
            self.line(&format!("\\* @type: <<{}>>;", all_types.join(", ")));
        }
        self.line(&format!("vars == <<{}>>", all_vars.join(", ")));
        self.blank();
    }

    /// Sanitize a variable name for TLA+ (replace dots with underscores)
    fn sanitize_var_name(&self, name: &str) -> String {
        name.replace('.', "_").replace('-', "_")
    }

    fn generate_functions(&mut self, functions: &[crate::parser::ast::FunctionDecl]) {
        let defined_names: HashSet<String> = functions.iter().map(|f| f.name.clone()).collect();

        if !functions.is_empty() {
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

        // In Apalache mode, emit stubs for called-but-undefined functions so the module parses.
        // Bool-context functions (bare guard calls) get `== TRUE`; value-context functions
        // (used inside comparisons/arithmetic) get an identity stub `== first_param`.
        if self.config.apalache_types {
            let stubs: Vec<(String, usize)> = self.called_functions
                .iter()
                .filter(|(name, _)| !defined_names.contains(*name))
                .map(|(name, arity)| (name.clone(), *arity))
                .collect();
            if !stubs.is_empty() {
                let mut stubs_sorted = stubs;
                stubs_sorted.sort_by(|a, b| a.0.cmp(&b.0));
                self.line("\\* Stubs for domain-specific functions (abstract model for formal verification)");
                let bool_fns = self.bool_functions.clone();
                for (name, arity) in stubs_sorted {
                    let arity = arity.max(1);
                    let params: Vec<String> = (0..arity).map(|i| format!("_p{}", i)).collect();
                    let body = if bool_fns.contains(&name) {
                        "TRUE".to_string()
                    } else {
                        params[0].clone()
                    };
                    self.line(&format!("{}({}) == {}", name, params.join(", "), body));
                }
                self.blank();
            }
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
        // Only emit ASSUME for constant-level assumptions extracted from invariant blocks.
        // Variable bounds belong in TypeOK (generated by generate_type_invariant), not here,
        // because TLA+ ASSUME is only valid for constant-level expressions.
        if self.module_level_assumes.is_empty() {
            return;
        }

        self.line("\\* Assumptions extracted from invariants (must be at module level)");
        for assume in &self.module_level_assumes.clone() {
            self.line(&format!("ASSUME {}", assume));
        }

        self.blank();
    }

    /// Generate CInit operator for Apalache's --cinit flag.
    /// Required for modules with CONSTANTS (e.g. distributed systems with a `nodes` set).
    /// Without CInit, Apalache cannot assign values to uninitialized CONSTANTS.
    fn generate_cinit(&mut self) {
        let nodes = match &self.nodes {
            Some(n) => n.clone(),
            None => return,
        };

        self.line("\\* CInit: constant initializer for Apalache (--cinit=CInit)");
        self.line("\\* Provides a small default value for bounded model checking.");
        self.line("CInit ==");
        self.indent += 1;
        self.line(&format!("{} = {{\"n1\", \"n2\", \"n3\"}}", nodes));
        self.indent -= 1;
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

        // For distributed systems, initialize all nodes to the initial state
        if let Some(nodes) = &self.nodes {
            if initial.len() == 1 {
                self.line(&format!("/\\ state = [n \\in {} |-> {}]", nodes, initial[0]));
            } else if initial.is_empty() {
                self.line(&format!("/\\ state \\in [{}-> States]", nodes));
            } else {
                self.line(&format!("/\\ state \\in [{} -> {{{}}}]", nodes, initial.join(", ")));
            }
        } else {
            // Single-state machine
            if initial.len() == 1 {
                self.line(&format!("/\\ state = {}", initial[0]));
            } else if initial.is_empty() {
                self.line("/\\ state \\in States");
            } else {
                self.line(&format!("/\\ state \\in {{{}}}", initial.join(", ")));
            }
        }
        self.line("/\\ pc = 0");
        self.line("/\\ history = <<>>");
        self.line("/\\ event_queue = <<>>");

        // Initialize message queues
        if !self.message_channels.is_empty() {
            let mut channels: Vec<_> = self.message_channels.keys().cloned().collect();
            channels.sort();
            for channel in &channels {
                let queue_name = format!("{}_queue", self.sanitize_var_name(channel));
                self.line(&format!("/\\ {} = <<>>", queue_name));
            }
        }

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

    /// Infer a reasonable initial value based on explicit type declarations and variable name patterns.
    fn infer_initial_value(&mut self, var_name: &str) -> String {
        // Check explicit type declaration first – use a type-appropriate zero value.
        // This prevents name heuristics from overriding the declared type (e.g. an Int
        // variable named "requestId" must not be initialised to the string "requestId").
        if let Some(type_name) = self.explicit_var_types.get(var_name) {
            // Still let bounds override the zero value when present (handled below).
            if self.variable_bounds.get(var_name).is_none() {
                return match type_name.as_str() {
                    "Int" | "Nat" => "0".to_string(),
                    "Bool" => "FALSE".to_string(),
                    "String" | "Str" => "\"\"".to_string(),
                    _ => "0".to_string(),
                };
            }
        }

        // Check if variable has bounds
        if let Some(bounds) = self.variable_bounds.get(var_name) {
            // If bounds specify allowed values (enumeration), use the first one
            if let Some(ref values) = bounds.values {
                if !values.is_empty() {
                    return self.expr_to_tla(&values[0]);
                }
            }
            // If bounds specify a minimum, use that
            if let Some(ref min) = bounds.min {
                return self.expr_to_tla(min);
            }
        }

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
        for (idx, t) in transitions.iter().enumerate() {
            match (&t.from, &t.to) {
                // Simple state-to-state transition
                (TransitionSource::State(from), TransitionTarget::State(to)) => {
                    self.generate_simple_transition(t, from, to, idx);
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
    fn generate_simple_transition(&mut self, t: &TransitionDecl, from: &str, to: &str, idx: usize) {
        let action_name = if idx < self.action_names.len() {
            self.action_names[idx].clone()
        } else {
            format!("{}_{}", from, t.on_event)
        };

        // For distributed systems, parameterize by node
        if let Some(nodes) = self.nodes.clone() {
            self.line(&format!("{}(n) ==", action_name));
            self.indent += 1;
            self.line(&format!("/\\ n \\in {}", nodes));
            self.line(&format!("/\\ state[n] = {}", from));

            if let Some(ref guard) = t.guard {
                self.line(&format!("/\\ {}", self.expr_to_tla(guard)));
            }

            self.line(&format!("/\\ state' = [state EXCEPT ![n] = {}]", to));
            self.line("/\\ pc' = pc + 1");
            self.line(&format!("/\\ history' = Append(history, state[n])"));

            self.generate_pending_and_effects(&t.effects);
            self.indent -= 1;
            self.blank();
        } else {
            // Single-state machine
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
            // Build a sequence of emitted events.
            // In Apalache mode, omit the args field: <<arg1, arg2>> is ambiguous
            // between a tuple and a Seq, which causes Snowcat type errors.
            let emit_strs: Vec<String> = emits
                .iter()
                .map(|(name, args)| {
                    if self.config.apalache_types {
                        format!("[type |-> \"{}\"]", name)
                    } else {
                        let args_str: Vec<String> =
                            args.iter().map(|a| self.expr_to_tla(a)).collect();
                        if args_str.is_empty() {
                            format!("[type |-> \"{}\"]", name)
                        } else {
                            format!(
                                "[type |-> \"{}\", args |-> <<{}>>]",
                                name,
                                args_str.join(", ")
                            )
                        }
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

        // Collect all variable names (extracted vars + message queues)
        let mut all_vars: Vec<String> = self.extracted_vars.iter().cloned().collect();
        let mut channels: Vec<_> = self.message_channels.keys().cloned().collect();
        channels.sort();
        for channel in &channels {
            all_vars.push(format!("{}_queue", channel));
        }

        // Handle variable updates
        if !all_vars.is_empty() {
            all_vars.sort();

            // Merge updates for the same variable (e.g. multiple sends to the same queue).
            // TLA+ only allows one assignment per primed variable per action.
            let mut merged_updates: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
            for (var, update_expr) in &var_updates {
                merged_updates.entry(var.clone()).or_default().push(update_expr.clone());
            }

            // Separate modified and unchanged variables
            let mut modified_vars: HashSet<String> = HashSet::new();
            for var in merged_updates.keys() {
                modified_vars.insert(var.clone());
            }

            // Output explicit updates for modified variables
            for (var, exprs) in &merged_updates {
                let safe_name = self.sanitize_var_name(var);
                if exprs.len() == 1 {
                    self.line(&format!("/\\ {}' = {}", safe_name, exprs[0]));
                } else {
                    // Multiple updates to the same queue – chain the \o concatenations.
                    // Each expr is like "Channel_queue \o <<msg>>", so we extract the
                    // message parts and combine them into a single concatenation.
                    let mut all_messages = Vec::new();
                    for expr in exprs {
                        // Extract the message record from "var \o <<record>>"
                        if let Some(start) = expr.find("\\o <<") {
                            let inner = &expr[start + 5..];
                            if let Some(end) = inner.rfind(">>") {
                                all_messages.push(inner[..end].to_string());
                            }
                        }
                    }
                    if !all_messages.is_empty() {
                        self.line(&format!("/\\ {}' = {} \\o <<{}>>", safe_name, safe_name, all_messages.join(", ")));
                    } else {
                        // Fallback: just use the last update
                        self.line(&format!("/\\ {}' = {}", safe_name, exprs.last().unwrap()));
                    }
                }
            }

            // Mark remaining vars as UNCHANGED
            let unchanged: Vec<String> = all_vars
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
                EffectKind::Send { channel, message, args } => {
                    // Generate message queue append operation with named arg fields
                    let queue_name = format!("{}_queue", self.sanitize_var_name(channel));
                    let channel_types = self.channel_arg_types.get(channel).cloned().unwrap_or_default();
                    let max_args = channel_types.len();

                    let mut fields = vec![format!("type |-> \"{}\"", message)];
                    for i in 0..max_args {
                        if i < args.len() {
                            let resolved_type = channel_types.get(i).map(|s| s.as_str()).unwrap_or("Int");
                            let arg_tla = self.expr_to_tla(&args[i]);
                            // Normalize Bool→Int when the resolved type is Int but value is Bool
                            let arg_val = if resolved_type == "Int" && matches!(&args[i], Expr::Bool(_)) {
                                match &args[i] {
                                    Expr::Bool(true) => "1".to_string(),
                                    Expr::Bool(false) => "0".to_string(),
                                    _ => arg_tla,
                                }
                            } else {
                                arg_tla
                            };
                            fields.push(format!("arg{} |-> {}", i, arg_val));
                        } else {
                            // Pad missing fields with default values for uniform record shape
                            let default = match channel_types.get(i).map(|s| s.as_str()) {
                                Some("Bool") => "FALSE",
                                Some("Str") => "\"\"",
                                _ => "0",
                            };
                            fields.push(format!("arg{} |-> {}", i, default));
                        }
                    }
                    let msg_record = format!("[{}]", fields.join(", "));
                    let update = format!("{} \\o <<{}>>", queue_name, msg_record);
                    updates.push((queue_name, update));
                }
                EffectKind::Receive { channel, message: _, filter: _ } => {
                    // Generate message queue dequeue operation
                    // For simplicity, just remove the head of the queue
                    let queue_name = format!("{}_queue", self.sanitize_var_name(channel));
                    let update = format!("Tail({})", queue_name);
                    updates.push((queue_name, update));
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
            EffectKind::Send { .. } => true, // Handled in message queue updates
            EffectKind::Receive { .. } => true, // Handled in message queue updates
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
            EffectKind::Send { channel, message, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.expr_to_tla(a)).collect();
                self.line(&format!("\\* SEND: {}.{}({})", channel, message, args_str.join(", ")));
            }
            EffectKind::Receive { channel, message, filter } => {
                if let Some(f) = filter {
                    self.line(&format!("\\* RECEIVE: {}.{} where {}", channel, message, self.expr_to_tla(f)));
                } else {
                    self.line(&format!("\\* RECEIVE: {}.{}", channel, message));
                }
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
            // Use pre-computed action names
            let actions: Vec<String> = if self.action_names.len() == transitions.len() {
                self.action_names.clone()
            } else {
                // Fallback if action_names not computed
                transitions.iter().map(|t| {
                    match (&t.from, &t.to) {
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
                    }
                }).collect()
            };

            // For distributed systems, wrap actions in existential quantifiers
            if let Some(nodes) = self.nodes.clone() {
                for action in &actions {
                    self.line(&format!("\\/ \\E n \\in {} : {}(n)", nodes, action));
                }
            } else {
                for action in &actions {
                    self.line(&format!("\\/ {}", action));
                }
            }

            // Always allow stuttering to prevent deadlock when no transitions are enabled
            self.line("\\/ UNCHANGED vars");
        }

        self.indent -= 1;
        self.blank();
    }

    fn generate_fairness(&mut self, fairness: &[FairnessSpec], transitions: &[TransitionDecl]) {
        if fairness.is_empty() {
            return;
        }

        self.line("\\* Fairness conditions");
        let mut emitted: HashSet<String> = HashSet::new();
        for f in fairness {
            let action_names = self.find_action_names(f, transitions);
            if action_names.is_empty() {
                continue;
            }
            let fair_type = match f.kind {
                FairnessKind::Weak => "WF",
                FairnessKind::Strong => "SF",
            };

            // Build a unique definition name from source and all targets
            let targets_label = {
                let mut targets = vec![f.to.clone()];
                targets.extend(f.alts.iter().cloned());
                targets.join("_or_")
            };
            let def_name = format!("Fairness_{}_to_{}", f.from, targets_label);
            if emitted.contains(&def_name) {
                continue;
            }
            emitted.insert(def_name.clone());

            // For distributed systems, apply fairness to each node
            if let Some(nodes) = self.nodes.clone() {
                if action_names.len() == 1 {
                    self.line(&format!(
                        "{} == \\A n \\in {} : {}_vars({}(n))",
                        def_name, nodes, fair_type, action_names[0]
                    ));
                } else {
                    let disj = action_names
                        .iter()
                        .map(|a| format!("{}_vars({}(n))", fair_type, a))
                        .collect::<Vec<_>>()
                        .join(" \\/ ");
                    self.line(&format!(
                        "{} == \\A n \\in {} : ({})",
                        def_name, nodes, disj
                    ));
                }
            } else if action_names.len() == 1 {
                self.line(&format!(
                    "{} == {}_vars({})",
                    def_name, fair_type, action_names[0]
                ));
            } else {
                let disj = action_names
                    .iter()
                    .map(|a| format!("{}_vars({})", fair_type, a))
                    .collect::<Vec<_>>()
                    .join(" \\/ ");
                self.line(&format!("{} == {}", def_name, disj));
            }
        }
        self.blank();
    }

    /// Find all action operator names matching a fairness spec's source and targets.
    fn find_action_names(&self, f: &FairnessSpec, transitions: &[TransitionDecl]) -> Vec<String> {
        let mut targets: Vec<&str> = vec![f.to.as_str()];
        for alt in &f.alts {
            targets.push(alt.as_str());
        }

        let mut actions = Vec::new();
        for (i, t) in transitions.iter().enumerate() {
            let from_match = t.from.as_state() == Some(f.from.as_str());
            let to_match = targets.iter().any(|target| t.to.as_state() == Some(target));
            if from_match && to_match {
                // Use pre-computed action name if available
                let name = if i < self.action_names.len() {
                    self.action_names[i].clone()
                } else {
                    format!("{}_{}", t.from, t.on_event)
                };
                actions.push(name);
            }
        }

        // Deduplicate
        let mut seen = HashSet::new();
        actions.retain(|a| seen.insert(a.clone()));

        actions
    }

    fn generate_spec(&mut self, fairness: &[FairnessSpec]) {
        self.line("Spec ==");
        self.indent += 1;
        self.line("/\\ Init");
        self.line("/\\ [][Next]_vars");

        // Deduplicate fairness conjuncts: only emit each unique WF/SF once
        let mut emitted = std::collections::HashSet::new();
        for f in fairness {
            let fair_type = match f.kind {
                FairnessKind::Weak => "WF",
                FairnessKind::Strong => "SF",
            };
            let conjunct = format!("/\\ {}_vars(Next)", fair_type);
            if emitted.insert(conjunct.clone()) {
                self.line(&conjunct);
            }
        }

        self.indent -= 1;
        self.blank();
    }

    fn generate_type_invariant(&mut self, states: &[StateDecl]) {
        self.line("\\* Type invariant");
        self.line("TypeOK ==");
        self.indent += 1;

        // For distributed systems, check state is a function from nodes to States
        if let Some(nodes) = self.nodes.clone() {
            self.line(&format!("/\\ state \\in [{} -> States]", nodes));
        } else {
            self.line("/\\ state \\in States");
        }

        self.line("/\\ pc \\in Nat");
        self.line("\\* history: checked via HistoryConsistent (Seq(States) unsupported by Apalache)");

        // Emit variable bounds as state invariant conditions.
        // ASSUME is only valid for constants; variable bounds must live in TypeOK.
        if !self.variable_bounds.is_empty() {
            let bounds_clone = self.variable_bounds.clone();
            let mut bounds_vec: Vec<_> = bounds_clone.iter().collect();
            bounds_vec.sort_by_key(|(name, _)| (*name).clone());

            for (var_name, bounds) in bounds_vec {
                let safe_name = self.sanitize_var_name(var_name);

                if let Some(ref values) = bounds.values {
                    let values_tla: Vec<String> = values.iter()
                        .map(|v| self.expr_to_tla(v))
                        .collect();
                    self.line(&format!("/\\ {} \\in {{{}}}", safe_name, values_tla.join(", ")));
                }
                if let Some(ref min) = bounds.min {
                    let min_tla = self.expr_to_tla(min);
                    self.line(&format!("/\\ {} >= {}", safe_name, min_tla));
                }
                if let Some(ref max) = bounds.max {
                    let max_tla = self.expr_to_tla(max);
                    self.line(&format!("/\\ {} <= {}", safe_name, max_tla));
                }
            }
        }

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

            // Add terminal state invariant (action invariant: no exit from terminal states)
            self.line("\\* Once in terminal state, cannot leave");
            self.line("TerminalStable ==");
            self.indent += 1;

            // Expressed as an action invariant so Apalache can check it with --inv.
            // Semantics: if a node is in a terminal state, it must remain there after every step.
            if let Some(nodes) = self.nodes.clone() {
                self.line(&format!("\\A n \\in {} : (state[n] \\in TerminalStates) => (state'[n] \\in TerminalStates)", nodes));
            } else {
                self.line("(state \\in TerminalStates) => (state' \\in TerminalStates)");
            }

            self.indent -= 1;
            self.blank();

            // State invariant for Apalache trace generation.
            // Apalache rejects temporal properties like Liveness with --inv.
            // Use: apalache-mc simulate --inv=NotTerminated to find traces that reach terminal states.
            self.line("\\* State invariant for Apalache trace generation");
            self.line("NotTerminated ==");
            self.indent += 1;

            if let Some(nodes) = self.nodes.clone() {
                self.line(&format!("\\E n \\in {} : state[n] \\notin TerminalStates", nodes));
            } else {
                self.line("state \\notin TerminalStates");
            }

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

        let boolean_invs: Vec<_> = invariants.iter()
            .filter(|inv| !Self::is_non_boolean_expr(&inv.expr))
            .collect();

        if boolean_invs.is_empty() {
            return;
        }

        self.line("\\* User-defined invariants");
        for inv in &boolean_invs {
            self.line(&format!("Inv_{} ==", inv.name));
            self.indent += 1;
            self.line(&self.expr_to_tla(&inv.expr));
            self.indent -= 1;
            self.blank();
        }
    }

    /// Check if an expression is clearly non-boolean (record, tuple, set literal, etc.)
    /// These cannot be used as TLA+ invariants.
    fn is_non_boolean_expr(expr: &Expr) -> bool {
        matches!(expr,
            Expr::Record(_) | Expr::Tuple(_) | Expr::SetLiteral(_) |
            Expr::Except { .. } | Expr::FunctionLiteral { .. } |
            Expr::Int(_) | Expr::String(_)
        )
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

        // Generate refinement instance – active, not commented
        if behavior.refinement_map.is_some() {
            self.line(&format!(
                "Abstract == INSTANCE {} WITH state <- Abs",
                abstract_module
            ));
        } else {
            self.line(&format!(
                "Abstract == INSTANCE {}",
                abstract_module
            ));
        }
        self.blank();

        // Generate refinement theorem: Concrete_Spec => Abstract_Spec
        self.line("\\* Refinement theorem: this spec implies the abstract spec");
        self.line(&format!(
            "THEOREM RefinementCorrect == Spec => Abstract!Spec"
        ));
        self.blank();
    }

    fn generate_properties(&mut self, properties: &[TemporalProperty]) {
        if properties.is_empty() {
            return;
        }

        self.line("\\* Temporal properties (LTL)");
        for prop in properties {
            // Sanitize property name: replace angle brackets and other invalid
            // TLA+ identifier characters (from pattern type args like <Op>)
            let safe_name = prop.name.replace('<', "_").replace('>', "");
            let tla_expr = self.temporal_to_tla(&prop.expr);

            if self.config.apalache_types && Self::contains_until(&prop.expr) {
                // Apalache's Snowcat type checker rejects \U (until/weak_until).
                // Emit as a comment so the module type-checks; use TLC
                // (--mode exhaustive) to verify these properties.
                self.line(&format!(
                    "\\* TLC-only (\\\\U unsupported by Apalache): Prop_{} == {}",
                    safe_name, tla_expr
                ));
            } else {
                self.line(&format!("Prop_{} == {}", safe_name, tla_expr));
            }
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

        // For distributed systems, check that all nodes eventually reach terminal states
        if let Some(nodes) = self.nodes.clone() {
            self.line(&format!("<>(\\A n \\in {} : state[n] \\in {{{}}})", nodes, terminals.join(", ")));
        } else {
            self.line(&format!("<>(state \\in {{{}}})", terminals.join(", ")));
        }

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

        // For distributed systems, check that Next is always enabled (due to UNCHANGED vars option)
        if self.nodes.is_some() {
            self.line("[](ENABLED(Next))");
        } else if self.has_terminal_states {
            self.line("[](state \\notin TerminalStates => ENABLED(Next))");
        } else {
            self.line("[](ENABLED(Next))");
        }

        self.indent -= 1;
        self.blank();
    }

    fn generate_reachability(&mut self, states: &[StateDecl]) {
        self.line("\\* Reachability helpers for model checking");

        // For distributed systems, check if any node can reach each state
        if let Some(nodes) = self.nodes.clone() {
            for s in states {
                self.line(&format!("CanReach_{} == <>(\\E n \\in {} : state[n] = {})", s.name, nodes, s.name));
            }
        } else {
            for s in states {
                self.line(&format!("CanReach_{} == <>(state = {})", s.name, s.name));
            }
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
        self.line("\\*   Apalache trace generation: apalache-mc simulate --inv=NotTerminated");
        self.blank();
    }

    fn temporal_to_tla_action(&self, expr: &TemporalExpr) -> String {
        // Converts temporal expressions with Next into action predicates
        // This strips away Next operators and returns expressions with primed variables
        match expr {
            TemporalExpr::Next(_) => {
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
                match self.resolve_state_name(name) {
                    Some(resolved) => format!("state = {}", resolved),
                    None => "TRUE".to_string(), // semantic condition, not a state
                }
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

    /// Check whether a temporal expression contains Until or WeakUntil operators.
    ///
    /// Apalache does not support the TLA+ `\U` (until) operator. Properties that
    /// contain Until or WeakUntil must be excluded from Apalache-mode output and
    /// verified with TLC (--mode exhaustive) instead.
    fn contains_until(expr: &TemporalExpr) -> bool {
        match expr {
            TemporalExpr::Until { .. } | TemporalExpr::WeakUntil { .. } => true,
            TemporalExpr::Always(inner)
            | TemporalExpr::Eventually(inner)
            | TemporalExpr::Not(inner)
            | TemporalExpr::Next(inner) => Self::contains_until(inner),
            TemporalExpr::Release { lhs, rhs }
            | TemporalExpr::StrongRelease { lhs, rhs }
            | TemporalExpr::AlwaysImplies { premise: lhs, conclusion: rhs } => {
                Self::contains_until(lhs) || Self::contains_until(rhs)
            }
            TemporalExpr::BinOp { lhs, rhs, .. } => {
                Self::contains_until(lhs) || Self::contains_until(rhs)
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
                        match self.resolve_state_name(name) {
                            Some(resolved) => format!("state' = {}", resolved),
                            None => "TRUE".to_string(), // semantic condition, not a state
                        }
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
                match self.resolve_state_name(name) {
                    Some(resolved) => {
                        // For distributed systems, a bare state reference doesn't make sense
                        // We interpret it as "there exists a node in this state"
                        // For proper per-node properties, users should use explicit quantification
                        if self.nodes.is_some() {
                            format!("\\E n \\in {} : state[n] = {}",
                                self.nodes.as_ref().unwrap(), resolved)
                        } else {
                            format!("state = {}", resolved)
                        }
                    }
                    None => "TRUE".to_string(), // semantic condition, not a state
                }
            }
            TemporalExpr::Count(state_name) => {
                // For distributed systems with nodes, use Cardinality
                // Note: This assumes state is a function [nodes -> States]
                // For single-state machines, count is either 0 or 1
                let resolved = self.resolve_state_name(state_name)
                    .unwrap_or_else(|| state_name.clone());
                if let Some(nodes) = &self.nodes {
                    format!("Cardinality({{n \\in {} : state[n] = {}}})", nodes, resolved)
                } else {
                    format!("IF state = {} THEN 1 ELSE 0", resolved)
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
            Expr::Float(f) => {
                if self.config.apalache_types {
                    // Apalache cannot process TLA+ decimal literals: it crashes with
                    // scala.MatchError on TlaDecimal values. Scale by 1000 and emit
                    // as an integer (e.g. 0.7 → 700, 0.06 → 60).
                    format!("{}", (*f * 1000.0).round() as i64)
                } else {
                    f.to_string()
                }
            }
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
                        let path_str: Vec<String> = path.iter().map(|e| {
                            // Record field names (identifiers) must be quoted as strings
                            // in TLA+ EXCEPT syntax: [rec EXCEPT !["field"] = val]
                            match e {
                                Expr::Ident(name) => format!("[\"{}\"]", name),
                                _ => format!("[{}]", self.expr_to_tla(e)),
                            }
                        }).collect();
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
            Expr::Assume(_pred) => {
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
        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

        assert_eq!(result.module_name, "TestSystem_TestMachine");
        assert!(result.content.contains("MODULE TestSystem_TestMachine"));
        assert!(result.content.contains("VARIABLES"));
        assert!(result.content.contains("Init =="));
        assert!(result.content.contains("Next =="));
        assert!(result.content.contains("TypeOK =="));
    }

    /// Create a test generator with common state names populated.
    fn make_test_generator() -> TlaGenerator {
        let mut gen = TlaGenerator::new("Test");
        for name in &["idle", "active", "done"] {
            gen.state_names.insert(name.to_string());
        }
        gen
    }

    #[test]
    fn test_temporal_to_tla_always() {
        let gen = make_test_generator();
        let expr = TemporalExpr::Always(Box::new(TemporalExpr::State("active".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "[](state = active)");
    }

    #[test]
    fn test_temporal_to_tla_eventually() {
        let gen = make_test_generator();
        let expr = TemporalExpr::Eventually(Box::new(TemporalExpr::State("done".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "<>(state = done)");
    }

    #[test]
    fn test_temporal_to_tla_next() {
        let gen = make_test_generator();
        let expr = TemporalExpr::Next(Box::new(TemporalExpr::State("active".to_string())));
        assert_eq!(gen.temporal_to_tla(&expr), "state' = active");
    }

    #[test]
    fn test_temporal_to_tla_until() {
        let gen = make_test_generator();
        let expr = TemporalExpr::Until {
            lhs: Box::new(TemporalExpr::State("active".to_string())),
            rhs: Box::new(TemporalExpr::State("done".to_string())),
        };
        assert_eq!(gen.temporal_to_tla(&expr), "(state = active) \\U (state = done)");
    }

    #[test]
    fn test_temporal_to_tla_release() {
        let gen = make_test_generator();
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
        let gen = make_test_generator();
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
        let gen = make_test_generator();
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
        let gen = make_test_generator();
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
        assert!(result.content.contains("@type: Str;"));
        assert!(result.content.contains("@type: Int"));
        assert!(result.content.contains("@type: Seq(Str);"));
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

        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

        // Should have refinement section
        assert!(result.content.contains("REFINEMENT"));
        assert!(result.content.contains("Abs =="));
        assert!(result.content.contains("THEOREM RefinementCorrect"));
        assert!(result.content.contains("Abstract!Spec"));
        assert!(result.content.contains("Abstract == INSTANCE AbstractSpec WITH state <- Abs"));
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
            "Cardinality({n \\in replicas : state[n] = leader})"
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
            "(Cardinality({n \\in replicas : state[n] = leader})) <= (1)"
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
            "(Cardinality({n \\in replicas : state[n] = failed})) /= (0)"
        );
    }

    #[test]
    fn test_generate_with_nodes() {
        let mut behavior = make_test_behavior();
        behavior.nodes = Some("replicas".to_string());

        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

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

        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

        // Should have FiniteSets extension
        assert!(result.content.contains("FiniteSets"));
        // Should have Cardinality in property
        assert!(result.content.contains("Cardinality({n \\in replicas : state[n] = leader})"));
        assert!(result.content.contains("Prop_single_leader"));
    }

    #[test]
    fn test_variable_bounds() {
        use crate::parser::ast::{VariableDecl, ValueBounds};

        let mut behavior = make_test_behavior();
        behavior.variables = vec![
            VariableDecl {
                name: "counter".to_string(),
                type_name: "Int".to_string(),
                initial_value: Some(Expr::Int(0)),
                bounds: Some(ValueBounds {
                    min: Some(Expr::Int(0)),
                    max: Some(Expr::Int(100)),
                    values: None,
                }),
            },
            VariableDecl {
                name: "status".to_string(),
                type_name: "String".to_string(),
                initial_value: Some(Expr::String("pending".to_string())),
                bounds: Some(ValueBounds {
                    min: None,
                    max: None,
                    values: Some(vec![
                        Expr::String("pending".to_string()),
                        Expr::String("active".to_string()),
                        Expr::String("done".to_string()),
                    ]),
                }),
            },
        ];

        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

        // Bounds belong in TypeOK (state invariant), not ASSUME (constants only)
        assert!(!result.content.contains("ASSUME counter >= 0"), "bounds must not use ASSUME");
        assert!(result.content.contains("/\\ counter >= 0"));
        assert!(result.content.contains("/\\ counter <= 100"));
        assert!(result.content.contains("/\\ status \\in {\"pending\", \"active\", \"done\"}"));

        // Should initialize with bounds-compliant values
        assert!(result.content.contains("/\\ counter = 0"));
        assert!(result.content.contains("/\\ status = \"pending\""));
    }

    #[test]
    fn test_message_queues() {
        let mut behavior = make_test_behavior();

        // Add a transition with Send effect
        behavior.transitions.push(TransitionDecl {
            from: TransitionSource::State("idle".to_string()),
            to: TransitionTarget::State("active".to_string()),
            on_event: "create".to_string(),
            guard: None,
            effects: vec![
                EffectStmt {
                    kind: EffectKind::Send {
                        channel: "PaymentService".to_string(),
                        message: "PaymentRequested".to_string(),
                        args: vec![
                            Expr::Int(100),
                            Expr::String("order123".to_string()),
                        ],
                    },
                },
            ],
            timing: None,
            span: Span { start: 0, end: 0 },
        });

        let result = generate(&behavior, "TestSystem", Path::new("."), None).unwrap();

        println!("Generated TLA+ with message queues:\n{}", result.content);

        // Should have message queue variable
        assert!(result.content.contains("PaymentService_queue"));
        // Should initialize queue to empty
        assert!(result.content.contains("/\\ PaymentService_queue = <<>>"));
        // Should have queue in vars tuple
        assert!(result.content.contains("PaymentService_queue"));
        // Should generate send operation
        assert!(result.content.contains("PaymentService_queue'"));
        assert!(result.content.contains("type |-> \"PaymentRequested\""));
    }
}
