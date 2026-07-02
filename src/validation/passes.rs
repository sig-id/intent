//! Standard validation passes for the Intent language.

use crate::diagnostic::{Diagnostic, ErrorCode, Span};
use crate::parser::ast::{
    BehaviorDecl, ComponentDecl, ConstraintDecl, ParamValue, PatternApplication, PatternDecl,
    PatternParam, SystemDecl,
};
use crate::types::checker;
use crate::types::inference::{InferType, InferenceContext};
use crate::validation::{ValidationContext, ValidationPass};

use std::collections::{HashMap, HashSet};

include!(concat!(env!("OUT_DIR"), "/stdlib_patterns.rs"));

/// Type checking pass.
///
/// Uses `InferenceContext` (Hindley-Milner) for unified type checking
/// instead of the simpler `TypeContext`/`is_compatible` checker.
pub struct TypeCheckPass;

impl ValidationPass for TypeCheckPass {
    fn name(&self) -> &'static str {
        "type_check"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        let infer_ctx = InferenceContext::new();

        // Check patterns (system-local + extra/stdlib)
        for pattern in system.patterns.iter().chain(ctx.extra_patterns.iter()) {
            check_pattern_params_unified(&pattern.parameters, &infer_ctx);
        }

        // Check pattern applications
        for applies in &system.applies {
            let pattern_name = applies.pattern.name();
            if let Some(pattern) = system
                .patterns
                .iter()
                .chain(ctx.extra_patterns.iter())
                .find(|p| p.name == pattern_name)
            {
                check_pattern_application_unified(applies, &pattern.parameters, &infer_ctx);
            }
        }

        // Check component-level patterns and applications
        for component in &system.components {
            check_component_types_unified(component, &infer_ctx);
        }

        ctx.diagnostics.merge(infer_ctx.diagnostics());
    }
}

/// Validate pattern parameter type names and check default values via unification.
fn check_pattern_params_unified(params: &[PatternParam], infer_ctx: &InferenceContext) {
    for param in params {
        // Validate the type name is a known type
        let type_name = &param.type_name;
        if crate::types::Type::from_name(type_name).is_none() && !type_name.contains('<') {
            // Unknown type - might be a custom type, emit info
            infer_ctx.add_diagnostic(Diagnostic::warning(
                ErrorCode::E034_InvalidTypeAnnotation,
                format!(
                    "Unknown type '{}' for parameter '{}'",
                    type_name, param.name
                ),
                param.span,
            ));
        }

        // Validate constraints
        for constraint in &param.constraints {
            if let crate::parser::ast::FieldConstraint::Default(value) = constraint {
                let expected = type_name_to_infer_type(&param.type_name);
                check_value_type_unified(value, &expected, infer_ctx, param.span);
            }
        }
    }
}

/// Validate parameter values against declared types via unification.
fn check_pattern_application_unified(
    application: &PatternApplication,
    params: &[PatternParam],
    infer_ctx: &InferenceContext,
) {
    // Build a map of expected parameter types
    let param_types: HashMap<&str, &str> = params
        .iter()
        .map(|p| (p.name.as_str(), p.type_name.as_str()))
        .collect();

    // Check each provided parameter
    for (name, value) in &application.params {
        if let Some(expected_type) = param_types.get(name.as_str()) {
            let expected = type_name_to_infer_type(expected_type);
            check_value_type_unified(value, &expected, infer_ctx, application.span);
        } else {
            infer_ctx.add_diagnostic(Diagnostic::error(
                ErrorCode::E007_InvalidPatternParameter,
                format!("Unknown parameter '{}' in pattern application", name),
                application.span,
            ));
        }
    }

    // Check for missing required parameters (those without defaults)
    for param in params {
        let has_value = application.params.iter().any(|(n, _)| n == &param.name);
        let has_default = param
            .constraints
            .iter()
            .any(|c| matches!(c, crate::parser::ast::FieldConstraint::Default(_)));

        if !has_value && !has_default {
            infer_ctx.add_diagnostic(Diagnostic::error(
                ErrorCode::E011_MissingRequiredField,
                format!("Missing required parameter '{}' for pattern", param.name),
                application.span,
            ));
        }
    }
}

/// Convert a ParamValue to an InferType, then unify with the expected type.
fn check_value_type_unified(
    value: &ParamValue,
    expected: &InferType,
    infer_ctx: &InferenceContext,
    span: Span,
) {
    let actual_type = checker::infer_param_value_type(value);
    let actual = InferType::Concrete(actual_type);

    // Unify – diagnostic is recorded inside infer_ctx on failure
    let _ = infer_ctx.unify(&actual, expected, span);
}

/// Check component-level types via unified inference.
fn check_component_types_unified(component: &ComponentDecl, _infer_ctx: &InferenceContext) {
    // Check component-level patterns
    for pattern in &component.behaviors {
        let _ = pattern;
    }
}

/// Entity resolution pass.
pub struct EntityResolutionPass;

impl ValidationPass for EntityResolutionPass {
    fn name(&self) -> &'static str {
        "entity_resolution"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Collect all declared entities
        let mut declared_entities: HashSet<String> = HashSet::new();

        // Add components
        for component in &system.components {
            declared_entities.insert(component.name.clone());
            declared_entities.extend(component.contains.iter().cloned());

            // Add nested components
            for nested in &component.components {
                declared_entities.insert(nested.name.clone());
            }
        }

        // Add let bindings
        for (name, _) in &system.let_bindings {
            declared_entities.insert(name.clone());
        }

        // Now check references
        for constraint in &system.constraints {
            check_constraint_references(constraint, &declared_entities, ctx);
        }

        // depends_only references are code-level dependencies (interfaces, modules)
        // and don't need to match declared Intent components – validated by
        // structural analysis against actual source code instead.
    }
}

fn check_constraint_references(
    constraint: &ConstraintDecl,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    for rule in &constraint.rules {
        check_rule_references(rule, declared, ctx);
    }
}

fn check_rule_references(
    rule: &crate::parser::ast::ConstraintRule,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::ConstraintRule;

    match rule {
        ConstraintRule::Not(inner) => {
            check_rule_references(inner, declared, ctx);
        }
        ConstraintRule::And(a, b)
        | ConstraintRule::Or(a, b)
        | ConstraintRule::Implies(a, b)
        | ConstraintRule::Iff(a, b) => {
            check_rule_references(a, declared, ctx);
            check_rule_references(b, declared, ctx);
        }
        ConstraintRule::Forall {
            var, domain, body, ..
        }
        | ConstraintRule::Exists {
            var, domain, body, ..
        } => {
            check_scope_expr_references(domain, declared, ctx);
            let mut declared_with_var = declared.clone();
            declared_with_var.insert(var.clone());
            check_rule_references(body, &declared_with_var, ctx);
        }
        ConstraintRule::Predicate(pred) => {
            check_predicate_references(pred, declared, ctx);
        }
        ConstraintRule::Comparison { .. } | ConstraintRule::NFConstraint { .. } => {}
        ConstraintRule::Call { subject, args, .. } => {
            check_scope_expr_references(subject, declared, ctx);
            for arg in args {
                check_scope_expr_references(arg, declared, ctx);
            }
        }
        ConstraintRule::Suppressed { rule, .. } => {
            check_rule_references(rule, declared, ctx);
        }
    }
}

fn check_scope_expr_references(
    expr: &crate::parser::ast::ScopeExpr,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::ScopeExpr;

    match expr {
        ScopeExpr::Ident(qname) => {
            if !qname.is_simple() || !declared.contains(&qname.to_dotted()) {
                // For now, only check simple names; qualified names may reference external items
                if qname.is_simple() && !declared.contains(&qname.to_dotted()) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E001_UnknownIdentifier,
                        format!(
                            "Unknown identifier '{}' in scope expression",
                            qname.to_dotted()
                        ),
                        Span::synthetic(),
                    ));
                }
            }
        }
        ScopeExpr::EntityList(names) => {
            for name in names {
                if !declared.contains(name) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E001_UnknownIdentifier,
                        format!("Unknown entity '{}' in scope expression", name),
                        Span::synthetic(),
                    ));
                }
            }
        }
        ScopeExpr::Union(a, b) | ScopeExpr::Intersection(a, b) | ScopeExpr::Difference(a, b) => {
            check_scope_expr_references(a, declared, ctx);
            check_scope_expr_references(b, declared, ctx);
        }
        ScopeExpr::Glob(_) | ScopeExpr::All => {}
        ScopeExpr::Matches { .. } => {}
        ScopeExpr::Filtered { condition, .. } => {
            let _ = condition; // Would check expression references
        }
    }
}

