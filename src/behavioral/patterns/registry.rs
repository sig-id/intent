//! Pattern registry for loading and expanding stdlib patterns.
//!
//! This module provides a registry of built-in patterns from `stdlib/patterns.intent`
//! that can be instantiated with specific parameters and merged into behaviors.

use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::parser::ast::{
    BehaviorDecl, ParamValue, PatternApplication, PatternDecl, StateDecl,
    TemporalProperty, TransitionDecl,
};

/// Registry of available patterns.
pub struct PatternRegistry {
    patterns: HashMap<String, PatternDecl>,
}

impl PatternRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
        }
    }

    /// Load patterns from parsed pattern declarations.
    pub fn load(&mut self, patterns: Vec<PatternDecl>) {
        for pattern in patterns {
            self.patterns.insert(pattern.name.clone(), pattern);
        }
    }

    /// Get a pattern by name.
    pub fn get(&self, name: &str) -> Option<&PatternDecl> {
        self.patterns.get(name)
    }

    /// Expand a pattern application into behavior elements.
    ///
    /// Returns states, transitions, properties, and fairness specs
    /// that should be merged into the applying behavior.
    pub fn expand(&self, app: &PatternApplication) -> Result<PatternExpansion> {
        let pattern_name = app.pattern.name();
        let pattern = self.get(pattern_name)
            .ok_or_else(|| anyhow!("pattern '{}' not found", app.pattern))?;

        let behavior = pattern.behavior.as_ref()
            .ok_or_else(|| anyhow!("pattern '{}' has no behavior", app.pattern))?;

        // Substitute parameters
        let params: HashMap<String, &ParamValue> = app.params.iter()
            .map(|(k, v)| (k.clone(), v))
            .collect();

        // Copy states with pattern prefix for namespacing
        let prefix = format!("{}_", pattern_name.to_lowercase());
        let states: Vec<StateDecl> = behavior.states.iter()
            .map(|s| StateDecl {
                name: format!("{}{}", prefix, s.name),
                initial: s.initial,
                terminal: s.terminal,
                parent: s.parent.clone(),
                substates: s.substates.clone(),
                entry_actions: s.entry_actions.clone(),
                exit_actions: s.exit_actions.clone(),
            })
            .collect();

        // Copy transitions with renamed states
        let transitions: Vec<TransitionDecl> = behavior.transitions.iter()
            .map(|t| {
                let mut t = t.clone();
                if let Some(from) = t.from.as_state() {
                    t.from = crate::parser::ast::TransitionSource::State(format!("{}{}", prefix, from));
                }
                if let Some(to) = t.to.as_state() {
                    t.to = crate::parser::ast::TransitionTarget::State(format!("{}{}", prefix, to));
                }
                t
            })
            .collect();

        // Copy properties with renamed state references
        let properties: Vec<TemporalProperty> = behavior.properties.iter()
            .map(|p| TemporalProperty {
                name: format!("{}_{}", app.pattern, p.name),
                expr: p.expr.clone(), // TODO: rename state refs
            })
            .collect();

        // Copy fairness specs
        let fairness = behavior.fairness.clone();

        Ok(PatternExpansion {
            pattern_name: pattern_name.to_string(),
            states,
            transitions,
            properties,
            fairness,
            params: params.into_iter().map(|(k, v)| (k, v.clone())).collect(),
        })
    }
}

impl Default for PatternRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Expanded pattern ready to be merged into a behavior.
#[derive(Debug, Clone)]
pub struct PatternExpansion {
    pub pattern_name: String,
    pub states: Vec<StateDecl>,
    pub transitions: Vec<TransitionDecl>,
    pub properties: Vec<TemporalProperty>,
    pub fairness: Vec<crate::parser::ast::FairnessSpec>,
    pub params: HashMap<String, ParamValue>,
}

impl PatternExpansion {
    /// Generate TLA+ constants for pattern parameters.
    pub fn generate_constants(&self) -> Vec<(String, String)> {
        let mut constants = Vec::new();
        for (name, value) in &self.params {
            let tla_val = param_to_tla(value);
            constants.push((format!("{}_{}", self.pattern_name, name), tla_val));
        }
        constants
    }
}

/// Convert a parameter value to TLA+ literal.
fn param_to_tla(value: &ParamValue) -> String {
    match value {
        ParamValue::Int(n) => n.to_string(),
        ParamValue::Float(f) => f.to_string(),
        ParamValue::Duration(ms) => ms.to_string(),
        ParamValue::String(s) => format!("\"{}\"", s),
        ParamValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        ParamValue::Ident(i) => i.clone(),
        ParamValue::List(items) => {
            let elems: Vec<String> = items.iter().map(param_to_tla).collect();
            format!("<<{}>>", elems.join(", "))
        }
        ParamValue::Map(entries) => {
            let fields: Vec<String> = entries.iter()
                .map(|(k, v)| format!("{} |-> {}", k, param_to_tla(v)))
                .collect();
            format!("[{}]", fields.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Span;

    #[test]
    fn test_param_to_tla() {
        assert_eq!(param_to_tla(&ParamValue::Int(42)), "42");
        assert_eq!(param_to_tla(&ParamValue::Bool(true)), "TRUE");
        assert_eq!(param_to_tla(&ParamValue::Duration(1000)), "1000");
    }
}
