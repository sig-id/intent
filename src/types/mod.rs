//! Type system for the Intent language.
//!
//! This module provides type representations and checking for:
//! - Pattern parameters and applications
//! - Expression type inference
//! - Type compatibility checking

pub mod checker;

use std::fmt;

/// A qualified name with optional path segments (e.g., `std.patterns.Retry`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedName {
    /// Path segments (e.g., ["std", "patterns", "Retry"])
    pub segments: Vec<String>,
    /// Source code span
    pub span: crate::diagnostic::Span,
}

impl QualifiedName {
    /// Create a new qualified name from segments.
    pub fn new(segments: Vec<String>, span: crate::diagnostic::Span) -> Self {
        Self { segments, span }
    }

    /// Create a qualified name from a single identifier.
    pub fn simple(name: impl Into<String>, span: crate::diagnostic::Span) -> Self {
        Self {
            segments: vec![name.into()],
            span,
        }
    }

    /// Create a qualified name from dotted string.
    pub fn from_dotted(dotted: &str, span: crate::diagnostic::Span) -> Self {
        Self {
            segments: dotted.split('.').map(|s| s.to_string()).collect(),
            span,
        }
    }

    /// Get the simple name (last segment).
    pub fn name(&self) -> &str {
        self.segments.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// Get the namespace (all segments except last).
    pub fn namespace(&self) -> &[String] {
        if self.segments.is_empty() {
            &[]
        } else {
            &self.segments[..self.segments.len() - 1]
        }
    }

    /// Check if this is a simple name (single segment).
    pub fn is_simple(&self) -> bool {
        self.segments.len() == 1
    }

    /// Convert to dotted string representation.
    pub fn to_dotted(&self) -> String {
        self.segments.join(".")
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_dotted())
    }
}

/// Type representation for the Intent language.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Integer type
    Int,
    /// Floating point type
    Float,
    /// Boolean type
    Bool,
    /// String type
    String,
    /// Duration type (milliseconds internally)
    Duration,
    /// Entity reference
    Entity,
    /// List of entities
    EntityList,
    /// State name
    State,
    /// Event name
    Event,
    /// List of events
    EventList,
    /// Component reference
    Component,
    /// Behavior reference
    Behavior,
    /// Pattern reference
    Pattern,
    /// List type with element type
    List(Box<Type>),
    /// Optional type
    Optional(Box<Type>),
    /// Named type (user-defined)
    Named(QualifiedName),
    /// Type variable (for generics)
    Var(String),
}

impl Type {
    /// Check if this is a primitive type.
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Duration
        )
    }

    /// Check if this is a collection type.
    pub fn is_collection(&self) -> bool {
        matches!(self, Type::List(_) | Type::EntityList | Type::EventList)
    }

    /// Get a string representation of the type.
    pub fn type_name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Duration => "Duration".to_string(),
            Type::Entity => "Entity".to_string(),
            Type::EntityList => "EntityList".to_string(),
            Type::State => "State".to_string(),
            Type::Event => "Event".to_string(),
            Type::EventList => "EventList".to_string(),
            Type::Component => "Component".to_string(),
            Type::Behavior => "Behavior".to_string(),
            Type::Pattern => "Pattern".to_string(),
            Type::List(inner) => format!("List<{}>", inner.type_name()),
            Type::Optional(inner) => format!("{}?", inner.type_name()),
            Type::Named(name) => name.to_string(),
            Type::Var(v) => v.clone(),
        }
    }

    /// Parse a type from a string name.
    pub fn from_name(name: &str) -> Option<Type> {
        match name {
            "Int" => Some(Type::Int),
            "Float" => Some(Type::Float),
            "Bool" => Some(Type::Bool),
            "String" => Some(Type::String),
            "Duration" => Some(Type::Duration),
            "Entity" => Some(Type::Entity),
            "EntityList" => Some(Type::EntityList),
            "State" => Some(Type::State),
            "Event" => Some(Type::Event),
            "EventList" => Some(Type::EventList),
            "Component" => Some(Type::Component),
            "Behavior" => Some(Type::Behavior),
            "Pattern" => Some(Type::Pattern),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_name())
    }
}

/// Type annotation in source code.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    /// Simple type name (e.g., `Int`, `String`)
    Simple {
        name: String,
        span: crate::diagnostic::Span,
    },
    /// Generic type with arguments (e.g., `List<Int>`)
    Generic {
        name: String,
        args: Vec<TypeAnnotation>,
        span: crate::diagnostic::Span,
    },
    /// Optional type (e.g., `String?`)
    Optional {
        inner: Box<TypeAnnotation>,
        span: crate::diagnostic::Span,
    },
}

impl TypeAnnotation {
    /// Create a simple type annotation.
    pub fn simple(name: impl Into<String>, span: crate::diagnostic::Span) -> Self {
        Self::Simple {
            name: name.into(),
            span,
        }
    }