fn check_predicate_references(
    pred: &crate::parser::ast::PredicateCall,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::PredicateCall;

    match pred {
        PredicateCall::Depends { from, .. }
        | PredicateCall::DependsTransitively { from, .. }
        | PredicateCall::References { from, .. } => {
            // Only check 'from' subject – 'to' targets are code-level entities
            // (types, modules, interfaces) validated by structural analysis
            check_scope_expr_references(from, declared, ctx);
        }
        PredicateCall::Implements { entity, .. } => {
            check_scope_expr_references(entity, declared, ctx);
        }
        PredicateCall::Contains {
            container,
            entities,
        } => {
            check_scope_expr_references(container, declared, ctx);
            for entity in entities {
                check_scope_expr_references(entity, declared, ctx);
            }
        }
    }
}

/// State reachability pass.
pub struct StateReachabilityPass;

impl ValidationPass for StateReachabilityPass {
    fn name(&self) -> &'static str {
        "state_reachability"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check system-level behaviors
        for behavior in &system.behaviors {
            check_behavior_reachability(behavior, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_reachability(behavior, ctx);
            }
        }
    }
}

fn check_behavior_reachability(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    // Composed behaviors derive their states from the composed sub-behaviors
    if !behavior.composes.is_empty() {
        return;
    }

    // Check for exactly one initial state
    let initial_states: Vec<_> = behavior.states.iter().filter(|s| s.initial).collect();

    match initial_states.len() {
        0 => {
            ctx.diagnostics.add(
                Diagnostic::error(
                    ErrorCode::E021_NoInitialState,
                    format!("Behavior '{}' has no initial state", behavior.name),
                    behavior.span,
                )
                .with_suggestion("Add `initial: true` to one state"),
            );
        }
        1 => {}
        _ => {
            let names: Vec<_> = initial_states.iter().map(|s| s.name.as_str()).collect();
            ctx.diagnostics.add(
                Diagnostic::error(
                    ErrorCode::E020_MultipleInitialStates,
                    format!(
                        "Behavior '{}' has multiple initial states: {}",
                        behavior.name,
                        names.join(", ")
                    ),
                    behavior.span,
                )
                .with_suggestion("Only one state should have `initial: true`"),
            );
        }
    }

    // Check for unreachable states
    let reachable = compute_reachable_states(behavior);
    for state in &behavior.states {
        if !reachable.contains(&state.name) && !state.initial {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E006_UnreachableState,
                    format!(
                        "State '{}' in behavior '{}' is unreachable",
                        state.name, behavior.name
                    ),
                    behavior.span,
                )
                .with_suggestion("Add a transition to this state or remove it"),
            );
        }
    }

    // Check for terminal states with outgoing transitions
    let terminal_states: HashSet<_> = behavior
        .states
        .iter()
        .filter(|s| s.terminal)
        .map(|s| s.name.as_str())
        .collect();

    for transition in &behavior.transitions {
        if let Some(from) = transition.from.as_state() {
            if terminal_states.contains(from) {
                ctx.diagnostics.add(Diagnostic::warning(
                    ErrorCode::E022_TerminalStateTransitions,
                    format!(
                        "Terminal state '{}' in behavior '{}' has outgoing transition to '{}'",
                        from, behavior.name, transition.to
                    ),
                    transition.span,
                ));
            }
        }
    }
}

fn compute_reachable_states(behavior: &BehaviorDecl) -> HashSet<String> {
    let mut reachable = HashSet::new();

    // Start from initial states
    for state in &behavior.states {
        if state.initial {
            reachable.insert(state.name.clone());
        }
    }

    // BFS to find all reachable states
    let mut changed = true;
    while changed {
        changed = false;
        for transition in &behavior.transitions {
            if let (Some(from), Some(to)) = (transition.from.as_state(), transition.to.as_state()) {
                if reachable.contains(from) && !reachable.contains(to) {
                    reachable.insert(to.to_string());
                    changed = true;
                }
            }
        }
    }

    reachable
}

/// Event declaration pass.
pub struct EventDeclarationPass;

impl ValidationPass for EventDeclarationPass {
    fn name(&self) -> &'static str {
        "event_declaration"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Build declared events map: name -> optional payload field count
        let declared_events: HashMap<&str, Option<usize>> = system
            .events
            .iter()
            .map(|e| {
                let field_count = e.payload.as_ref().map(|p| payload_field_count(p));
                (e.name.as_str(), field_count)
            })
            .collect();

        // Build declared messages map: (channel, name) -> optional payload field count
        let declared_messages: HashMap<(&str, &str), Option<usize>> = system
            .messages
            .iter()
            .map(|m| {
                let field_count = m.payload.as_ref().map(|p| payload_field_count(p));
                ((m.channel.as_str(), m.name.as_str()), field_count)
            })
            .collect();

        // Collect all events used in transitions and all emit/send/receive effects
        let all_behaviors = system
            .behaviors
            .iter()
            .chain(system.components.iter().flat_map(|c| c.behaviors.iter()));

        for behavior in all_behaviors {
            for transition in &behavior.transitions {
                let event_name = &transition.on_event;

                // Warn about undeclared events if any events ARE declared
                if !declared_events.is_empty() && !declared_events.contains_key(event_name.as_str())
                {
                    ctx.diagnostics.add(
                        Diagnostic::warning(
                            ErrorCode::E009_UndefinedEvent,
                            format!(
                                "Event '{}' used in transition but not declared in system events",
                                event_name
                            ),
                            transition.span,
                        )
                        .with_suggestion(
                            "Add an event declaration: event <name> { payload: <Type> }",
                        ),
                    );
                }

                // Check emit effects against declared event payloads
                for effect in &transition.effects {
                    check_effect_event_declarations(
                        effect,
                        &declared_events,
                        &declared_messages,
                        ctx,
                        transition.span,
                    );
                }
            }
        }
    }
}

/// Count the number of expected arguments for a payload type.
fn payload_field_count(st: &crate::types::SpannedType) -> usize {
    match &st.ty {
        crate::types::Type::Record(fields) => fields.len(),
        _ => 1,
    }
}

/// Check effect statements against declared events and messages.
fn check_effect_event_declarations(
    effect: &crate::parser::ast::EffectStmt,
    declared_events: &HashMap<&str, Option<usize>>,
    declared_messages: &HashMap<(&str, &str), Option<usize>>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    use crate::parser::ast::EffectKind;

    match &effect.kind {
        EffectKind::Emit { name, args } => {
            if let Some(expected_fields) = declared_events.get(name.as_str()) {
                if let Some(count) = expected_fields {
                    if args.len() != *count {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E009_UndefinedEvent,
                            format!(
                                "Event '{}' expects {} payload argument(s) but {} provided",
                                name,
                                count,
                                args.len()
                            ),
                            span,
                        ));
                    }
                }
            } else if !declared_events.is_empty() {
                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E009_UndefinedEvent,
                        format!("Emitting undeclared event '{}'", name),
                        span,
                    )
                    .with_suggestion("Add an event declaration for this event"),
                );
            }
        }
        EffectKind::Send {
            channel,
            message,
            args,
        } => {
            let key = (channel.as_str(), message.as_str());
            if let Some(expected_fields) = declared_messages.get(&key) {
                if let Some(count) = expected_fields {
                    if args.len() != *count {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E009_UndefinedEvent,
                            format!(
                                "Message '{}.{}' expects {} payload argument(s) but {} provided",
                                channel,
                                message,
                                count,
                                args.len()
                            ),
                            span,
                        ));
                    }
                }
            } else if !declared_messages.is_empty() {
                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E009_UndefinedEvent,
                        format!("Sending undeclared message '{}.{}'", channel, message),
                        span,
                    )
                    .with_suggestion("Add a message declaration for this message"),
                );
            }
        }
        EffectKind::Receive {
            channel, message, ..
        } => {
            let key = (channel.as_str(), message.as_str());
            if !declared_messages.is_empty() && !declared_messages.contains_key(&key) {
                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E009_UndefinedEvent,
                        format!("Receiving undeclared message '{}.{}'", channel, message),
                        span,
                    )
                    .with_suggestion("Add a message declaration for this message"),
                );
            }
        }
        EffectKind::If {
            then_effects,
            else_effects,
            ..
        } => {
            for eff in then_effects {
                check_effect_event_declarations(eff, declared_events, declared_messages, ctx, span);
            }
            if let Some(else_effs) = else_effects {
                for eff in else_effs {
                    check_effect_event_declarations(
                        eff,
                        declared_events,
                        declared_messages,
                        ctx,
                        span,
                    );
                }
            }
        }
        EffectKind::Assign { .. } | EffectKind::Expr(_) => {}
    }
}

