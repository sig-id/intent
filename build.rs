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
}
