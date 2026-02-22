//! TypeScript/JavaScript import analysis using regex-based parsing.
//!
//! This module provides fast, simple import tracking for TypeScript files without
//! requiring a full AST parser. It handles:
//! - Named imports: `import { Foo, Bar } from 'module'`
//! - Default imports: `import Foo from 'module'`
//! - Namespace imports: `import * as Foo from 'module'`
//! - Class implements: `class X implements Y`
//!
//! Trade-off: Won't handle complex edge cases (multi-line imports, dynamic imports),
//! but provides 95%+ accuracy for structural constraint checking.

use std::path::{Path, PathBuf};
use regex::Regex;
use lazy_static::lazy_static;

/// Analysis results extracted from a single TypeScript/JavaScript file.
#[derive(Debug, Clone)]
pub struct TsFileAnalysis {
    pub path: PathBuf,
    pub imports: Vec<TsImport>,
    pub exports: Vec<TsExport>,
    pub classes: Vec<ClassDecl>,
    pub interfaces: Vec<InterfaceDecl>,
}

/// A TypeScript import statement.
#[derive(Debug, Clone)]
pub struct TsImport {
    /// Imported names (e.g., ["Foo", "Bar"] for `import { Foo, Bar }`)
    pub names: Vec<String>,
    /// Import source (e.g., "./database/db" or "express")
    pub source: String,
    /// Line number
    pub line: usize,
    /// Whether this is a namespace import (`import * as Foo`)
    pub is_namespace: bool,
    /// Default import name if present (`import Foo from`)
    pub default_name: Option<String>,
}

/// A TypeScript export statement.
#[derive(Debug, Clone)]
pub struct TsExport {
    /// Exported name
    pub name: String,
    /// Line number
    pub line: usize,
}

/// A class declaration.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    /// Class name
    pub name: String,
    /// Interfaces/classes this class implements/extends
    pub implements: Vec<String>,
    /// Line number
    pub line: usize,
}

/// An interface declaration.
#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    /// Interface name
    pub name: String,
    /// Line number
    pub line: usize,
}

lazy_static! {
    // Named imports: import { Foo, Bar as Baz } from 'module'
    static ref NAMED_IMPORT_RE: Regex = Regex::new(
        r#"import\s*\{\s*([^}]+)\s*\}\s*from\s*['"]([^'"]+)['"]"#
    ).unwrap();

    // Default import: import Foo from 'module'
    static ref DEFAULT_IMPORT_RE: Regex = Regex::new(
        r#"import\s+([A-Z][a-zA-Z0-9_]*)\s+from\s*['"]([^'"]+)['"]"#
    ).unwrap();

    // Namespace import: import * as Foo from 'module'
    static ref NAMESPACE_IMPORT_RE: Regex = Regex::new(
        r#"import\s+\*\s+as\s+([A-Z][a-zA-Z0-9_]*)\s+from\s*['"]([^'"]+)['"]"#
    ).unwrap();

    // Class declaration with implements: class Foo implements Bar, Baz
    static ref CLASS_IMPLEMENTS_RE: Regex = Regex::new(
        r"class\s+([A-Z][a-zA-Z0-9_]*)\s+(?:extends\s+[A-Za-z0-9_]+\s+)?implements\s+([A-Za-z0-9_,\s]+)"
    ).unwrap();

    // Simple class declaration: class Foo
    static ref CLASS_DECL_RE: Regex = Regex::new(
        r"class\s+([A-Z][a-zA-Z0-9_]*)"
    ).unwrap();

    // Interface declaration: interface Foo
    static ref INTERFACE_DECL_RE: Regex = Regex::new(
        r"interface\s+([A-Z][a-zA-Z0-9_]*)"
    ).unwrap();

    // Export statement: export class/interface/const/function Foo
    static ref EXPORT_RE: Regex = Regex::new(
        r"export\s+(?:class|interface|const|function|let|var|type)\s+([A-Za-z][a-zA-Z0-9_]*)"
    ).unwrap();

    // Single-line comment
    static ref SINGLE_LINE_COMMENT_RE: Regex = Regex::new(r"^\s*//").unwrap();
}