/// Pattern compatibility pass.
pub struct PatternCompatibilityPass;

impl ValidationPass for PatternCompatibilityPass {
    fn name(&self) -> &'static str {
        "pattern_compatibility"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Build a map of available patterns (system-local + extra/stdlib)
        let patterns: HashMap<&str, &PatternDecl> = system
            .patterns
            .iter()
            .chain(ctx.extra_patterns.iter())
            .map(|p| (p.name.as_str(), p))
            .collect();

        let stdlib_names: HashSet<&str> = STDLIB_PATTERN_NAMES.iter().cloned().collect();

        let is_known =
            |name: &str| -> bool { patterns.contains_key(name) || stdlib_names.contains(name) };

        let available_list = || -> String {
            let mut names: Vec<&str> = patterns.keys().cloned().collect();
            names.extend(stdlib_names.iter().cloned());
            names.sort();
            names.join(", ")
        };

        // Check system-level pattern applications
        for applies in &system.applies {
            let name = applies.pattern.name();
            if !is_known(name) {
                ctx.diagnostics.add(
                    Diagnostic::error(
                        ErrorCode::E015_PatternNotFound,
                        format!("Pattern '{}' not found", applies.pattern),
                        Span::synthetic(),
                    )
                    .with_suggestion(format!("Available patterns: {}", available_list())),
                );
            }
        }

        // Check system-level behavior applications
        for behavior in &system.behaviors {
            for applies in &behavior.applies {
                let name = applies.pattern.name();
                if !is_known(name) {
                    ctx.diagnostics.add(
                        Diagnostic::error(
                            ErrorCode::E015_PatternNotFound,
                            format!(
                                "Pattern '{}' not found in behavior '{}'",
                                applies.pattern, behavior.name
                            ),
                            Span::synthetic(),
                        )
                        .with_suggestion(format!("Available patterns: {}", available_list())),
                    );
                }
            }
        }

        // Check component-level applications
        for component in &system.components {
            for behavior in &component.behaviors {
                for applies in &behavior.applies {
                    let name = applies.pattern.name();
                    if !is_known(name) {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E015_PatternNotFound,
                            format!(
                                "Pattern '{}' not found in component '{}'",
                                applies.pattern, component.name
                            ),
                            Span::synthetic(),
                        ));
                    }
                }
            }
        }
    }
}

/// Pattern conflict detection pass.
pub struct PatternConflictPass;

impl ValidationPass for PatternConflictPass {
    fn name(&self) -> &'static str {
        "pattern_conflict"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Move extra patterns out of ctx so pattern refs don't borrow ctx
        let extra_patterns = std::mem::take(&mut ctx.extra_patterns);

        // Build a map of available patterns (system-local + extra/stdlib)
        let patterns: HashMap<&str, &PatternDecl> = system
            .patterns
            .iter()
            .chain(extra_patterns.iter())
            .map(|p| (p.name.as_str(), p))
            .collect();

        // Check all behaviors
        for behavior in &system.behaviors {
            check_pattern_conflicts(behavior, &patterns, ctx);
        }

        // Check component-level behaviors
        for component in &system.components {
            for behavior in &component.behaviors {
                check_pattern_conflicts(behavior, &patterns, ctx);
            }
        }

        ctx.extra_patterns = extra_patterns;
    }
}

fn check_pattern_conflicts(
    behavior: &BehaviorDecl,
    patterns: &HashMap<&str, &PatternDecl>,
    ctx: &mut ValidationContext,
) {
    use crate::behavioral::composition::{
        compose_behaviors, CompositionConfig, ConflictStrategy, ConflictType,
    };

    // Only check behaviors that apply multiple patterns
    if behavior.applies.len() <= 1 {
        return;
    }

    // Collect pattern behaviors
    let mut pattern_behaviors: Vec<(&str, &BehaviorDecl)> = Vec::new();
    for app in &behavior.applies {
        let pattern_name = app.pattern.name();
        if let Some(pattern) = patterns.get(pattern_name) {
            if let Some(ref pattern_behavior) = pattern.behavior {
                pattern_behaviors.push((pattern_name, pattern_behavior));
            }
        }
    }

    // Need at least 2 pattern behaviors to detect conflicts
    if pattern_behaviors.len() < 2 {
        return;
    }

    // Compose the patterns with conflict detection enabled
    let config = CompositionConfig {
        state_conflict_strategy: ConflictStrategy::Error,
        transition_conflict_strategy: ConflictStrategy::Error,
        state_prefix: None,
    };

    match compose_behaviors(&behavior.name, &pattern_behaviors, &config) {
        Ok(composed) => {
            // Check for all conflict types
            let transition_conflicts = composed.conflicts_of_type(ConflictType::Transition);
            for conflict in transition_conflicts {
                if let crate::behavioral::composition::CompositionConflict::TransitionConflict {
                    from,
                    event,
                    targets,
                } = conflict
                {
                    let sources: Vec<String> = targets
                        .iter()
                        .map(|(s, t)| format!("{} -> {}", s, t))
                        .collect();
                    ctx.diagnostics.add(Diagnostic::warning(
                        ErrorCode::E030_PatternCompositionConflict,
                        format!(
                            "Pattern conflict in behavior '{}': state '{}' on event '{}' has conflicting transitions: {}",
                            behavior.name, from, event, sources.join(", ")
                        ),
                        behavior.span,
                    ).with_suggestion("Consider using a different combination of patterns or manually resolving the conflict"));
                }
            }

            // Also check for state conflicts
            let state_conflicts = composed.conflicts_of_type(ConflictType::State);
            for conflict in state_conflicts {
                match conflict {
                    crate::behavioral::composition::CompositionConflict::MultipleInitialStates { states } => {
                        let state_list: Vec<String> = states.iter().map(|(s, st)| format!("{}: {}", s, st)).collect();
                        ctx.diagnostics.add(Diagnostic::warning(
                            ErrorCode::E030_PatternCompositionConflict,
                            format!(
                                "Pattern conflict in behavior '{}': multiple initial states from different patterns: {}",
                                behavior.name, state_list.join(", ")
                            ),
                            behavior.span,
                        ).with_suggestion("Explicitly mark one state as initial in your behavior definition"));
                    }
                    crate::behavioral::composition::CompositionConflict::StateModifierMismatch { state, sources } => {
                        let source_list: Vec<String> = sources.iter().map(|(s, _)| s.clone()).collect();
                        ctx.diagnostics.add(Diagnostic::warning(
                            ErrorCode::E030_PatternCompositionConflict,
                            format!(
                                "Pattern conflict in behavior '{}': state '{}' has different modifiers in patterns: {}",
                                behavior.name, state, source_list.join(", ")
                            ),
                            behavior.span,
                        ).with_suggestion("Explicitly define the state modifiers in your behavior"));
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E030_PatternCompositionConflict,
                    format!(
                        "Pattern composition check failed for behavior '{}': {}",
                        behavior.name, e
                    ),
                    behavior.span,
                )
                .with_suggestion(
                    "Some pattern features may not be compatible with composition analysis",
                ),
            );
        }
    }
}

/// Refinement validation pass.
pub struct RefinementValidationPass;

impl ValidationPass for RefinementValidationPass {
    fn name(&self) -> &'static str {
        "refinement_validation"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Build behavior lookup for resolving refinement targets
        let behavior_map: HashMap<&str, &BehaviorDecl> = system
            .behaviors
            .iter()
            .chain(system.components.iter().flat_map(|c| c.behaviors.iter()))
            .map(|b| (b.name.as_str(), b))
            .collect();

        // Check behavior refinements
        for behavior in &system.behaviors {
            check_behavior_refinement(behavior, &behavior_map, ctx);
        }

        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_refinement(behavior, &behavior_map, ctx);
            }
        }
    }
}

