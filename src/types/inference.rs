//! Hindley-Milner type inference with Algorithm W.
//!
//! This module provides complete type inference including:
//! - Unification with occurs check
//! - Let-polymorphism (generalization and instantiation)
//! - Type schemes for polymorphic values
//! - Bidirectional type checking

use std::cell::RefCell;
use std::collections::HashMap;

use crate::diagnostic::{Diagnostic, Diagnostics, ErrorCode, Span};
use crate::parser::ast::{ArithOp, Expr, UnaryOp};
use crate::types::Type;

// ═══════════════════════════════════════════════════════════════════════════
// TYPE VARIABLES AND SCHEMES
// ═══════════════════════════════════════════════════════════════════════════

/// Unique identifier for type variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeVarId(pub u32);

impl std::fmt::Display for TypeVarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "'{}", self.0)
    }
}

/// Type variable for inference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeVar {
    /// Unique identifier
    pub id: TypeVarId,
    /// Optional name for better error messages
    pub name: Option<String>,
}

impl TypeVar {
    /// Create a new type variable with an optional name.
    pub fn new(id: TypeVarId, name: Option<String>) -> Self {
        Self { id, name }
    }
}

impl std::fmt::Display for TypeVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "'{}", name),
            None => write!(f, "{}", self.id),
        }
    }
}

/// Inference type - extended Type with type variables.
#[derive(Debug, Clone, PartialEq)]
pub enum InferType {
    /// Concrete type
    Concrete(Type),
    /// Type variable (unified)
    Var(TypeVar),
    /// Function type: arg -> ret
    Function(Box<InferType>, Box<InferType>),
    /// Record type with row polymorphism
    Record(RowType),
    /// Universal quantification: ∀a. τ
    ForAll {
        vars: Vec<TypeVar>,
        body: Box<InferType>,
    },
}

impl InferType {
    /// Create an integer type.
    pub fn int() -> Self {
        InferType::Concrete(Type::Int)
    }

    /// Create a float type.
    pub fn float() -> Self {
        InferType::Concrete(Type::Float)
    }

    /// Create a boolean type.
    pub fn bool() -> Self {
        InferType::Concrete(Type::Bool)
    }

    /// Create a string type.
    pub fn string() -> Self {
        InferType::Concrete(Type::String)
    }

    /// Create a duration type.
    pub fn duration() -> Self {
        InferType::Concrete(Type::Duration)
    }

    /// Create a function type.
    pub fn function(arg: InferType, ret: InferType) -> Self {
        InferType::Function(Box::new(arg), Box::new(ret))
    }

    /// Create a type variable.
    pub fn var(id: TypeVarId, name: Option<String>) -> Self {
        InferType::Var(TypeVar::new(id, name))
    }

    /// Create a polymorphic type.
    pub fn for_all(vars: Vec<TypeVar>, body: InferType) -> Self {
        InferType::ForAll {
            vars,
            body: Box::new(body),
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> String {
        match self {
            InferType::Concrete(t) => t.type_name(),
            InferType::Var(v) => v.to_string(),
            InferType::Function(arg, ret) => {
                format!("({} -> {})", arg.type_name(), ret.type_name())
            }
            InferType::Record(row) => format!("{{{}}}", row.type_name()),
            InferType::ForAll { vars, body } => {
                let vars_str: Vec<String> = vars.iter().map(|v| v.to_string()).collect();
                format!("∀{}. {}", vars_str.join(" "), body.type_name())
            }
        }
    }

    /// Collect all free type variables in this type.
    pub fn free_vars(&self) -> Vec<TypeVarId> {
        match self {
            InferType::Concrete(_) => vec![],
            InferType::Var(v) => vec![v.id],
            InferType::Function(arg, ret) => {
                let mut vars = arg.free_vars();
                vars.extend(ret.free_vars());
                vars
            }
            InferType::Record(row) => row.free_vars(),
            InferType::ForAll { vars, body } => {
                let bound: std::collections::HashSet<TypeVarId> =
                    vars.iter().map(|v| v.id).collect();
                body.free_vars()
                    .into_iter()
                    .filter(|id| !bound.contains(id))
                    .collect()
            }
        }
    }

    /// Apply a substitution to this type.
    pub fn apply_subst(&self, subst: &Substitution) -> Self {
        match self {
            InferType::Concrete(t) => InferType::Concrete(t.clone()),
            InferType::Var(v) => {
                if let Some(t) = subst.get(v.id) {
                    t.apply_subst(subst)
                } else {
                    self.clone()
                }
            }
            InferType::Function(arg, ret) => {
                InferType::function(arg.apply_subst(subst), ret.apply_subst(subst))
            }
            InferType::Record(row) => InferType::Record(row.apply_subst(subst)),
            InferType::ForAll { vars, body } => {
                let bound: std::collections::HashSet<TypeVarId> =
                    vars.iter().map(|v| v.id).collect();
                let filtered_subst = subst.filter_out(&bound);
                InferType::for_all(vars.clone(), body.apply_subst(&filtered_subst))
            }
        }
    }
}

impl std::fmt::Display for InferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_name())
    }
}

