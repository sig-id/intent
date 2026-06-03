use std::env;
use std::path::PathBuf;

fn main() {
    // Get the output directory from cargo
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Get the manifest directory (where Cargo.toml is)
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src_dir = manifest_dir.join("src");

    // Copy the grammar file to OUT_DIR so lalrpop can write to it
    let grammar_src = src_dir.join("parser/intent.lalrpop");
    let grammar_dst = out_dir.join("intent.lalrpop");
    std::fs::copy(&grammar_src, &grammar_dst).expect("failed to copy grammar");

    // LALRPOP reports shift/reduce conflicts for arithmetic expressions
    // and comparison chaining, but these are benign and resolve correctly
    // via standard shift/reduce behavior (shift wins = correct precedence).
    if let Err(e) = lalrpop::Configuration::new()
        .force_build(true)
        .emit_rerun_directives(true)
        .process_file(&grammar_dst)
    {
        panic!("lalrpop failed: {:?}", e);
    }

    // Rerun if grammar changes
    println!("cargo:rerun-if-changed={}", grammar_src.display());

    // Extract stdlib pattern names from stdlib/patterns.intent
    let stdlib_path = manifest_dir.join("stdlib/patterns.intent");
    println!("cargo:rerun-if-changed={}", stdlib_path.display());

    let mut pattern_names = Vec::new();
    if let Ok(contents) = std::fs::read_to_string(&stdlib_path) {
        for line in contents.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("pattern ") {
                // Extract name: take until whitespace, '<', or '{'
                let name: String = rest
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '<' && *c != '{')
                    .collect();
                if !name.is_empty() {
                    pattern_names.push(name);
                }
            }
        }
    }

    let generated = format!(
        "pub const STDLIB_PATTERN_NAMES: &[&str] = &[{}];\n",
        pattern_names
            .iter()
            .map(|n| format!("\"{}\"", n))
            .collect::<Vec<_>>()
            .join(", ")
    );
    std::fs::write(out_dir.join("stdlib_patterns.rs"), generated)
        .expect("failed to write stdlib_patterns.rs");
}