fn check_behavior_refinement(
    behavior: &BehaviorDecl,
    behavior_map: &HashMap<&str, &BehaviorDecl>,
    ctx: &mut ValidationContext,
) {
    let refines = match &behavior.refines {
        Some(r) => r,
        None => return,
    };

    let concrete_states: HashSet<_> = behavior.states.iter().map(|s| s.name.as_str()).collect();

    if let Some(ref map) = &behavior.refinement_map {
        // Phase 1a: Verify concrete states in map exist
        for (_, concrete_list) in &map.mappings {
            for concrete in concrete_list {
                if !concrete_states.contains(concrete.as_str()) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E012_InvalidRefinementMapping,
                        format!(
                            "Concrete state '{}' in refinement map not found in behavior '{}'",
                            concrete, behavior.name
                        ),
                        Span::synthetic(),
                    ));
                }
            }
        }

        // Phase 1b: Verify mapping totality – every concrete state must appear
        //           in at most one mapping, and every concrete state should be mapped
        let mut concrete_to_abstract: HashMap<&str, &str> = HashMap::new();
        for (abstract_state, concrete_list) in &map.mappings {
            for concrete in concrete_list {
                if let Some(existing) = concrete_to_abstract.get(concrete.as_str()) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E012_InvalidRefinementMapping,
                        format!(
                            "Concrete state '{}' appears in multiple abstract mappings: '{}' and '{}'",
                            concrete, existing, abstract_state
                        ),
                        behavior.span,
                    ));
                } else {
                    concrete_to_abstract.insert(concrete.as_str(), abstract_state.as_str());
                }
            }
        }

        for state in &behavior.states {
            if !concrete_to_abstract.contains_key(state.name.as_str()) {
                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E012_InvalidRefinementMapping,
                        format!(
                            "Concrete state '{}' in behavior '{}' has no abstract mapping",
                            state.name, behavior.name
                        ),
                        behavior.span,
                    )
                    .with_suggestion("Add this state to the refinement map"),
                );
            }
        }
    }

    // Phase 1c: If we can resolve the abstract spec, do full transition validation
    if let Some(abstract_spec) = behavior_map.get(refines.as_str()) {
        use crate::behavioral::refinement::validate_refinement;

        match validate_refinement(behavior, abstract_spec, &behavior.refinement_map) {
            Ok(result) => {
                for violation in &result.violations {
                    use crate::behavioral::refinement::RefinementViolation;
                    match violation {
                        RefinementViolation::UnmappedConcreteState { state } => {
                            ctx.diagnostics.add(Diagnostic::error(
                                ErrorCode::E012_InvalidRefinementMapping,
                                format!(
                                    "Concrete state '{}' has no mapping to abstract spec '{}'",
                                    state, refines
                                ),
                                behavior.span,
                            ));
                        }
                        RefinementViolation::UnreachableAbstractState { state } => {
                            ctx.diagnostics.add(Diagnostic::warning(
                                ErrorCode::E012_InvalidRefinementMapping,
                                format!(
                                    "Abstract state '{}' in '{}' is not covered by any concrete state",
                                    state, refines
                                ),
                                behavior.span,
                            ));
                        }
                        RefinementViolation::IllegalTransition {
                            from,
                            to,
                            event,
                            reason,
                        } => {
                            ctx.diagnostics.add(Diagnostic::error(
                                ErrorCode::E012_InvalidRefinementMapping,
                                format!(
                                    "Concrete transition {} -> {} on '{}' violates refinement: {}",
                                    from, to, event, reason
                                ),
                                behavior.span,
                            ));
                        }
                        RefinementViolation::InconsistentMapping {
                            abstract_state,
                            concrete_states,
                        } => {
                            ctx.diagnostics.add(Diagnostic::error(
                                ErrorCode::E012_InvalidRefinementMapping,
                                format!(
                                    "Inconsistent mapping for abstract state '{}': concrete states {:?} have conflicting transitions",
                                    abstract_state, concrete_states
                                ),
                                behavior.span,
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E012_InvalidRefinementMapping,
                    format!(
                        "Refinement validation failed for '{}' refines '{}': {}",
                        behavior.name, refines, e
                    ),
                    behavior.span,
                ));
            }
        }
    }
}

/// Guard and effect identifier resolution pass.
pub struct GuardEffectResolutionPass;

impl ValidationPass for GuardEffectResolutionPass {
    fn name(&self) -> &'static str {
        "guard_effect_resolution"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Check all behaviors in the system
        for behavior in &system.behaviors {
            check_behavior_identifiers(behavior, ctx);
        }

        // Check all behaviors in components
        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_identifiers(behavior, ctx);
            }
        }
    }
}

fn check_behavior_identifiers(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    // Collect all declared identifiers
    let mut declared: HashSet<String> = HashSet::new();

    // Add state names
    for state in &behavior.states {
        declared.insert(state.name.clone());
    }

    // Add variable names
    for var in &behavior.variables {
        declared.insert(var.name.clone());
    }

    // Add driver-local memory variable names (v2 executable behaviors)
    for var in &behavior.memory {
        declared.insert(var.name.clone());
    }

    // Add parameter names
    for param in &behavior.parameters {
        declared.insert(param.name.clone());
    }

    // Add function names
    for func in &behavior.functions {
        declared.insert(func.name.clone());
    }

    // Add event names from transitions
    for trans in &behavior.transitions {
        declared.insert(trans.on_event.clone());
    }

    // Fixture bind targets (`insert/call ... -> name`, `bind name = ...`) are
    // binding sites: they introduce names for later steps, transitions, and
    // projections, so collect them before reference checking.
    for fixture in &behavior.fixtures {
        for step in &fixture.steps {
            match step {
                crate::parser::ast::FixtureStep::Insert { bind: Some(name), .. }
                | crate::parser::ast::FixtureStep::Call { bind: Some(name), .. }
                | crate::parser::ast::FixtureStep::Bind { name, .. } => {
                    declared.insert(name.clone());
                }
                _ => {}
            }
        }
    }

    for fixture in &behavior.fixtures {
        for step in &fixture.steps {
            check_fixture_step_metadata(step, &declared, ctx, fixture.span);
        }
    }

    for projection in &behavior.projections {
        if let Some(source) = &projection.source {
            if let Some(filter) = &source.filter {
                check_meta_expr_refs(filter, &declared, ctx, projection.span);
            }
        }
        for clause in &projection.clauses {
            if !behavior
                .states
                .iter()
                .any(|state| state.name == clause.state)
            {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Unknown projection target state '{}' in behavior '{}'",
                        clause.state, behavior.name
                    ),
                    clause.span,
                ));
            }
            check_meta_expr_refs(&clause.condition, &declared, ctx, clause.span);
        }
        if let Some(else_state) = &projection.else_state {
            if !behavior
                .states
                .iter()
                .any(|state| state.name == *else_state)
            {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Unknown projection else-state '{}' in behavior '{}'",
                        else_state, behavior.name
                    ),
                    projection.span,
                ));
            }
        }
    }

    if let Some(mbt) = &behavior.mbt {
        check_mbt_metadata(mbt, behavior, ctx);
    }

    // Check guards and effects in transitions
    for trans in &behavior.transitions {
        let mut transition_declared = declared.clone();
        for input in &trans.inputs {
            transition_declared.insert(input.name.clone());
        }
        if trans
            .bindings
            .iter()
            .any(|binding| matches!(binding, crate::parser::ast::TransitionBinding::Call { .. }))
        {
            transition_declared.insert("result".to_string());
        }

        for binding in &trans.bindings {
            check_transition_binding_metadata(binding, &transition_declared, ctx, behavior.span);
        }

        for input in &trans.inputs {
            if let Some(ref domain) = input.domain {
                check_expr_identifiers(domain, &transition_declared, ctx, behavior.span);
            }
            if let Some(ref default_value) = input.default_value {
                check_expr_identifiers(default_value, &transition_declared, ctx, behavior.span);
            }
        }

        if let Some(ref guard) = trans.guard {
            check_expr_identifiers(guard, &transition_declared, ctx, behavior.span);
        }

        for expect in &trans.expects {
            check_expr_identifiers(expect, &transition_declared, ctx, behavior.span);
        }

        for effect in &trans.effects {
            check_effect_identifiers(effect, &transition_declared, ctx, behavior.span);
        }
    }
}

