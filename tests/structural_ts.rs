//! Integration tests for TypeScript structural analysis.

use intent::structural::checker;
use intent::structural::index::CrateIndex;

#[test]
#[allow(clippy::unwrap_used)]
fn test_typescript_constraint_checking() {
    let tmp = tempfile::tempdir().unwrap();

    // Create TypeScript files
    let api_dir = tmp.path().join("api");
    let database_dir = tmp.path().join("database");
    std::fs::create_dir(&api_dir).unwrap();
    std::fs::create_dir(&database_dir).unwrap();

    // API service that imports from database
    std::fs::write(
        api_dir.join("service.ts"),
        r#"
import { Database } from '../database/db';
import { Connection } from '../database/connection';

export class ApiService {
    private db: Database;

    constructor() {
        this.db = new Database();
    }
}
"#,
    )
    .unwrap();

    // Database module
    std::fs::write(
        database_dir.join("db.ts"),
        r#"
export class Database {
    connect() {}
}
"#,
    )
    .unwrap();

    std::fs::write(
        database_dir.join("connection.ts"),
        r#"
export class Connection {
    open() {}
}
"#,
    )
    .unwrap();

    // Build index
    let index = CrateIndex::build(tmp.path()).unwrap();

    // Verify TypeScript files were indexed
    assert!(
        !index.ts_files.is_empty(),
        "Should have indexed TypeScript files"
    );
    assert_eq!(index.ts_files.len(), 3);

    // Verify entity references were captured
    assert!(
        index.entity_refs.contains_key("Database"),
        "Should have indexed Database entity"
    );
    assert!(
        index.entity_refs.contains_key("Connection"),
        "Should have indexed Connection entity"
    );

    // Check constraint: API must not reference Database
    let result = checker::must_not_ref::check(
        "no_db_access",
        "TestSystem",
        &["api".into()],
        &["Database".into(), "Connection".into()],
        &index,
    );

    // Should have violations (API imports Database and Connection)
    assert!(
        !result.holds,
        "Constraint should fail: API references Database and Connection"
    );
    assert!(
        !result.violations.is_empty(),
        "Should have violation entries"
    );

    // Verify violations point to the right file
    let service_path = api_dir.join("service.ts");
    let has_violation = result.violations.iter().any(|v| v.file == service_path);
    assert!(has_violation, "Should have violation in service.ts");
}

#[test]
#[allow(clippy::unwrap_used)]
fn test_mixed_rust_and_typescript() {
    let tmp = tempfile::tempdir().unwrap();

    // Create Rust files
    std::fs::write(
        tmp.path().join("lib.rs"),
        "mod services;\npub struct RustEntity;\nuse services::AuthMiddleware;\n",
    )
    .unwrap();

    let services = tmp.path().join("services");
    std::fs::create_dir(&services).unwrap();
    std::fs::write(
        services.join("mod.rs"),
        "pub struct AuthMiddleware;\npub fn get_auth() -> AuthMiddleware { todo!() }\n",
    )
    .unwrap();

    // Create TypeScript files
    let api_dir = tmp.path().join("api");
    std::fs::create_dir(&api_dir).unwrap();
    std::fs::write(
        api_dir.join("index.ts"),
        r#"
import { Service } from './service';

export class Api {
    private service: Service;
}
"#,
    )
    .unwrap();

    std::fs::write(
        api_dir.join("service.ts"),
        r#"
export class Service {
    handle() {}
}
"#,
    )
    .unwrap();

    // Build index
    let index = CrateIndex::build(tmp.path()).unwrap();

    // Verify both Rust and TypeScript files were indexed
    assert!(!index.rust_files.is_empty(), "Should have Rust files");
    assert!(!index.ts_files.is_empty(), "Should have TypeScript files");
    assert_eq!(index.rust_files.len(), 2, "Should have 2 Rust files");
    assert_eq!(index.ts_files.len(), 2, "Should have 2 TypeScript files");

    // Verify unified entity references contain entities from both languages
    assert!(
        index.entity_refs.contains_key("AuthMiddleware"),
        "Should have Rust entity"
    );
    assert!(
        index.entity_refs.contains_key("Service"),
        "Should have TypeScript entity"
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn test_unsupported_language() {
    let tmp = tempfile::tempdir().unwrap();

    // Create directory with Python files (unsupported)
    let backend = tmp.path().join("backend");
    std::fs::create_dir(&backend).unwrap();
    std::fs::write(
        backend.join("main.py"),
        r#"
def hello():
    print("Hello from Python")
"#,
    )
    .unwrap();

    // Create also a TypeScript file
    std::fs::write(backend.join("index.ts"), "export class Foo {}").unwrap();

    // Build index - should not crash, just skip Python files
    let index = CrateIndex::build(tmp.path()).unwrap();

    // Verify TypeScript file was indexed but Python was not
    assert_eq!(index.ts_files.len(), 1, "Should have 1 TypeScript file");
    assert!(index.rust_files.is_empty(), "Should have no Rust files");

    // Python file should not appear in any index
    let py_file = backend.join("main.py");
    assert!(
        !index.ts_files.contains_key(&py_file),
        "Python file should not be in ts_files"
    );
    assert!(
        !index.rust_files.contains_key(&py_file),
        "Python file should not be in rust_files"
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn test_typescript_class_implements() {
    let tmp = tempfile::tempdir().unwrap();

    // Create TypeScript files with interface implementations
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();

    std::fs::write(
        src.join("service.ts"),
        r#"
interface ILogger {
    log(msg: string): void;
}

class ConsoleLogger implements ILogger {
    log(msg: string) {
        console.log(msg);
    }
}

class FileLogger implements ILogger {
    log(msg: string) {
        // write to file
    }
}
"#,
    )
    .unwrap();

    // Build index
    let index = CrateIndex::build(tmp.path()).unwrap();

    // Verify class implementations were tracked
    assert!(
        index
            .trait_impls
            .contains_key(&("ILogger".into(), "ConsoleLogger".into())),
        "Should track ConsoleLogger implements ILogger"
    );
    assert!(
        index
            .trait_impls
            .contains_key(&("ILogger".into(), "FileLogger".into())),
        "Should track FileLogger implements ILogger"
    );

    // Check must_implement constraint
    let result = checker::must_implement::check(
        "logger_interface",
        "TestSystem",
        "ConsoleLogger",
        "ILogger",
        &index,
    );
    assert!(result.holds, "ConsoleLogger should implement ILogger");

    // Check for non-existent implementation
    let result = checker::must_implement::check(
        "logger_interface",
        "TestSystem",
        "ConsoleLogger",
        "IDatabase",
        &index,
    );
    assert!(
        !result.holds,
        "ConsoleLogger should not implement IDatabase"
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn test_javascript_files() {
    let tmp = tempfile::tempdir().unwrap();

    // Create JavaScript files (should be treated like TypeScript)
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();

    std::fs::write(
        src.join("app.js"),
        r#"
import { Router } from './router';
import { Database } from './db';

export class App {
    constructor() {
        this.router = new Router();
        this.db = new Database();
    }
}
"#,
    )
    .unwrap();

    std::fs::write(src.join("router.js"), "export class Router {}").unwrap();

    // Build index
    let index = CrateIndex::build(tmp.path()).unwrap();

    // Verify JavaScript files were indexed
    assert_eq!(index.ts_files.len(), 2, "Should have 2 JavaScript files");

    // Verify entity references
    assert!(
        index.entity_refs.contains_key("Router"),
        "Should have Router entity"
    );
    assert!(
        index.entity_refs.contains_key("Database"),
        "Should have Database entity"
    );
}