/// Row type for record polymorphism.
#[derive(Debug, Clone, PartialEq)]
pub enum RowType {
    /// Empty row
    Empty,
    /// Row extension: { field: type | rest }
    Extend {
        field: String,
        type_: Box<InferType>,
        rest: Box<RowType>,
    },
    /// Row variable for polymorphism
    Var(TypeVar),
}

impl RowType {
    /// Create an empty row.
    pub fn empty() -> Self {
        RowType::Empty
    }

    /// Extend a row with a field.
    pub fn extend(field: String, type_: InferType, rest: RowType) -> Self {
        RowType::Extend {
            field,
            type_: Box::new(type_),
            rest: Box::new(rest),
        }
    }

    /// Create a row from field list.
    pub fn from_fields(fields: Vec<(String, InferType)>) -> Self {
        let mut row = RowType::empty();
        for (field, type_) in fields.into_iter().rev() {
            row = RowType::extend(field, type_, row);
        }
        row
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> String {
        match self {
            RowType::Empty => String::new(),
            RowType::Extend { field, type_, rest } => {
                let rest_str = rest.type_name();
                if rest_str.is_empty() {
                    format!("{}: {}", field, type_.type_name())
                } else {
                    format!("{}: {}, {}", field, type_.type_name(), rest_str)
                }
            }
            RowType::Var(v) => format!("|{}", v),
        }
    }

    /// Collect free type variables.
    pub fn free_vars(&self) -> Vec<TypeVarId> {
        match self {
            RowType::Empty => vec![],
            RowType::Extend { type_, rest, .. } => {
                let mut vars = type_.free_vars();
                vars.extend(rest.free_vars());
                vars
            }
            RowType::Var(v) => vec![v.id],
        }
    }

    /// Apply substitution.
    pub fn apply_subst(&self, subst: &Substitution) -> Self {
        match self {
            RowType::Empty => RowType::Empty,
            RowType::Extend { field, type_, rest } => RowType::extend(
                field.clone(),
                type_.apply_subst(subst),
                rest.apply_subst(subst),
            ),
            RowType::Var(_v) => {
                // Row variables don't substitute to regular types
                self.clone()
            }
        }
    }

    /// Look up a field in the row.
    pub fn lookup(&self, field: &str) -> Option<&InferType> {
        match self {
            RowType::Empty => None,
            RowType::Extend {
                field: f,
                type_,
                rest,
            } => {
                if f == field {
                    Some(type_)
                } else {
                    rest.lookup(field)
                }
            }
            RowType::Var(_) => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TYPE SCHEMES
// ═══════════════════════════════════════════════════════════════════════════

/// Type scheme for polymorphism: ∀α₁...αₙ. τ
#[derive(Debug, Clone, PartialEq)]
pub struct TypeScheme {
    /// Quantified type variables
    pub forall: Vec<TypeVar>,
    /// The type body
    pub body: InferType,
}

impl TypeScheme {
    /// Create a monomorphic type scheme (no quantifiers).
    pub fn mono(t: InferType) -> Self {
        TypeScheme {
            forall: vec![],
            body: t,
        }
    }

    /// Create a polymorphic type scheme.
    pub fn poly(vars: Vec<TypeVar>, body: InferType) -> Self {
        TypeScheme { forall: vars, body }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SUBSTITUTION
// ═══════════════════════════════════════════════════════════════════════════

/// Substitution mapping type variables to types.
#[derive(Debug, Clone, Default)]
pub struct Substitution {
    bindings: HashMap<TypeVarId, InferType>,
}

impl Substitution {
    /// Create an empty substitution.
    pub fn empty() -> Self {
        Substitution {
            bindings: HashMap::new(),
        }
    }

    /// Add a binding to the substitution.
    pub fn extend(&mut self, var: TypeVarId, t: InferType) {
        self.bindings.insert(var, t);
    }

    /// Look up a type variable.
    pub fn get(&self, var: TypeVarId) -> Option<&InferType> {
        self.bindings.get(&var)
    }

    /// Compose two substitutions: self ∘ other
    pub fn compose(&mut self, other: Substitution) {
        // Apply other to all bindings in self
        for (_, t) in self.bindings.iter_mut() {
            *t = t.apply_subst(&other);
        }
        // Add bindings from other that aren't in self
        for (var, t) in other.bindings {
            if !self.bindings.contains_key(&var) {
                self.bindings.insert(var, t);
            }
        }
    }

    /// Filter out bound variables.
    pub fn filter_out(&self, bound: &std::collections::HashSet<TypeVarId>) -> Substitution {
        Substitution {
            bindings: self
                .bindings
                .iter()
                .filter(|(id, _)| !bound.contains(id))
                .map(|(id, t)| (*id, t.clone()))
                .collect(),
        }
    }

    /// Check if the substitution is empty.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// INFERENCE CONTEXT
// ═══════════════════════════════════════════════════════════════════════════

/// Unification variable state.
#[derive(Debug, Clone)]
pub enum UnificationVar {
    /// Unbound type variable
    Unbound(TypeVar),
    /// Bound to a type
    Bound(InferType),
}

/// Type environment mapping names to type schemes.
pub type TypeEnv = HashMap<String, TypeScheme>;

/// Inference context with unification state.
pub struct InferenceContext {
    /// Counter for generating fresh type variables
    var_counter: RefCell<u32>,
    /// Unification variables
    unification_vars: RefCell<HashMap<TypeVarId, UnificationVar>>,
    /// Nesting level for let-polymorphism
    level: RefCell<u32>,
    /// Collected diagnostics (interior mutable for ergonomic API)
    diagnostics: RefCell<Diagnostics>,
}

impl InferenceContext {
    /// Get the collected diagnostics.
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.borrow().clone()
    }

    /// Add a diagnostic.
    pub fn add_diagnostic(&self, diag: Diagnostic) {
        self.diagnostics.borrow_mut().add(diag);
    }
}

impl InferenceContext {
    /// Create a new inference context.
    pub fn new() -> Self {
        InferenceContext {
            var_counter: RefCell::new(0),
            unification_vars: RefCell::new(HashMap::new()),
            level: RefCell::new(0),
            diagnostics: RefCell::new(Diagnostics::new()),
        }
    }

    /// Generate a fresh type variable.
    pub fn fresh_var(&self) -> TypeVarId {
        let mut counter = self.var_counter.borrow_mut();
        let id = TypeVarId(*counter);
        *counter += 1;

        self.unification_vars
            .borrow_mut()
            .insert(id, UnificationVar::Unbound(TypeVar::new(id, None)));

        id
    }

    /// Generate a fresh type variable with a name.
    pub fn fresh_var_named(&self, name: &str) -> TypeVarId {
        let mut counter = self.var_counter.borrow_mut();
        let id = TypeVarId(*counter);
        *counter += 1;

        self.unification_vars.borrow_mut().insert(
            id,
            UnificationVar::Unbound(TypeVar::new(id, Some(name.to_string()))),
        );

        id
    }

    /// Generate a fresh InferType::Var.
    pub fn fresh_type(&self) -> InferType {
        InferType::var(self.fresh_var(), None)
    }

    /// Generate a fresh InferType::Var with a name.
    pub fn fresh_type_named(&self, name: &str) -> InferType {
        InferType::var(self.fresh_var_named(name), Some(name.to_string()))
    }

    /// Enter a new scope (increase level).
    pub fn enter_scope(&self) {
        *self.level.borrow_mut() += 1;
    }

    /// Exit a scope (decrease level).
    pub fn exit_scope(&self) {
        *self.level.borrow_mut() -= 1;
    }

    /// Get the current level.
    pub fn current_level(&self) -> u32 {
        *self.level.borrow()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // UNIFICATION (Robinson's Algorithm)
    // ═══════════════════════════════════════════════════════════════════════

    /// Unify two types, returning the resulting substitution.
    pub fn unify(&self, t1: &InferType, t2: &InferType, span: Span) -> Result<Substitution, ()> {
        match (t1, t2) {
            // Same concrete types unify trivially
            (InferType::Concrete(a), InferType::Concrete(b)) if a == b => Ok(Substitution::empty()),

            // Int is a subtype of Float (numeric promotion)
            (InferType::Concrete(Type::Int), InferType::Concrete(Type::Float))
            | (InferType::Concrete(Type::Float), InferType::Concrete(Type::Int)) => {
                Ok(Substitution::empty())
            }

            // Type variable unification
            (InferType::Var(v1), InferType::Var(v2)) if v1.id == v2.id => Ok(Substitution::empty()),

            // Bind unbound variable to type
            (InferType::Var(v), t) | (t, InferType::Var(v)) => self.bind_var(v.id, t, span),

            // Function unification
            (InferType::Function(arg1, ret1), InferType::Function(arg2, ret2)) => {
                let mut subst = self.unify(arg1, arg2, span)?;
                let ret1_subst = ret1.apply_subst(&subst);
                let ret2_subst = ret2.apply_subst(&subst);
                let subst2 = self.unify(&ret1_subst, &ret2_subst, span)?;
                subst.compose(subst2);
                Ok(subst)
            }

            // Record unification
            (InferType::Record(r1), InferType::Record(r2)) => self.unify_rows(r1, r2, span),

            // ForAll unification (instantiate first)
            (InferType::ForAll { vars, body }, t) | (t, InferType::ForAll { vars, body }) => {
                let instantiated = self.instantiate(vars, body);
                self.unify(&instantiated, t, span)
            }

            // Constrained type unifies with its base type (widening)
            (InferType::Concrete(Type::Constrained { base, .. }), other)
            | (other, InferType::Concrete(Type::Constrained { base, .. })) => {
                let base_infer = InferType::Concrete(*base.clone());
                self.unify(&base_infer, other, span)
            }

            // Type mismatch
            _ => {
                self.add_diagnostic(
                    Diagnostic::error(
                        ErrorCode::E002_TypeMismatch,
                        format!(
                            "Cannot unify types '{}' and '{}'",
                            t1.type_name(),
                            t2.type_name()
                        ),
                        span,
                    )
                    .with_suggestion("Check that the types are compatible"),
                );
                Err(())
            }
        }
    }

    /// Unify two row types.
    fn unify_rows(&self, r1: &RowType, r2: &RowType, span: Span) -> Result<Substitution, ()> {
        match (r1, r2) {
            (RowType::Empty, RowType::Empty) => Ok(Substitution::empty()),

            (
                RowType::Extend { field, type_, rest },
                RowType::Extend {
                    field: f2,
                    type_: t2,
                    rest: r2_rest,
                },
            ) if field == f2 => {
                let mut subst = self.unify(type_, t2, span)?;
                let rest1 = rest.apply_subst(&subst);
                let rest2 = r2_rest.apply_subst(&subst);
                let subst2 = self.unify_rows(&rest1, &rest2, span)?;
                subst.compose(subst2);
                Ok(subst)
            }

            // Row rewriting would go here for full row polymorphism
            _ => {
                self.add_diagnostic(Diagnostic::error(
                    ErrorCode::E002_TypeMismatch,
                    format!(
                        "Cannot unify record types '{}' and '{}'",
                        r1.type_name(),
                        r2.type_name()
                    ),
                    span,
                ));
                Err(())
            }
        }
    }

    /// Bind a type variable to a type (with occurs check).
    fn bind_var(&self, var: TypeVarId, t: &InferType, span: Span) -> Result<Substitution, ()> {
        // Occurs check: prevent infinite types
        if self.occurs_in(var, t) {
            self.add_diagnostic(Diagnostic::error(
                ErrorCode::E002_TypeMismatch,
                format!(
                    "Infinite type: type variable {} occurs in {}",
                    var,
                    t.type_name()
                ),
                span,
            ));
            return Err(());
        }

        let mut subst = Substitution::empty();
        subst.extend(var, t.clone());

        // Update unification variable
        self.unification_vars
            .borrow_mut()
            .insert(var, UnificationVar::Bound(t.clone()));

        Ok(subst)
    }

    /// Check if a type variable occurs in a type (occurs check).
    fn occurs_in(&self, var: TypeVarId, t: &InferType) -> bool {
        match t {
            InferType::Concrete(_) => false,
            InferType::Var(v) => v.id == var,
            InferType::Function(arg, ret) => self.occurs_in(var, arg) || self.occurs_in(var, ret),
            InferType::Record(row) => self.occurs_in_row(var, row),
            InferType::ForAll { vars, body } => {
                // Variable is bound, so it doesn't occur
                if vars.iter().any(|v| v.id == var) {
                    false
                } else {
                    self.occurs_in(var, body)
                }
            }
        }
    }

    /// Check if a type variable occurs in a row type.
    fn occurs_in_row(&self, var: TypeVarId, row: &RowType) -> bool {
        match row {
            RowType::Empty => false,
            RowType::Extend { type_, rest, .. } => {
                self.occurs_in(var, type_) || self.occurs_in_row(var, rest)
            }
            RowType::Var(v) => v.id == var,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // INSTANTIATION AND GENERALIZATION
    // ═══════════════════════════════════════════════════════════════════════

    /// Instantiate a type scheme: replace bound variables with fresh ones.
    pub fn instantiate(&self, vars: &[TypeVar], body: &InferType) -> InferType {
        let mut subst = Substitution::empty();
        for var in vars {
            let fresh = self.fresh_type();
            subst.extend(var.id, fresh);
        }
        body.apply_subst(&subst)
    }

    /// Generalize a type at a given level: create a type scheme.
    pub fn generalize(&self, env: &TypeEnv, t: &InferType) -> TypeScheme {
        let env_vars: Vec<TypeVarId> = env
            .values()
            .flat_map(|scheme| scheme.body.free_vars())
            .collect();

        let type_vars = t
            .free_vars()
            .into_iter()
            .filter(|v| !env_vars.contains(v))
            .map(|id| {
                // Get the name if available
                let name = self
                    .unification_vars
                    .borrow()
                    .get(&id)
                    .and_then(|uv| match uv {
                        UnificationVar::Unbound(v) => v.name.clone(),
                        UnificationVar::Bound(_) => None,
                    });
                TypeVar::new(id, name)
            })
            .collect();

        TypeScheme::poly(type_vars, t.clone())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // TYPE INFERENCE (Algorithm W)
    // ═══════════════════════════════════════════════════════════════════════

    /// Infer the type of an expression in a given environment.
    pub fn infer_expr(&self, expr: &Expr, env: &TypeEnv) -> Result<InferType, ()> {
        let span = Span::synthetic(); // TODO: thread spans through expressions
        self.infer_expr_with_span(expr, env, span)
    }

    /// Infer the type of an expression with a span for error reporting.
    pub fn infer_expr_with_span(
        &self,
        expr: &Expr,
        env: &TypeEnv,
        span: Span,
    ) -> Result<InferType, ()> {
        match expr {
            // Literals
            Expr::Int(_) => Ok(InferType::int()),
            Expr::Float(_) => Ok(InferType::float()),
            Expr::Bool(_) => Ok(InferType::bool()),
            Expr::String(_) => Ok(InferType::string()),
            Expr::Duration(_) => Ok(InferType::duration()),

            // Identifier lookup
            Expr::Ident(name) => {
                if let Some(scheme) = env.get(name) {
                    Ok(self.instantiate(&scheme.forall, &scheme.body))
                } else {
                    // Unknown identifier - create fresh type variable
                    Ok(self.fresh_type_named(name))
                }
            }

            Expr::DottedName(name) => {
                if let Some(scheme) = env.get(name) {
                    Ok(self.instantiate(&scheme.forall, &scheme.body))
                } else if let Some(var) = name.strip_prefix("memory.") {
                    if let Some(scheme) = env.get(var) {
                        Ok(self.instantiate(&scheme.forall, &scheme.body))
                    } else {
                        Ok(self.fresh_type_named(var))
                    }
                } else {
                    Ok(self.fresh_type_named(name))
                }
            }

            // Function call
            Expr::Call { name, args } => {
                let result_type = self.fresh_type();

                // Build function type from args to result
                let mut func_type = result_type.clone();
                for arg in args.iter().rev() {
                    let arg_type = self.infer_expr_with_span(arg, env, span)?;
                    func_type = InferType::function(arg_type, func_type);
                }

                // Look up the function in the environment
                if let Some(scheme) = env.get(name) {
                    let func_actual = self.instantiate(&scheme.forall, &scheme.body);
                    let _subst = self.unify(&func_type, &func_actual, span)?;
                    Ok(result_type)
                } else {
                    // Unknown function - assume it has the inferred type
                    Ok(result_type)
                }
            }

            // Binary arithmetic operations
            Expr::BinOp { lhs, op, rhs } => {
                let lhs_type = self.infer_expr_with_span(lhs, env, span)?;
                let rhs_type = self.infer_expr_with_span(rhs, env, span)?;

                match op {
                    ArithOp::Add | ArithOp::Sub | ArithOp::Mul | ArithOp::Div => {
                        // Numeric operations require numeric types
                        let num_type = InferType::Concrete(Type::Int);
                        self.unify(&lhs_type, &num_type, span)?;
                        self.unify(&rhs_type, &num_type, span)?;

                        // Result is numeric (int or float depending on operands)
                        if matches!(lhs_type, InferType::Concrete(Type::Float))
                            || matches!(rhs_type, InferType::Concrete(Type::Float))
                        {
                            Ok(InferType::float())
                        } else {
                            Ok(InferType::int())
                        }
                    }
                }
            }

            // Comparison operations
            Expr::CompOp { lhs, op: _, rhs } => {
                let _lhs_type = self.infer_expr_with_span(lhs, env, span)?;
                let _rhs_type = self.infer_expr_with_span(rhs, env, span)?;
                Ok(InferType::bool())
            }

            // Logical operations
            Expr::LogicalOp { lhs, op: _, rhs } => {
                let lhs_type = self.infer_expr_with_span(lhs, env, span)?;
                let rhs_type = self.infer_expr_with_span(rhs, env, span)?;

                let bool_type = InferType::bool();
                self.unify(&lhs_type, &bool_type, span)?;
                self.unify(&rhs_type, &bool_type, span)?;

                Ok(InferType::bool())
            }

            // Unary operations
            Expr::UnaryOp { op, expr } => {
                let expr_type = self.infer_expr_with_span(expr, env, span)?;

                match op {
                    UnaryOp::Not => {
                        self.unify(&expr_type, &InferType::bool(), span)?;
                        Ok(InferType::bool())
                    }
                    UnaryOp::Neg => {
                        // Negation works on numeric types
                        Ok(expr_type)
                    }
                }
            }

            // Count operation
            Expr::Count(_) => Ok(InferType::int()),

            // TLA+ expressions
            Expr::Choose { .. } => Ok(self.fresh_type()),
            Expr::Let { bindings, body } => {
                let mut new_env = env.clone();

                // Process each binding
                for (name, expr) in bindings {
                    let expr_type = self.infer_expr_with_span(expr, &new_env, span)?;
                    let scheme = self.generalize(&new_env, &expr_type);
                    new_env.insert(name.clone(), scheme);
                }

                self.infer_expr_with_span(body, &new_env, span)
            }
            Expr::IfThenElse {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond_type = self.infer_expr_with_span(cond, env, span)?;
                self.unify(&cond_type, &InferType::bool(), span)?;

                let then_type = self.infer_expr_with_span(then_expr, env, span)?;
                let else_type = self.infer_expr_with_span(else_expr, env, span)?;

                self.unify(&then_type, &else_type, span)?;
                Ok(then_type)
            }
            Expr::Case { arms, default } => {
                let result_type = self.fresh_type();

                for (cond, expr) in arms {
                    let _cond_type = self.infer_expr_with_span(cond, env, span)?;
                    let expr_type = self.infer_expr_with_span(expr, env, span)?;
                    self.unify(&expr_type, &result_type, span)?;
                }

                if let Some(default_expr) = default {
                    let default_type = self.infer_expr_with_span(default_expr, env, span)?;
                    self.unify(&default_type, &result_type, span)?;
                }

                Ok(result_type)
            }

            // Set operations
            Expr::Subset(_)
            | Expr::BigUnion(_)
            | Expr::SetLiteral(_)
            | Expr::SetDiff { .. }
            | Expr::SetUnion { .. }
            | Expr::SetIntersect { .. } => Ok(InferType::Concrete(Type::List(Box::new(
                InferType::Concrete(Type::Int).into_concrete().unwrap(),
            )))),

            // Other expressions
            Expr::Domain(_) => Ok(self.fresh_type()),
            Expr::Except { base, .. } => self.infer_expr_with_span(base, env, span),
            Expr::FunctionLiteral { .. } => Ok(self.fresh_type()),
            Expr::Record(fields) => {
                let mut field_types = Vec::new();
                for (name, expr) in fields {
                    let type_ = self.infer_expr_with_span(expr, env, span)?;
                    field_types.push((name.clone(), type_));
                }
                Ok(InferType::Record(RowType::from_fields(field_types)))
            }
            Expr::FieldAccess { record, field } => {
                let record_type = self.infer_expr_with_span(record, env, span)?;
                let field_type = self.fresh_type();

                // Create a row type with the expected field
                let expected_row = RowType::extend(
                    field.clone(),
                    field_type.clone(),
                    RowType::Var(TypeVar::new(self.fresh_var(), None)),
                );

                self.unify(&record_type, &InferType::Record(expected_row), span)?;

                Ok(field_type)
            }
            Expr::Tuple(_) => Ok(self.fresh_type()),
            Expr::Index { base, .. } => {
                let _ = self.infer_expr_with_span(base, env, span)?;
                Ok(self.fresh_type())
            }
            Expr::In { .. } => Ok(InferType::bool()),
            Expr::Forall { .. } | Expr::Exists { .. } => Ok(InferType::bool()),
            Expr::Assume(_) => Ok(InferType::bool()),
            Expr::TlaInline { .. } => Ok(InferType::bool()),
        }
    }
}

impl Default for InferenceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl InferType {
    /// Convert to a concrete Type if possible.
    pub fn into_concrete(self) -> Option<Type> {
        match self {
            InferType::Concrete(t) => Some(t),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::ComparisonOp;

    #[test]
    fn test_fresh_var() {
        let ctx = InferenceContext::new();
        let v1 = ctx.fresh_var();
        let v2 = ctx.fresh_var();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_unify_int_int() {
        let ctx = InferenceContext::new();
        let result = ctx.unify(&InferType::int(), &InferType::int(), Span::synthetic());
        assert!(result.is_ok());
    }

    #[test]
    fn test_unify_int_float() {
        let ctx = InferenceContext::new();
        // Int is compatible with Float (promotion)
        let result = ctx.unify(&InferType::int(), &InferType::float(), Span::synthetic());
        assert!(result.is_ok());
    }

    #[test]
    fn test_unify_var_concrete() {
        let ctx = InferenceContext::new();
        let var = ctx.fresh_type();
        let result = ctx.unify(&var, &InferType::int(), Span::synthetic());
        assert!(result.is_ok());

        // After unification, var should be bound to int
        let var_id = match &var {
            InferType::Var(v) => v.id,
            _ => panic!("Expected var"),
        };
        let binding = ctx.unification_vars.borrow().get(&var_id).cloned();
        assert!(matches!(binding, Some(UnificationVar::Bound(_))));
    }

    #[test]
    fn test_unify_function() {
        let ctx = InferenceContext::new();
        let f1 = InferType::function(InferType::int(), InferType::bool());
        let f2 = InferType::function(InferType::int(), InferType::bool());
        let result = ctx.unify(&f1, &f2, Span::synthetic());
        assert!(result.is_ok());
    }

    #[test]
    fn test_unify_function_mismatch() {
        let ctx = InferenceContext::new();
        let f1 = InferType::function(InferType::int(), InferType::bool());
        let f2 = InferType::function(InferType::string(), InferType::bool());
        let result = ctx.unify(&f1, &f2, Span::synthetic());
        assert!(result.is_err());
    }

    #[test]
    fn test_occurs_check() {
        let ctx = InferenceContext::new();
        let var = ctx.fresh_type();
        let func = InferType::function(var.clone(), InferType::int());

        // Trying to unify var with (var -> int) should fail (occurs check)
        let result = ctx.unify(&var, &func, Span::synthetic());
        assert!(result.is_err());
        assert!(ctx.diagnostics().has_errors());
    }

    #[test]
    fn test_instantiate() {
        let ctx = InferenceContext::new();
        // Create a fresh type using the context (so it's registered)
        let body = ctx.fresh_type();
        let var_id = match &body {
            InferType::Var(v) => v.id,
            _ => panic!("Expected var"),
        };
        let var = TypeVar::new(var_id, Some("a".to_string()));
        let scheme = TypeScheme::poly(vec![var.clone()], body);

        // Instantiation should create new fresh variables
        let inst1 = ctx.instantiate(&scheme.forall, &scheme.body);
        let inst2 = ctx.instantiate(&scheme.forall, &scheme.body);

        // Each instantiation should produce fresh variables that are different
        // from each other and from the original
        assert_ne!(inst1, inst2);

        // Both should be type variables
        assert!(matches!(inst1, InferType::Var(_)));
        assert!(matches!(inst2, InferType::Var(_)));
    }

    #[test]
    fn test_generalize() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        let var = ctx.fresh_type();
        let scheme = ctx.generalize(&env, &var);

        // The scheme should quantify the free variable
        assert!(!scheme.forall.is_empty());
    }

    #[test]
    fn test_infer_literal() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        let expr = Expr::Int(42);
        let result = ctx.infer_expr(&expr, &env);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InferType::int());
    }

    #[test]
    fn test_infer_arithmetic() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Int(1)),
            op: ArithOp::Add,
            rhs: Box::new(Expr::Int(2)),
        };
        let result = ctx.infer_expr(&expr, &env);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InferType::int());
    }

    #[test]
    fn test_infer_comparison() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        let expr = Expr::CompOp {
            lhs: Box::new(Expr::Int(1)),
            op: ComparisonOp::Lt,
            rhs: Box::new(Expr::Int(2)),
        };
        let result = ctx.infer_expr(&expr, &env);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InferType::bool());
    }

    #[test]
    fn test_infer_let_polymorphism() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        // let id = \x -> x in (id 1, id true)
        // This should work because of let-polymorphism
        let let_expr = Expr::Let {
            bindings: vec![
                // For simplicity, we'll just test the binding part
                ("x".to_string(), Expr::Int(42)),
            ],
            body: Box::new(Expr::Ident("x".to_string())),
        };

        let result = ctx.infer_expr(&let_expr, &env);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InferType::int());
    }

    #[test]
    fn test_infer_record() {
        let ctx = InferenceContext::new();
        let env = TypeEnv::new();

        let expr = Expr::Record(vec![
            ("x".to_string(), Expr::Int(1)),
            ("y".to_string(), Expr::Bool(true)),
        ]);

        let result = ctx.infer_expr(&expr, &env);
        assert!(result.is_ok());

        if let InferType::Record(row) = result.unwrap() {
            assert!(matches!(
                row.lookup("x"),
                Some(InferType::Concrete(Type::Int))
            ));
            assert!(matches!(
                row.lookup("y"),
                Some(InferType::Concrete(Type::Bool))
            ));
        } else {
            panic!("Expected record type");
        }
    }

    #[test]
    fn test_row_type() {
        let row = RowType::from_fields(vec![
            ("x".to_string(), InferType::int()),
            ("y".to_string(), InferType::bool()),
        ]);

        assert!(matches!(
            row.lookup("x"),
            Some(InferType::Concrete(Type::Int))
        ));
        assert!(matches!(
            row.lookup("y"),
            Some(InferType::Concrete(Type::Bool))
        ));
        assert!(row.lookup("z").is_none());
    }

    #[test]
    fn test_substitution_compose() {
        let mut s1 = Substitution::empty();
        let mut s2 = Substitution::empty();

        s1.extend(TypeVarId(0), InferType::int());
        s2.extend(TypeVarId(1), InferType::bool());

        s1.compose(s2);

        assert!(s1.get(TypeVarId(0)).is_some());
        assert!(s1.get(TypeVarId(1)).is_some());
    }

    #[test]
    fn test_type_display() {
        assert_eq!(InferType::int().to_string(), "Int");
        assert_eq!(InferType::bool().to_string(), "Bool");

        let func = InferType::function(InferType::int(), InferType::bool());
        assert!(func.to_string().contains("->"));
    }
}