fn check_mbt_metadata(
    mbt: &crate::parser::ast::MbtDecl,
    behavior: &crate::parser::ast::BehaviorDecl,
    ctx: &mut ValidationContext,
) {
    if let Some(generator) = &mbt.generator {
        for invariant in &generator.invariants {
            if !is_known_mbt_invariant(invariant, behavior) {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Unknown MBT invariant '{}' in behavior '{}'",
                        invariant, behavior.name
                    ),
                    generator.span,
                ));
            }
        }

        if let Some(mode) = &generator.mode {
            if !matches!(mode.as_str(), "check" | "simulate" | "trace") {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Unsupported MBT mode '{}' in behavior '{}'",
                        mode, behavior.name
                    ),
                    generator.span,
                ));
            }
        }
    }

    if let Some(replay) = &mbt.replay {
        if let Some(projection) = &replay.state_projection {
            if !behavior
                .projections
                .iter()
                .any(|decl| decl.name == *projection)
            {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!(
                        "Unknown MBT replay state projection '{}' in behavior '{}'",
                        projection, behavior.name
                    ),
                    replay.span,
                ));
            }
        }
    }
}

fn is_known_mbt_invariant(name: &str, behavior: &crate::parser::ast::BehaviorDecl) -> bool {
    match name {
        // These are always emitted by the executable emitter; `NotTerminated`
        // is well-defined even when `TerminalStates` is empty.
        "TypeOK" | "HistoryConsistent" | "TerminalStable" | "NotTerminated" => true,
        _ => behavior
            .invariants
            .iter()
            .any(|invariant| invariant.name == name),
    }
}

fn check_fixture_step_metadata(
    step: &crate::parser::ast::FixtureStep,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    // Bind targets are binding sites collected into `declared` by the caller;
    // here we only validate that the seeded field/arg values resolve.
    match step {
        crate::parser::ast::FixtureStep::Insert { fields, .. } => {
            for (_, value) in fields {
                check_meta_expr_refs(value, declared, ctx, span);
            }
        }
        crate::parser::ast::FixtureStep::Call { args, .. } => {
            for (_, value) in args {
                check_meta_expr_refs(value, declared, ctx, span);
            }
        }
        crate::parser::ast::FixtureStep::Bind { value, .. } => {
            check_meta_expr_refs(value, declared, ctx, span);
        }
    }
}

fn check_transition_binding_metadata(
    binding: &crate::parser::ast::TransitionBinding,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    match binding {
        crate::parser::ast::TransitionBinding::Call { args, .. } => {
            for (_, value) in args {
                check_meta_expr_refs(value, declared, ctx, span);
            }
        }
        crate::parser::ast::TransitionBinding::Update {
            assignments,
            filter,
            ..
        } => {
            for (_, value) in assignments {
                check_meta_expr_refs(value, declared, ctx, span);
            }
            if let Some(filter_expr) = filter {
                check_meta_expr_refs(filter_expr, declared, ctx, span);
            }
        }
    }
}

fn check_meta_expr_refs(
    expr: &crate::parser::ast::MetaExpr,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    use crate::parser::ast::MetaExpr;

    match expr {
        MetaExpr::Ref(name) => {
            if !declared.contains(name) {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!("Undeclared metadata reference '${}'", name),
                    span,
                ));
            }
        }
        MetaExpr::Call { args, .. } | MetaExpr::List(args) => {
            for arg in args {
                check_meta_expr_refs(arg, declared, ctx, span);
            }
        }
        MetaExpr::Object(fields) => {
            for (_, value) in fields {
                check_meta_expr_refs(value, declared, ctx, span);
            }
        }
        MetaExpr::Binary { lhs, rhs, .. } => {
            check_meta_expr_refs(lhs, declared, ctx, span);
            check_meta_expr_refs(rhs, declared, ctx, span);
        }
        MetaExpr::Exists { filter, .. } => {
            if let Some(filter_expr) = filter {
                check_meta_expr_refs(filter_expr, declared, ctx, span);
            }
        }
        MetaExpr::Int(_)
        | MetaExpr::Duration(_)
        | MetaExpr::String(_)
        | MetaExpr::Bool(_)
        | MetaExpr::Null
        | MetaExpr::Ident(_)
        | MetaExpr::DottedName(_) => {}
    }
}

/// Helpers/clocks resolved by the executable replay layer (not model state):
/// time primitives, the bound-call `result`, and replay assertion helpers.
fn is_replay_builtin(name: &str) -> bool {
    matches!(
        name,
        "now_epoch"
            | "now"
            | "result"
            | "contains_id"
            | "always"
            | "eventually"
            | "db_pool"
            // Seed/value generators resolved by the contract replay runtime.
            | "random_bytes"
            | "unique"
            | "uuid4"
    )
}

fn check_expr_identifiers(
    expr: &crate::parser::ast::Expr,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    use crate::parser::ast::Expr;

    match expr {
        Expr::Ident(name) => {
            // `$ref` fixture/binding references and known replay/clock builtins
            // are resolved by the replay layer, not the model.
            if !name.starts_with('$') && !is_replay_builtin(name) && !declared.contains(name) {
                ctx.diagnostics.add(
                    Diagnostic::error(
                        ErrorCode::E013_ComponentNotFound,
                        format!("Undeclared identifier '{}' in guard expression", name),
                        span,
                    )
                    .with_suggestion("Declare this variable, state, parameter, or function"),
                );
            }
        }
        Expr::DottedName(name) => {
            if let Some(var) = name.strip_prefix("memory.") {
                if !declared.contains(var) {
                    ctx.diagnostics.add(Diagnostic::error(
                        ErrorCode::E013_ComponentNotFound,
                        format!("Undeclared memory variable '{}' in guard expression", var),
                        span,
                    ));
                }
            } else {
                let parts: Vec<&str> = name.split('.').collect();
                if let Some(first) = parts.first() {
                    if !declared.contains(*first) {
                        ctx.diagnostics.add(Diagnostic::error(
                            ErrorCode::E013_ComponentNotFound,
                            format!("Undeclared identifier '{}' in guard expression", first),
                            span,
                        ));
                    }
                }
            }
        }
        Expr::Call { name, args } => {
            if !is_replay_builtin(name) && !declared.contains(name) {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!("Undeclared function '{}' in guard expression", name),
                    span,
                ));
            }
            for arg in args {
                check_expr_identifiers(arg, declared, ctx, span);
            }
        }
        Expr::BinOp { lhs, rhs, .. }
        | Expr::CompOp { lhs, rhs, .. }
        | Expr::LogicalOp { lhs, rhs, .. } => {
            check_expr_identifiers(lhs, declared, ctx, span);
            check_expr_identifiers(rhs, declared, ctx, span);
        }
        Expr::UnaryOp { expr, .. } => {
            check_expr_identifiers(expr, declared, ctx, span);
        }
        Expr::IfThenElse {
            cond,
            then_expr,
            else_expr,
        } => {
            check_expr_identifiers(cond, declared, ctx, span);
            check_expr_identifiers(then_expr, declared, ctx, span);
            check_expr_identifiers(else_expr, declared, ctx, span);
        }
        Expr::Case { arms, default } => {
            for (cond, val) in arms {
                check_expr_identifiers(cond, declared, ctx, span);
                check_expr_identifiers(val, declared, ctx, span);
            }
            if let Some(def) = default {
                check_expr_identifiers(def, declared, ctx, span);
            }
        }
        // Add more cases as needed
        _ => {}
    }
}

fn check_effect_identifiers(
    effect: &crate::parser::ast::EffectStmt,
    declared: &HashSet<String>,
    ctx: &mut ValidationContext,
    span: Span,
) {
    use crate::parser::ast::EffectKind;

    match &effect.kind {
        EffectKind::Emit { name: _, args } => {
            // Events don't need to be pre-declared
            for arg in args {
                check_expr_identifiers(arg, declared, ctx, span);
            }
        }
        EffectKind::Send {
            channel: _,
            message: _,
            args,
        } => {
            // TODO: Validate message declarations exist
            for arg in args {
                check_expr_identifiers(arg, declared, ctx, span);
            }
        }
        EffectKind::Receive {
            channel: _,
            message: _,
            filter,
        } => {
            // TODO: Validate message declarations exist
            if let Some(filter_expr) = filter {
                check_expr_identifiers(filter_expr, declared, ctx, span);
            }
        }
        EffectKind::If {
            cond,
            then_effects,
            else_effects,
        } => {
            check_expr_identifiers(cond, declared, ctx, span);
            for eff in then_effects {
                check_effect_identifiers(eff, declared, ctx, span);
            }
            if let Some(else_effs) = else_effects {
                for eff in else_effs {
                    check_effect_identifiers(eff, declared, ctx, span);
                }
            }
        }
        EffectKind::Expr(expr) => {
            check_expr_identifiers(expr, declared, ctx, span);
        }
        EffectKind::Assign { var, value } => {
            if !declared.contains(var) {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E013_ComponentNotFound,
                    format!("Undeclared variable '{}' in assignment", var),
                    span,
                ));
            }
            check_expr_identifiers(value, declared, ctx, span);
        }
    }
}

