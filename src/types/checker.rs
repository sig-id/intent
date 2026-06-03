//! Type checker for the Intent language.
//!
//! Validates type annotations and infers types for expressions.
//!
//! **Deprecated:** The `TypeContext`/`is_compatible` system is superseded by
//! `InferenceContext` (Hindley-Milner) in `types::inference`. The `TypeCheckPass`
//! now uses `InferenceContext` directly. This module is retained for its
//! `infer_param_value_type` and `infer_param_value_type_checked` utilities
//! which are still used to convert `ParamValue` → `Type`.

use crate::diagnostic::{Diagnostic, Diagnostics, ErrorCode, Span};
use crate::parser::ast::{Expr, ParamValue, PatternApplication, PatternParam};
use crate::types::Type;

/// Type checking context.
#[derive(Debug, Default)]
pub struct TypeContext {
    /// Collected diagnostics
    pub diagnostics: Diagnostics,
    /// Type variable bindings
    bindings: std::collections::HashMap<String, Type>,
}

impl TypeContext {
    /// Create a new type context.
    pub fn new() -> Self {
        Self {
            diagnostics: Diagnostics::new(),
            bindings: std::collections::HashMap::new(),
        }
    }

    /// Bind a type variable.
    pub fn bind(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    /// Look up a type variable.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Check if two types are compatible.
    pub fn is_compatible(&self, expected: &Type, actual: &Type) -> bool {
        match (expected, actual) {
            // Same types are always compatible
            (a, b) if a == b => true,
            // Optional accepts the inner type or None
            (Type::Optional(inner), actual) => self.is_compatible(inner, actual),
            // EntityList is compatible with List<Entity>
            (Type::EntityList, Type::List(inner)) => matches!(inner.as_ref(), Type::Entity),
            // EventList is compatible with List<Event>
            (Type::EventList, Type::List(inner)) => matches!(inner.as_ref(), Type::Event),
            // Type variables
            (Type::Var(name), actual) | (actual, Type::Var(name)) => {
                if let Some(bound) = self.lookup(name) {
                    self.is_compatible(bound, actual)
                } else {
                    false // Unbound type var: unknown type, reject
                }
            }
            _ => false,
        }
    }
}

/// Check pattern parameter types.
pub fn check_pattern_params(params: &[PatternParam], ctx: &mut TypeContext) {
    for param in params {
        // Validate the type name is a known type
        let type_name = &param.type_name;
        if Type::from_name(type_name).is_none() {
            // Unknown type - might be a custom type, emit info
            ctx.diagnostics.add(Diagnostic::warning(
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
                // Check that default value matches declared type
                if let Err(diag) = check_value_type(value, &param.type_name, param.span) {
                    ctx.diagnostics.add(diag);
                }
            }
        }
    }
}

/// Check pattern application parameter types.
pub fn check_pattern_application(
    application: &PatternApplication,
    params: &[PatternParam],
    ctx: &mut TypeContext,
) {
    // Build a map of expected parameter types
    let param_types: std::collections::HashMap<&str, &str> = params
        .iter()
        .map(|p| (p.name.as_str(), p.type_name.as_str()))
        .collect();

    // Check each provided parameter
    for (name, value) in &application.params {
        if let Some(expected_type) = param_types.get(name.as_str()) {
            if let Err(diag) = check_value_type(value, expected_type, application.span) {
                ctx.diagnostics.add(diag);
            }
        } else {
            ctx.diagnostics.add(Diagnostic::error(
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
            ctx.diagnostics.add(Diagnostic::error(
                ErrorCode::E011_MissingRequiredField,
                format!("Missing required parameter '{}' for pattern", param.name),
                application.span,
            ));
        }
    }
}

/// Check that a value matches an expected type.
pub fn check_value_type(
    value: &ParamValue,
    expected_type: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let actual_type = infer_param_value_type(value);
    let expected = Type::from_name(expected_type).unwrap_or_else(|| {
        Type::Named(crate::types::QualifiedName::simple(expected_type).with_span(span))
    });

    // Check type compatibility
    let compatible = match (&expected, &actual_type) {
        (Type::Int, Type::Int) => true,
        (Type::Float, Type::Float | Type::Int) => true, // Int is assignable to Float
        (Type::Bool, Type::Bool) => true,
        (Type::String, Type::String) => true,
        (Type::Duration, Type::Duration) => true,
        (Type::Entity, Type::String) => true, // Entity names are strings
        (Type::State, Type::String) => true,  // State names are strings
        (Type::Event, Type::String) => true,  // Event names are strings
        (Type::EntityList, Type::List(_)) => true,
        (Type::EventList, Type::List(_)) => true,
        (Type::List(expected_inner), Type::List(actual_inner)) => {
            // Check element types
            expected_inner.as_ref() == actual_inner.as_ref()
        }
        _ => expected == actual_type,
    };

    if compatible {
        Ok(())
    } else {
        Err(Diagnostic::error(
            ErrorCode::E002_TypeMismatch,
            format!(
                "Type mismatch: expected '{}', found '{}'",
                expected.type_name(),
                actual_type.type_name()
            ),
            span,
        ))
    }
}

/// Infer the type of a parameter value, checking all list elements for consistency.
///
/// When inferring the type of a list, all elements are checked. If any element
/// has a different type from the first, a diagnostic is emitted.
pub fn infer_param_value_type_checked(
    value: &ParamValue,
    diagnostics: &mut Diagnostics,
    span: Span,
) -> Type {
    match value {
        ParamValue::Ident(_) => Type::String,
        ParamValue::Int(_) => Type::Int,
        ParamValue::Float(_) => Type::Float,
        ParamValue::Duration(_) => Type::Duration,
        ParamValue::String(_) => Type::String,
        ParamValue::Bool(_) => Type::Bool,
        ParamValue::List(items) => {
            if items.is_empty() {
                Type::List(Box::new(Type::Var("T".to_string())))
            } else {
                let first_type = infer_param_value_type_checked(&items[0], diagnostics, span);
                for (i, item) in items.iter().enumerate().skip(1) {
                    let item_type = infer_param_value_type_checked(item, diagnostics, span);
                    if item_type != first_type {
                        diagnostics.add(Diagnostic::error(
                            ErrorCode::E002_TypeMismatch,
                            format!(
                                "Heterogeneous list: element {} has type '{}', expected '{}'",
                                i,
                                item_type.type_name(),
                                first_type.type_name()
                            ),
                            span,
                        ));
                    }
                }
                Type::List(Box::new(first_type))
            }
        }
        ParamValue::Map(_) => Type::Named(crate::types::QualifiedName::simple("Map")),
    }
}

/// Infer the type of a parameter value.
///
/// For backward compatibility, this delegates to `infer_param_value_type_checked`
/// with a throwaway diagnostics collection.
pub fn infer_param_value_type(value: &ParamValue) -> Type {
    let mut diagnostics = Diagnostics::new();
    infer_param_value_type_checked(value, &mut diagnostics, Span::synthetic())
}

/// Infer the type of an expression.
pub fn infer_expr_type(expr: &Expr) -> Type {
    match expr {
        Expr::Int(_) => Type::Int,
        Expr::Float(_) => Type::Float,
        Expr::Duration(_) => Type::Duration,
        Expr::String(_) => Type::String,
        Expr::Bool(_) => Type::Bool,
        Expr::Ident(_) => Type::Var("T".to_string()), // Unknown type
        Expr::DottedName(_) => Type::Var("T".to_string()),
        Expr::Call { name, args: _ } => {
            // Special cases
            match name.as_str() {
                "count" => Type::Int,
                "p99" | "p95" | "avg" | "min" | "max" => Type::Float,
                _ => Type::Var("T".to_string()),
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let lhs_type = infer_expr_type(lhs);
            let rhs_type = infer_expr_type(rhs);
            match op {
                crate::parser::ast::ArithOp::Add
                | crate::parser::ast::ArithOp::Sub
                | crate::parser::ast::ArithOp::Mul
                | crate::parser::ast::ArithOp::Div => {
                    // Numeric operations
                    if matches!(lhs_type, Type::Float) || matches!(rhs_type, Type::Float) {
                        Type::Float
                    } else {
                        Type::Int
                    }
                }
            }
        }
        Expr::CompOp { .. } => Type::Bool,
        Expr::LogicalOp { .. } => Type::Bool,
        Expr::UnaryOp { op, .. } => {
            match op {
                crate::parser::ast::UnaryOp::Not => Type::Bool,
                crate::parser::ast::UnaryOp::Neg => Type::Int, // or Float
            }
        }
        Expr::Count(_) => Type::Int, // count() returns an integer
        Expr::Choose { .. } => Type::Var("T".to_string()),
        Expr::Let { body, .. } => infer_expr_type(body),
        Expr::IfThenElse { then_expr, .. } => infer_expr_type(then_expr),
        Expr::Case { arms, default } => {
            if let Some((_, expr)) = arms.first() {
                infer_expr_type(expr)
            } else if let Some(d) = default {
                infer_expr_type(d)
            } else {
                Type::Var("T".to_string())
            }
        }
        Expr::Subset(_) => Type::Var("Set".to_string()),
        Expr::BigUnion(_) => Type::Var("Set".to_string()),
        Expr::Domain(_) => Type::Var("Set".to_string()),
        Expr::Except { base, .. } => infer_expr_type(base),
        Expr::FunctionLiteral { .. } => Type::Var("Function".to_string()),
        Expr::Record(_) => Type::Var("Record".to_string()),
        Expr::FieldAccess { .. } => Type::Var("T".to_string()),
        Expr::Tuple(_) => Type::Var("Tuple".to_string()),
        Expr::SetLiteral(_) => Type::Var("Set".to_string()),
        Expr::Index { .. } => Type::Var("T".to_string()),
        Expr::SetDiff { .. } => Type::Var("Set".to_string()),
        Expr::SetUnion { .. } => Type::Var("Set".to_string()),
        Expr::SetIntersect { .. } => Type::Var("Set".to_string()),
        Expr::In { .. } => Type::Bool,
        Expr::Forall { .. } => Type::Bool,
        Expr::Exists { .. } => Type::Bool,
        Expr::Assume(_) => Type::Bool,
        Expr::TlaInline { .. } => Type::Bool, // TLA+ inline formulas are boolean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_param_value_type() {
        assert_eq!(infer_param_value_type(&ParamValue::Int(42)), Type::Int);
        assert_eq!(
            infer_param_value_type(&ParamValue::Float(3.14)),
            Type::Float
        );
        assert_eq!(infer_param_value_type(&ParamValue::Bool(true)), Type::Bool);
        assert_eq!(
            infer_param_value_type(&ParamValue::String("test".to_string())),
            Type::String
        );
        assert_eq!(
            infer_param_value_type(&ParamValue::Duration(1000)),
            Type::Duration
        );
    }

    #[test]
    fn test_check_value_type_int() {
        let result = check_value_type(&ParamValue::Int(42), "Int", Span::synthetic());
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_value_type_mismatch() {
        let result = check_value_type(
            &ParamValue::String("test".to_string()),
            "Int",
            Span::synthetic(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_infer_expr_type() {
        assert_eq!(infer_expr_type(&Expr::Int(42)), Type::Int);
        assert_eq!(infer_expr_type(&Expr::Bool(true)), Type::Bool);
        assert_eq!(infer_expr_type(&Expr::Float(3.14)), Type::Float);

        // Comparison returns bool
        let comp = Expr::CompOp {
            lhs: Box::new(Expr::Int(1)),
            op: crate::parser::ast::ComparisonOp::Lt,
            rhs: Box::new(Expr::Int(2)),
        };
        assert_eq!(infer_expr_type(&comp), Type::Bool);
    }

    #[test]
    fn test_infer_param_value_type_homogeneous_list() {
        let list = ParamValue::List(vec![
            ParamValue::Int(1),
            ParamValue::Int(2),
            ParamValue::Int(3),
        ]);
        let mut diagnostics = Diagnostics::new();
        let ty = infer_param_value_type_checked(&list, &mut diagnostics, Span::synthetic());
        assert_eq!(ty, Type::List(Box::new(Type::Int)));
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn test_infer_param_value_type_heterogeneous_list() {
        let list = ParamValue::List(vec![
            ParamValue::Int(1),
            ParamValue::String("hello".to_string()),
            ParamValue::Bool(true),
        ]);
        let mut diagnostics = Diagnostics::new();
        let ty = infer_param_value_type_checked(&list, &mut diagnostics, Span::synthetic());
        // Returns the first element's type
        assert_eq!(ty, Type::List(Box::new(Type::Int)));
        // But emits diagnostics for mismatched elements
        assert!(diagnostics.has_errors());
        assert_eq!(diagnostics.items.len(), 2); // Two mismatches (element 1 and 2)
    }

    #[test]
    fn test_type_context() {
        let mut ctx = TypeContext::new();
        ctx.bind("T".to_string(), Type::Int);

        assert!(ctx.is_compatible(&Type::Var("T".to_string()), &Type::Int));
        assert!(!ctx.is_compatible(&Type::Var("T".to_string()), &Type::String));
    }

    #[test]
    fn test_unbound_type_var_rejects() {
        let ctx = TypeContext::new();
        // Unbound type variable should NOT be compatible with anything
        assert!(!ctx.is_compatible(&Type::Var("U".to_string()), &Type::Int));
        assert!(!ctx.is_compatible(&Type::Var("U".to_string()), &Type::String));
        assert!(!ctx.is_compatible(&Type::Int, &Type::Var("U".to_string())));
    }
}
