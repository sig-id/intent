//! Test that standard library patterns parse correctly.

use intent::parser;

#[test]
fn parse_simple_pattern_with_default() {
    let source = r#"
pattern Test {
    parameters {
        max_count: Int { default: 3 }
    }
    behavior TestBehavior {
        states { a b }
    }
}
"#;
    let result = parser::parse(source);
    assert!(result.is_ok(), "Failed to parse simple pattern: {:?}", result.err());
}

#[test]
fn parse_stdlib_patterns() {
    let source = include_str!("../stdlib/patterns.intent");
    let result = parser::parse(source);
    assert!(result.is_ok(), "Failed to parse stdlib/patterns.intent: {:?}", result.err());
}

#[test]
fn parse_stdlib_types() {
    let source = include_str!("../stdlib/std.intent");
    let result = parser::parse(source);
    assert!(result.is_ok(), "Failed to parse stdlib/std.intent: {:?}", result.err());
}

#[test]
fn count_stdlib_patterns() {
    let source = include_str!("../stdlib/patterns.intent");
    let result = parser::parse(source);
    assert!(result.is_ok(), "Failed to parse stdlib/patterns.intent: {:?}", result.err());

    let top_levels = result.unwrap();
    let pattern_count = top_levels.iter().filter(|t| matches!(t, parser::ast::TopLevel::Pattern(_))).count();
    assert_eq!(pattern_count, 19, "Expected 19 patterns in stdlib, found {}", pattern_count);
}