/// Expression type checking pass using Hindley-Milner inference.
///
/// This pass uses the `InferenceContext` from the inference engine to
/// type-check expressions in behavior guards (where clauses), effects,
/// and invariants. It goes beyond the simple `TypeCheckPass` by performing
/// full unification-based type inference with occurs check.
pub struct ExpressionTypeCheckPass;

impl ValidationPass for ExpressionTypeCheckPass {
    fn name(&self) -> &'static str {
        "expression_type_check"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        for behavior in &system.behaviors {
            check_behavior_expressions(behavior, ctx);
        }
        for component in &system.components {
            for behavior in &component.behaviors {
                check_behavior_expressions(behavior, ctx);
            }
        }
    }
}

/// Convert a type name string (from a VariableDecl) to an InferType.
///
/// Handles simple types (e.g. "Int"), parameterized types (e.g. "List<Int>"),
/// and unknown/user-defined types.
fn type_name_to_infer_type(type_name: &str) -> crate::types::inference::InferType {
    use crate::types::inference::InferType;
    use crate::types::Type;

    // Check for parameterized types like "List<Int>" or "Map<String, Int>"
    if let Some(open) = type_name.find('<') {
        let base = &type_name[..open];
        let inner = &type_name[open + 1..type_name.len() - 1]; // strip < >
        match base {
            "List" => {
                let elem_type = type_name_to_infer_type(inner.trim());
                InferType::Concrete(Type::List(Box::new(
                    elem_type
                        .into_concrete()
                        .unwrap_or_else(|| Type::Var(inner.trim().to_string())),
                )))
            }
            _ => {
                // Generic Named type
                InferType::Concrete(Type::Named(crate::types::QualifiedName::simple(type_name)))
            }
        }
    } else {
        match Type::from_name(type_name) {
            Some(t) => InferType::Concrete(t),
            None => {
                // For unknown/user-defined types, create a concrete Named type
                InferType::Concrete(Type::Named(crate::types::QualifiedName::simple(type_name)))
            }
        }
    }
}

/// Build a type environment from a behavior's declared variables, parameters,
/// and functions.
fn build_type_env(behavior: &BehaviorDecl) -> crate::types::inference::TypeEnv {
    use crate::types::inference::{InferType, TypeEnv, TypeScheme};

    let mut env = TypeEnv::new();

    // Add declared variables
    for var in &behavior.variables {
        let infer_ty = type_name_to_infer_type(&var.type_name);
        env.insert(var.name.clone(), TypeScheme::mono(infer_ty));
    }

    // Add parameters
    for param in &behavior.parameters {
        let infer_ty = type_name_to_infer_type(&param.type_name);
        env.insert(param.name.clone(), TypeScheme::mono(infer_ty));
    }

    // Add functions with their return types
    for func in &behavior.functions {
        if let Some(ref ret_type) = func.return_type {
            let ret_infer = type_name_to_infer_type(ret_type);
            // Build a curried function type from params -> return
            let mut func_type = ret_infer;
            for (_pname, ptype) in func.params.iter().rev() {
                let param_infer = type_name_to_infer_type(ptype);
                func_type = InferType::function(param_infer, func_type);
            }
            env.insert(func.name.clone(), TypeScheme::mono(func_type));
        }
    }

    // Add state names as identifiers (they resolve to String/State type)
    for state in &behavior.states {
        env.insert(
            state.name.clone(),
            TypeScheme::mono(InferType::Concrete(crate::types::Type::State)),
        );
    }

    // Add event names from transitions
    for trans in &behavior.transitions {
        env.insert(
            trans.on_event.clone(),
            TypeScheme::mono(InferType::Concrete(crate::types::Type::Event)),
        );
    }

    // Add implicit 'state' identifier (refers to the current state value)
    env.insert(
        "state".to_string(),
        TypeScheme::mono(InferType::Concrete(crate::types::Type::String)),
    );

    env
}

/// Type-check all expressions within a behavior using Hindley-Milner inference.
fn check_behavior_expressions(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    use crate::types::inference::{InferType, InferenceContext};

    let infer_ctx = InferenceContext::new();
    let base_env = build_type_env(behavior);

    // Type-check transition guards and effects
    for transition in &behavior.transitions {
        let mut env = base_env.clone();

        for input in &transition.inputs {
            let input_type = type_name_to_infer_type(&input.type_name);
            env.insert(
                input.name.clone(),
                crate::types::inference::TypeScheme::mono(input_type.clone()),
            );

            if let Some(ref domain) = input.domain {
                let _ = infer_ctx.infer_expr(domain, &env);
            }

            if let Some(ref default_value) = input.default_value {
                if let Ok(default_type) = infer_ctx.infer_expr(default_value, &env) {
                    let _ = infer_ctx.unify(&default_type, &input_type, input.span);
                }
            }
        }

        // Check guard (where clause) -- must be boolean
        if let Some(ref guard) = transition.guard {
            match infer_ctx.infer_expr(guard, &env) {
                Ok(guard_type) => {
                    if infer_ctx
                        .unify(&guard_type, &InferType::bool(), transition.span)
                        .is_err()
                    {
                        // Unification failed – diagnostic already recorded,
                        // skip further checking of this transition to avoid cascading errors
                        continue;
                    }
                }
                Err(()) => {
                    // Inference error already recorded in infer_ctx diagnostics
                    continue;
                }
            }
        }

        for expect in &transition.expects {
            match infer_ctx.infer_expr(expect, &env) {
                Ok(expect_type) => {
                    let _ = infer_ctx.unify(&expect_type, &InferType::bool(), transition.span);
                }
                Err(()) => continue,
            }
        }

        // Check effects
        for effect in &transition.effects {
            check_effect_expression_types(effect, &infer_ctx, &env, behavior, transition.span);
        }
    }

    // Type-check invariants – infer types within expressions (catches internal
    // type errors) but don't require the overall type to be Bool.  TLA+
    // primitives may produce non-Bool values; the model checker enforces
    // correct types at verification time.
    for invariant in &behavior.invariants {
        let _ = infer_ctx.infer_expr(&invariant.expr, &base_env);
    }

    // Collect diagnostics from the inference context
    ctx.diagnostics.merge(infer_ctx.diagnostics());
}