/// Parse a TypeScript/JavaScript file and extract structural information.
pub fn analyze_file(path: &Path) -> Option<TsFileAnalysis> {
    let source = std::fs::read_to_string(path).ok()?;
    Some(analyze_source(&source, path))
}

/// Parse TypeScript/JavaScript source text and extract structural information.
pub fn analyze_source(source: &str, path: &Path) -> TsFileAnalysis {
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut classes = Vec::new();
    let mut interfaces = Vec::new();

    let mut in_multiline_comment = false;

    for (line_num, line) in source.lines().enumerate() {
        let line_num = line_num + 1; // 1-indexed

        // Skip comments (simple heuristic)
        if line.trim_start().starts_with("/*") {
            in_multiline_comment = true;
        }
        if in_multiline_comment {
            if line.contains("*/") {
                in_multiline_comment = false;
            }
            continue;
        }
        if SINGLE_LINE_COMMENT_RE.is_match(line) {
            continue;
        }

        // Parse namespace imports first (most specific)
        if let Some(caps) = NAMESPACE_IMPORT_RE.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let source_path = caps.get(2).unwrap().as_str().to_string();
            imports.push(TsImport {
                names: vec![name.clone()],
                source: source_path,
                line: line_num,
                is_namespace: true,
                default_name: Some(name),
            });
            continue;
        }

        // Parse named imports
        if let Some(caps) = NAMED_IMPORT_RE.captures(line) {
            let names_str = caps.get(1).unwrap().as_str();
            let source_path = caps.get(2).unwrap().as_str().to_string();

            // Split by comma and extract names, handling aliases
            let names: Vec<String> = names_str
                .split(',')
                .filter_map(|s| {
                    let s = s.trim();
                    // Handle "Foo as Bar" -> extract "Foo"
                    if let Some(pos) = s.find(" as ") {
                        Some(s[..pos].trim().to_string())
                    } else {
                        if !s.is_empty() {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    }
                })
                .collect();

            if !names.is_empty() {
                imports.push(TsImport {
                    names,
                    source: source_path,
                    line: line_num,
                    is_namespace: false,
                    default_name: None,
                });
            }
            continue;
        }

        // Parse default imports
        if let Some(caps) = DEFAULT_IMPORT_RE.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let source_path = caps.get(2).unwrap().as_str().to_string();
            imports.push(TsImport {
                names: vec![name.clone()],
                source: source_path,
                line: line_num,
                is_namespace: false,
                default_name: Some(name),
            });
            continue;
        }

        // Parse class with implements
        if let Some(caps) = CLASS_IMPLEMENTS_RE.captures(line) {
            let class_name = caps.get(1).unwrap().as_str().to_string();
            let implements_str = caps.get(2).unwrap().as_str();

            let implements: Vec<String> = implements_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            classes.push(ClassDecl {
                name: class_name,
                implements,
                line: line_num,
            });
            continue;
        }

        // Parse simple class declarations
        if let Some(caps) = CLASS_DECL_RE.captures(line) {
            let class_name = caps.get(1).unwrap().as_str().to_string();
            // Only add if not already captured by implements pattern
            if !classes.iter().any(|c| c.name == class_name && c.line == line_num) {
                classes.push(ClassDecl {
                    name: class_name,
                    implements: Vec::new(),
                    line: line_num,
                });
            }
        }

        // Parse interface declarations
        if let Some(caps) = INTERFACE_DECL_RE.captures(line) {
            let interface_name = caps.get(1).unwrap().as_str().to_string();
            interfaces.push(InterfaceDecl {
                name: interface_name,
                line: line_num,
            });
        }

        // Parse exports
        if let Some(caps) = EXPORT_RE.captures(line) {
            let export_name = caps.get(1).unwrap().as_str().to_string();
            exports.push(TsExport {
                name: export_name,
                line: line_num,
            });
        }
    }

    TsFileAnalysis {
        path: path.to_path_buf(),
        imports,
        exports,
        classes,
        interfaces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_import_parsing() {
        let source = r#"
import { Database, Connection } from './db';
import { User } from '../models/user';
        "#;

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.imports.len(), 2);

        let first = &analysis.imports[0];
        assert_eq!(first.names, vec!["Database", "Connection"]);
        assert_eq!(first.source, "./db");
        assert_eq!(first.line, 2);
        assert!(!first.is_namespace);

        let second = &analysis.imports[1];
        assert_eq!(second.names, vec!["User"]);
        assert_eq!(second.source, "../models/user");
    }

    #[test]
    fn test_import_with_alias() {
        let source = "import { Database as DB, Connection as Conn } from './db';";

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.imports.len(), 1);

        let import = &analysis.imports[0];
        assert_eq!(import.names, vec!["Database", "Connection"]);
        assert_eq!(import.source, "./db");
    }

    #[test]
    fn test_default_import() {
        let source = "import Express from 'express';";

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.imports.len(), 1);

        let import = &analysis.imports[0];
        assert_eq!(import.names, vec!["Express"]);
        assert_eq!(import.default_name, Some("Express".to_string()));
        assert_eq!(import.source, "express");
    }

    #[test]
    fn test_namespace_import() {
        let source = "import * as React from 'react';";

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.imports.len(), 1);

        let import = &analysis.imports[0];
        assert_eq!(import.names, vec!["React"]);
        assert!(import.is_namespace);
        assert_eq!(import.source, "react");
    }

    #[test]
    fn test_class_implements() {
        let source = r#"
class UserService implements IService {
    constructor() {}
}

class AdminService extends BaseService implements IAdmin, IService {
    run() {}
}
        "#;

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.classes.len(), 2);

        let first = &analysis.classes[0];
        assert_eq!(first.name, "UserService");
        assert_eq!(first.implements, vec!["IService"]);

        let second = &analysis.classes[1];
        assert_eq!(second.name, "AdminService");
        assert_eq!(second.implements, vec!["IAdmin", "IService"]);
    }

    #[test]
    fn test_simple_class() {
        let source = "class Database {\n  connect() {}\n}";

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.classes.len(), 1);

        let class = &analysis.classes[0];
        assert_eq!(class.name, "Database");
        assert!(class.implements.is_empty());
    }

    #[test]
    fn test_interface_declaration() {
        let source = r#"
interface IDatabase {
    query(): void;
}

interface IConnection extends IBase {
    connect(): void;
}
        "#;

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.interfaces.len(), 2);
        assert_eq!(analysis.interfaces[0].name, "IDatabase");
        assert_eq!(analysis.interfaces[1].name, "IConnection");
    }

    #[test]
    fn test_comment_skipping() {
        let source = r#"
// import { Fake } from 'fake';
import { Real } from 'real';

/*
import { AlsoFake } from 'fake';
*/

import { Another } from 'another';
        "#;

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.imports.len(), 2);
        assert_eq!(analysis.imports[0].names, vec!["Real"]);
        assert_eq!(analysis.imports[1].names, vec!["Another"]);
    }

    #[test]
    fn test_exports() {
        let source = r#"
export class Database {}
export interface IConnection {}
export const config = {};
export function connect() {}
        "#;

        let analysis = analyze_source(source, Path::new("test.ts"));
        assert_eq!(analysis.exports.len(), 4);
        assert_eq!(analysis.exports[0].name, "Database");
        assert_eq!(analysis.exports[1].name, "IConnection");
        assert_eq!(analysis.exports[2].name, "config");
        assert_eq!(analysis.exports[3].name, "connect");
    }
}