    /// Create a generic type annotation.
    pub fn generic(
        name: impl Into<String>,
        args: Vec<TypeAnnotation>,
        span: crate::diagnostic::Span,
    ) -> Self {
        Self::Generic {
            name: name.into(),
            args,
            span,
        }
    }

    /// Create an optional type annotation.
    pub fn optional(inner: TypeAnnotation, span: crate::diagnostic::Span) -> Self {
        Self::Optional {
            inner: Box::new(inner),
            span,
        }
    }

    /// Get the span of this annotation.
    pub fn span(&self) -> crate::diagnostic::Span {
        match self {
            TypeAnnotation::Simple { span, .. } => *span,
            TypeAnnotation::Generic { span, .. } => *span,
            TypeAnnotation::Optional { span, .. } => *span,
        }
    }

    /// Convert to a resolved Type.
    pub fn to_type(&self) -> Type {
        match self {
            TypeAnnotation::Simple { name, .. } => {
                Type::from_name(name).unwrap_or_else(|| Type::Named(QualifiedName::simple(
                    name.clone(),
                    crate::diagnostic::Span::synthetic(),
                )))
            }
            TypeAnnotation::Generic { name, args, .. } => {
                if name == "List" && args.len() == 1 {
                    Type::List(Box::new(args[0].to_type()))
                } else {
                    Type::Named(QualifiedName::simple(
                        format!("{}<{}>", name, args.iter().map(|a| a.to_type().type_name()).collect::<Vec<_>>().join(", ")),
                        crate::diagnostic::Span::synthetic(),
                    ))
                }
            }
            TypeAnnotation::Optional { inner, .. } => {
                Type::Optional(Box::new(inner.to_type()))
            }
        }
    }
}

impl fmt::Display for TypeAnnotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeAnnotation::Simple { name, .. } => write!(f, "{}", name),
            TypeAnnotation::Generic { name, args, .. } => {
                let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                write!(f, "{}<{}>", name, args_str.join(", "))
            }
            TypeAnnotation::Optional { inner, .. } => write!(f, "{}?", inner),
        }
    }
}

/// Type parameter with optional bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    /// Parameter name
    pub name: String,
    /// Optional bounds (e.g., Entity, State, Event)
    pub bounds: Vec<TypeBound>,
    /// Source span
    pub span: crate::diagnostic::Span,
}

/// Type bound for generic parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeBound {
    /// Must be an entity type
    Entity,
    /// Must be a state type
    State,
    /// Must be an event type
    Event,
    /// Must be a component type
    Component,
    /// Must be a behavior type
    Behavior,
    /// Custom bound with name
    Custom(String),
}

impl fmt::Display for TypeBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeBound::Entity => write!(f, "Entity"),
            TypeBound::State => write!(f, "State"),
            TypeBound::Event => write!(f, "Event"),
            TypeBound::Component => write!(f, "Component"),
            TypeBound::Behavior => write!(f, "Behavior"),
            TypeBound::Custom(name) => write!(f, "{}", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qualified_name() {
        let name = QualifiedName::from_dotted("std.patterns.Retry", crate::diagnostic::Span::synthetic());
        assert_eq!(name.segments, vec!["std", "patterns", "Retry"]);
        assert_eq!(name.name(), "Retry");
        assert_eq!(name.namespace(), &["std", "patterns"]);
        assert!(!name.is_simple());
        assert_eq!(name.to_string(), "std.patterns.Retry");
    }

    #[test]
    fn test_simple_qualified_name() {
        let name = QualifiedName::simple("Retry", crate::diagnostic::Span::synthetic());
        assert_eq!(name.segments, vec!["Retry"]);
        assert_eq!(name.name(), "Retry");
        assert!(name.namespace().is_empty());
        assert!(name.is_simple());
    }

    #[test]
    fn test_type_from_name() {
        assert_eq!(Type::from_name("Int"), Some(Type::Int));
        assert_eq!(Type::from_name("Duration"), Some(Type::Duration));
        assert_eq!(Type::from_name("EntityList"), Some(Type::EntityList));
        assert_eq!(Type::from_name("Unknown"), None);
    }

    #[test]
    fn test_type_display() {
        assert_eq!(Type::Int.to_string(), "Int");
        assert_eq!(Type::List(Box::new(Type::Int)).to_string(), "List<Int>");
        assert_eq!(Type::Optional(Box::new(Type::String)).to_string(), "String?");
    }

    #[test]
    fn test_type_annotation() {
        let ann = TypeAnnotation::simple("Int", crate::diagnostic::Span::synthetic());
        assert_eq!(ann.to_string(), "Int");
        assert_eq!(ann.to_type(), Type::Int);
    }

    #[test]
    fn test_generic_type_annotation() {
        let ann = TypeAnnotation::generic(
            "List",
            vec![TypeAnnotation::simple("Int", crate::diagnostic::Span::synthetic())],
            crate::diagnostic::Span::synthetic(),
        );
        assert_eq!(ann.to_string(), "List<Int>");
        assert_eq!(ann.to_type(), Type::List(Box::new(Type::Int)));
    }
}