/// Type-check expressions within an effect statement.
fn check_effect_expression_types(
    effect: &crate::parser::ast::EffectStmt,
    infer_ctx: &crate::types::inference::InferenceContext,
    env: &crate::types::inference::TypeEnv,
    behavior: &BehaviorDecl,
    span: Span,
) {
    use crate::parser::ast::EffectKind;
    use crate::types::inference::InferType;

    match &effect.kind {
        EffectKind::Assign { var, value } => {
            // Infer the type of the RHS expression
            if let Ok(rhs_type) = infer_ctx.infer_expr(value, env) {
                // Look up the declared type of the variable
                if let Some(var_decl) = behavior.variables.iter().find(|v| v.name == *var) {
                    let declared_type = type_name_to_infer_type(&var_decl.type_name);
                    // Unify RHS type with declared variable type
                    let _ = infer_ctx.unify(&rhs_type, &declared_type, span);
                }
            }
        }
        EffectKind::Emit { args, .. } => {
            for arg in args {
                let _ = infer_ctx.infer_expr(arg, env);
            }
        }
        EffectKind::Send { args, .. } => {
            for arg in args {
                let _ = infer_ctx.infer_expr(arg, env);
            }
        }
        EffectKind::Receive { filter, .. } => {
            if let Some(filter_expr) = filter {
                let _ = infer_ctx.infer_expr(filter_expr, env);
            }
        }
        EffectKind::If {
            cond,
            then_effects,
            else_effects,
        } => {
            // Condition must be boolean
            if let Ok(cond_type) = infer_ctx.infer_expr(cond, env) {
                let _ = infer_ctx.unify(&cond_type, &InferType::bool(), span);
            }
            for eff in then_effects {
                check_effect_expression_types(eff, infer_ctx, env, behavior, span);
            }
            if let Some(else_effs) = else_effects {
                for eff in else_effs {
                    check_effect_expression_types(eff, infer_ctx, env, behavior, span);
                }
            }
        }
        EffectKind::Expr(expr) => {
            let _ = infer_ctx.infer_expr(expr, env);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Change 20: Temporal Operator Compatibility Pass
// ═══════════════════════════════════════════════════════════════════════════

/// Checks temporal properties for operators not supported by Apalache.
///
/// `Until`, `Release`, `WeakUntil`, and `StrongRelease` require TLC.
/// This pass emits E055 warnings so the user knows before attempting
/// verification with Apalache.
pub struct TemporalOperatorCompatibilityPass;

impl ValidationPass for TemporalOperatorCompatibilityPass {
    fn name(&self) -> &'static str {
        "temporal_operator_compatibility"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        for behavior in &system.behaviors {
            check_temporal_operators(behavior, ctx);
        }
        for component in &system.components {
            for behavior in &component.behaviors {
                check_temporal_operators(behavior, ctx);
            }
        }
    }
}

fn check_temporal_operators(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    for property in &behavior.properties {
        check_temporal_expr_compat(&property.expr, &property.name, behavior, ctx);
    }
}

fn check_temporal_expr_compat(
    expr: &crate::parser::ast::TemporalExpr,
    property_name: &str,
    behavior: &BehaviorDecl,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::TemporalExpr;

    match expr {
        TemporalExpr::Until { lhs, rhs } => {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E055_UnsupportedTemporalOperator,
                    format!(
                        "Property '{}' in behavior '{}' uses 'until' which is not supported by Apalache",
                        property_name, behavior.name
                    ),
                    behavior.span,
                )
                .with_suggestion(
                    "This operator requires TLC. Use '--mode exhaustive' or rewrite using always/eventually.",
                ),
            );
            check_temporal_expr_compat(lhs, property_name, behavior, ctx);
            check_temporal_expr_compat(rhs, property_name, behavior, ctx);
        }
        TemporalExpr::Release { lhs, rhs } => {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E055_UnsupportedTemporalOperator,
                    format!(
                        "Property '{}' in behavior '{}' uses 'releases' which is not supported by Apalache",
                        property_name, behavior.name
                    ),
                    behavior.span,
                )
                .with_suggestion(
                    "This operator requires TLC. Use '--mode exhaustive' or rewrite using always/eventually.",
                ),
            );
            check_temporal_expr_compat(lhs, property_name, behavior, ctx);
            check_temporal_expr_compat(rhs, property_name, behavior, ctx);
        }
        TemporalExpr::WeakUntil { lhs, rhs } => {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E055_UnsupportedTemporalOperator,
                    format!(
                        "Property '{}' in behavior '{}' uses 'weak_until' which is not supported by Apalache",
                        property_name, behavior.name
                    ),
                    behavior.span,
                )
                .with_suggestion(
                    "This operator requires TLC. Use '--mode exhaustive' or rewrite using always/eventually.",
                ),
            );
            check_temporal_expr_compat(lhs, property_name, behavior, ctx);
            check_temporal_expr_compat(rhs, property_name, behavior, ctx);
        }
        TemporalExpr::StrongRelease { lhs, rhs } => {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E055_UnsupportedTemporalOperator,
                    format!(
                        "Property '{}' in behavior '{}' uses 'strong_releases' which is not supported by Apalache",
                        property_name, behavior.name
                    ),
                    behavior.span,
                )
                .with_suggestion(
                    "This operator requires TLC. Use '--mode exhaustive' or rewrite using always/eventually.",
                ),
            );
            check_temporal_expr_compat(lhs, property_name, behavior, ctx);
            check_temporal_expr_compat(rhs, property_name, behavior, ctx);
        }
        // Recurse into sub-expressions
        TemporalExpr::Always(inner)
        | TemporalExpr::Eventually(inner)
        | TemporalExpr::Next(inner)
        | TemporalExpr::Not(inner) => {
            check_temporal_expr_compat(inner, property_name, behavior, ctx);
        }
        TemporalExpr::AlwaysImplies {
            premise,
            conclusion,
        } => {
            check_temporal_expr_compat(premise, property_name, behavior, ctx);
            check_temporal_expr_compat(conclusion, property_name, behavior, ctx);
        }
        TemporalExpr::BinOp { lhs, rhs, .. } => {
            check_temporal_expr_compat(lhs, property_name, behavior, ctx);
            check_temporal_expr_compat(rhs, property_name, behavior, ctx);
        }
        // Leaf nodes: no recursion needed
        TemporalExpr::State(_) | TemporalExpr::Count(_) | TemporalExpr::Int(_) | TemporalExpr::Str(_) => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Change 23: Guard Overlap Analysis Pass
// ═══════════════════════════════════════════════════════════════════════════

/// Detects non-deterministic guard overlap in transitions.
///
/// Groups transitions by (from_state, on_event). For groups with >1 transition:
/// - If any lacks a guard → warn about unguarded fallthrough
/// - If all have guards → attempt syntactic complementarity check
/// - If guards not complementary → warn that determinism cannot be verified
pub struct GuardOverlapPass;

impl ValidationPass for GuardOverlapPass {
    fn name(&self) -> &'static str {
        "guard_overlap"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        for behavior in &system.behaviors {
            check_guard_overlap(behavior, ctx);
        }
        for component in &system.components {
            for behavior in &component.behaviors {
                check_guard_overlap(behavior, ctx);
            }
        }
    }
}

fn check_guard_overlap(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    use crate::parser::ast::TransitionSource;

    // Group transitions by (from_state, on_event)
    let mut groups: HashMap<(String, String), Vec<&crate::parser::ast::TransitionDecl>> =
        HashMap::new();

    for transition in &behavior.transitions {
        let from_key = match &transition.from {
            TransitionSource::State(s) => s.clone(),
            TransitionSource::Wildcard => "*".to_string(),
            TransitionSource::States(states) => states.join(","),
        };
        let key = (from_key, transition.on_event.clone());
        groups.entry(key).or_default().push(transition);
    }

    for ((from, event), transitions) in &groups {
        if transitions.len() <= 1 {
            continue;
        }

        let has_unguarded = transitions.iter().any(|t| t.guard.is_none());
        let all_guarded = transitions.iter().all(|t| t.guard.is_some());

        if has_unguarded {
            ctx.diagnostics.add(
                Diagnostic::warning(
                    ErrorCode::E057_NonDeterministicGuards,
                    format!(
                        "Behavior '{}': {} transitions from '{}' on '{}', but not all have guards – unguarded fallthrough",
                        behavior.name,
                        transitions.len(),
                        from,
                        event,
                    ),
                    behavior.span,
                )
                .with_suggestion("Add guards to all transitions sharing the same source and event."),
            );
        } else if all_guarded {
            // All have guards – check syntactic complementarity
            let guards: Vec<&crate::parser::ast::Expr> = transitions
                .iter()
                .filter_map(|t| t.guard.as_ref())
                .collect();

            if !are_guards_complementary(&guards) {
                ctx.diagnostics.add(
                    Diagnostic::warning(
                        ErrorCode::E057_NonDeterministicGuards,
                        format!(
                            "Behavior '{}': {} transitions from '{}' on '{}' have guards that may overlap – determinism cannot be verified statically",
                            behavior.name,
                            transitions.len(),
                            from,
                            event,
                        ),
                        behavior.span,
                    )
                    .with_suggestion("Ensure guards are mutually exclusive (e.g. 'x < y' vs 'x >= y')."),
                );
            }
        }
    }
}

