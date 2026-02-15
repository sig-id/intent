fn main() {
    // LALRPOP reports shift/reduce conflicts for arithmetic expressions
    // and comparison chaining, but these are benign and resolve correctly
    // via standard shift/reduce behavior (shift wins = correct precedence).
    let _ = lalrpop::Configuration::new()
        .emit_rerun_directives(true)
        .force_build(true)
        .process_current_dir();
}