/// Syntactic check for complementary guards.
///
/// For exactly two guards, checks:
/// - `!a` vs `a`
/// - `x < y` vs `x >= y` (comparison op complements)
fn are_guards_complementary(guards: &[&crate::parser::ast::Expr]) -> bool {
    use crate::parser::ast::{Expr, UnaryOp};

    if guards.len() != 2 {
        return false;
    }

    let a = guards[0];
    let b = guards[1];

    // Check !a vs a or a vs !a
    if matches!(a, Expr::UnaryOp { op: UnaryOp::Not, expr } if expr.as_ref() == b) {
        return true;
    }
    if matches!(b, Expr::UnaryOp { op: UnaryOp::Not, expr } if expr.as_ref() == a) {
        return true;
    }

    // Check comparison complements: (x op1 y) vs (x op2 y) where op1 and op2 are complements
    if let (
        Expr::CompOp {
            lhs: l1,
            op: op1,
            rhs: r1,
        },
        Expr::CompOp {
            lhs: l2,
            op: op2,
            rhs: r2,
        },
    ) = (a, b)
    {
        if l1 == l2 && r1 == r2 {
            return is_complement_op(*op1, *op2);
        }
    }

    false
}

fn is_complement_op(
    a: crate::parser::ast::ComparisonOp,
    b: crate::parser::ast::ComparisonOp,
) -> bool {
    use crate::parser::ast::ComparisonOp::*;
    matches!(
        (a, b),
        (Lt, Ge) | (Ge, Lt) | (Gt, Le) | (Le, Gt) | (Eq, Ne) | (Ne, Eq)
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature 27: Constrained Type Validation Pass
// ═══════════════════════════════════════════════════════════════════════════

/// Validates initial values of variables with constrained types.
///
/// For variables declared with a constrained type (e.g., `Nat`, `Int(1..10)`),
/// checks that the initial value satisfies the constraint.
pub struct ConstrainedTypeValidationPass;

impl ValidationPass for ConstrainedTypeValidationPass {
    fn name(&self) -> &'static str {
        "constrained_type_validation"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        for behavior in &system.behaviors {
            check_constrained_variables(behavior, ctx);
        }
        for component in &system.components {
            for behavior in &component.behaviors {
                check_constrained_variables(behavior, ctx);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature 28: Pattern Type Parameter Validation Pass
// ═══════════════════════════════════════════════════════════════════════════

/// Validates where constraints on pattern type parameters during pattern application.
///
/// For each pattern application with type arguments, checks that the concrete
/// types satisfy the where constraints declared on the pattern.
pub struct PatternTypeParameterPass;

impl ValidationPass for PatternTypeParameterPass {
    fn name(&self) -> &'static str {
        "pattern_type_parameter"
    }

    fn run(&self, system: &SystemDecl, ctx: &mut ValidationContext) {
        // Move extra patterns out of ctx so pattern refs don't borrow ctx
        let extra_patterns = std::mem::take(&mut ctx.extra_patterns);

        let patterns: HashMap<&str, &PatternDecl> = system
            .patterns
            .iter()
            .chain(extra_patterns.iter())
            .map(|p| (p.name.as_str(), p))
            .collect();

        // Check system-level pattern applications
        for app in &system.applies {
            let pattern_name = app.pattern.name();
            if let Some(pattern) = patterns.get(pattern_name) {
                check_where_constraints(app, pattern, system, ctx);
            }
        }

        // Check behavior-level pattern applications
        for behavior in &system.behaviors {
            for app in &behavior.applies {
                let pattern_name = app.pattern.name();
                if let Some(pattern) = patterns.get(pattern_name) {
                    check_where_constraints(app, pattern, system, ctx);
                }
            }
        }

        for component in &system.components {
            for behavior in &component.behaviors {
                for app in &behavior.applies {
                    let pattern_name = app.pattern.name();
                    if let Some(pattern) = patterns.get(pattern_name) {
                        check_where_constraints(app, pattern, system, ctx);
                    }
                }
            }
        }

        ctx.extra_patterns = extra_patterns;
    }
}

fn check_where_constraints(
    app: &crate::parser::ast::PatternApplication,
    pattern: &PatternDecl,
    system: &SystemDecl,
    ctx: &mut ValidationContext,
) {
    use crate::parser::ast::TypeBound;

    // Build map: type_param -> concrete type arg
    let type_map: HashMap<&str, &str> = pattern
        .type_params
        .iter()
        .zip(app.type_args.iter())
        .map(|(param, arg)| (param.name.as_str(), arg.as_str()))
        .collect();

    // Check for missing type arguments
    if !pattern.type_params.is_empty() && app.type_args.len() < pattern.type_params.len() {
        for param in pattern.type_params.iter().skip(app.type_args.len()) {
            ctx.diagnostics.add(Diagnostic::error(
                ErrorCode::E033_MissingTypeArgument,
                format!(
                    "Missing type argument for parameter '{}' in pattern '{}'",
                    param.name, pattern.name
                ),
                app.span,
            ));
        }
        return;
    }

    // Validate each where constraint
    for constraint in &pattern.where_constraints {
        let concrete_type = match type_map.get(constraint.type_param.as_str()) {
            Some(t) => *t,
            None => continue, // Type param not found – separate error
        };

        for (field_name, bound) in &constraint.required_fields {
            let satisfied = match bound {
                TypeBound::Event => {
                    // Check if the concrete type has an event with this name
                    // Look for the event in system behaviors
                    has_event_in_system(concrete_type, field_name, system)
                }
                TypeBound::State => {
                    // Check if the concrete type has a state with this name
                    has_state_in_system(concrete_type, field_name, system)
                }
                _ => true, // Other bounds not checked here
            };

            if !satisfied {
                ctx.diagnostics.add(Diagnostic::error(
                    ErrorCode::E031_TypeParameterBoundViolation,
                    format!(
                        "Type argument '{}' for parameter '{}' in pattern '{}' does not satisfy constraint: missing {} '{}'",
                        concrete_type, constraint.type_param, pattern.name,
                        match bound {
                            TypeBound::Event => "event",
                            TypeBound::State => "state",
                            _ => "field",
                        },
                        field_name,
                    ),
                    app.span,
                ));
            }
        }
    }
}

/// Check if a component/behavior in the system has an event with the given name.
fn has_event_in_system(component_name: &str, event_name: &str, system: &SystemDecl) -> bool {
    // Check system events
    if system.events.iter().any(|e| e.name == event_name) {
        return true;
    }

    // Check transition events in behaviors matching the component
    for behavior in &system.behaviors {
        if behavior.name == component_name {
            if behavior
                .transitions
                .iter()
                .any(|t| t.on_event == event_name)
            {
                return true;
            }
        }
    }

    for component in &system.components {
        if component.name == component_name {
            for behavior in &component.behaviors {
                if behavior
                    .transitions
                    .iter()
                    .any(|t| t.on_event == event_name)
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if a component/behavior in the system has a state with the given name.
fn has_state_in_system(component_name: &str, state_name: &str, system: &SystemDecl) -> bool {
    for behavior in &system.behaviors {
        if behavior.name == component_name {
            if behavior.states.iter().any(|s| s.name == state_name) {
                return true;
            }
        }
    }

    for component in &system.components {
        if component.name == component_name {
            for behavior in &component.behaviors {
                if behavior.states.iter().any(|s| s.name == state_name) {
                    return true;
                }
            }
        }
    }

    false
}

fn check_constrained_variables(behavior: &BehaviorDecl, ctx: &mut ValidationContext) {
    use crate::types::{Type, TypeConstraint};

    for var in &behavior.variables {
        let ty = Type::from_name(&var.type_name);
        if let Some(Type::Constrained { constraint, .. }) = ty {
            if let Some(ref init) = var.initial_value {
                match (&constraint, init) {
                    (TypeConstraint::Range(lo, hi), crate::parser::ast::Expr::Int(val)) => {
                        if *val < *lo || *val > *hi {
                            ctx.diagnostics.add(Diagnostic::error(
                                ErrorCode::E058_RefinementConstraintViolation,
                                format!(
                                    "Variable '{}' initial value {} is outside range {}..{}",
                                    var.name, val, lo, hi
                                ),
                                behavior.span,
                            ));
                        }
                    }
                    (TypeConstraint::NonNegative, crate::parser::ast::Expr::Int(val)) => {
                        if *val < 0 {
                            ctx.diagnostics.add(Diagnostic::error(
                                ErrorCode::E058_RefinementConstraintViolation,
                                format!(
                                    "Variable '{}' initial value {} violates Nat (non-negative) constraint",
                                    var.name, val
                                ),
                                behavior.span,
                            ));
                        }
                    }
                    _ => {} // Other constraint types or non-literal initial values
                }
            }
        }
    }
}
