use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use rayon::prelude::*;
use walkdir::WalkDir;

use intent::{behavioral, diagnostic::Severity, linter, parser, plan, rationale, structural};

#[derive(Parser)]
#[command(name = "intent", about = "Static analysis for Intent design constraints")]
struct Cli {
    /// Output format
    #[arg(short, long, default_value = "text", global = true)]
    format: OutputFormat,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, ValueEnum)]
enum VerifyMode {
    /// Fast verification with Apalache (bounded model checking)
    Fast,
    /// Exhaustive verification with TLC (complete state space)
    Exhaustive,
    /// Both Apalache and TLC
    Both,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all phases: structural, compile, verify, rationale
    Check {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Path to the codebase source root
        #[arg(short, long)]
        codebase: PathBuf,
        /// TLA+ spec directory (defaults to formal/tla/ relative to project root)
        #[arg(short, long)]
        specs: Option<PathBuf>,
    },
    /// Run structural constraint verification only
    Structural {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Path to the codebase source root
        #[arg(short, long)]
        codebase: PathBuf,
    },
    /// Lint intent files for syntax errors and style issues
    Lint {
        /// Files or directories to lint
        paths: Vec<PathBuf>,
        /// Enable pedantic checks
        #[arg(short, long)]
        pedantic: bool,
        /// Allow unused components
        #[arg(short = 'u', long)]
        allow_unused: bool,
        /// Check naming conventions
        #[arg(short = 'n', long, default_value = "true")]
        check_naming: bool,
        /// Show hints
        #[arg(short = 'H', long)]
        hints: bool,
    },
    /// Generate TLA+ obligation modules from applies blocks
    Compile {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Output directory for generated TLA+ files
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Verify TLA+ obligation modules with model checkers
    Verify {
        /// Directory containing generated TLA+ files
        #[arg(short, long)]
        obligations: PathBuf,
        /// Filter: a substring pattern (e.g. "Auth") or single .tla file path
        filter: Option<String>,
        /// Verification mode: fast (Apalache), exhaustive (TLC), or both
        #[arg(short, long, default_value = "fast")]
        mode: VerifyMode,
        /// Maximum length for bounded checking (Apalache only)
        #[arg(short, long, default_value = "10")]
        length: usize,
        /// Check temporal properties (requires TLC)
        #[arg(short, long)]
        temporal: bool,
    },
    /// Extract rationale JSON from system metadata
    Rationale {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Output path for rationale JSON
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Run plan-mode validation (no codebase required)
    Plan {
        /// Directory containing .intent files
        intent_dir: PathBuf,
    },
    /// Extract non-functional constraints as benchmark configuration
    ExtractBenchmarks {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Output path for benchmark JSON
        #[arg(short, long)]
        output: PathBuf,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e:#}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let json_mode = matches!(cli.format, OutputFormat::Json);
    let quiet = cli.quiet;

    match cli.command {
        Commands::Check {
            intent_dir,
            codebase,
            specs: _,
        } => {
            let project_root = find_project_root(&codebase)?;
            let systems = load_systems(&intent_dir)?;

            // Phase 1: Structural
            if !quiet {
                println!("=== Phase 1: Structural verification ===");
            }
            let structural_results = structural::check(&systems, &codebase)?;
            if !quiet && !json_mode {
                print_structural_results(&structural_results);
            }
            let structural_ok = structural_results.iter().all(|r| r.holds);

            // Phase 2: Behavioral compilation
            if !quiet {
                println!("\n=== Phase 2: Behavioral compilation ===");
            }
            let obligations_dir = project_root.join("formal/tla/obligations");
            let opts = behavioral::CompileOptions { apalache: true, generate_cfg: true };
            let generated = behavioral::compile_with_options(
                &systems, &obligations_dir, &project_root, &opts)?;
            if !quiet && !json_mode {
                for path in &generated {
                    println!("  generated: {}", path.display().dimmed());
                }
            }

            // Phase 3: Obligation verification
            if !quiet {
                println!("\n=== Phase 3: Obligation verification ===");
            }
            let obligation_results = behavioral::verify(&obligations_dir, &project_root)?;
            if !quiet && !json_mode {
                print_obligation_results(&obligation_results);
            }
            let behavioral_ok = obligation_results
                .iter()
                .all(|r| r.status != behavioral::ObligationStatus::Fail);

            // Phase 4: Rationale extraction
            if !quiet {
                println!("\n=== Phase 4: Rationale extraction ===");
            }
            let report =
                rationale::build_report(&systems, &structural_results, &obligation_results);
            let rationale_path = intent_dir.join("rationale.json");
            rationale::write_json(&report, &rationale_path)?;
            if !quiet && !json_mode {
                println!("  written: {}", rationale_path.display().dimmed());
            }

            // Phase 5: Coverage report
            if !quiet {
                println!("\n=== Phase 5: Spec coverage ===");
            }
            let coverage = intent::coverage::analyze(&systems);
            if !quiet && !json_mode {
                for report in &coverage {
                    for comp in &report.components {
                        let structural = if comp.has_structural_constraints { "S" } else { "-" };
                        let behavioral = if comp.has_behavioral_specs { "B" } else { "-" };
                        println!("  [{}{}] {}", structural, behavioral, comp.name);
                    }
                    println!("  Coverage: {:.0}% ({}/{} components)",
                        report.summary.coverage_percentage,
                        report.summary.total_components - report.summary.unconstrained,
                        report.summary.total_components,
                    );
                }
            }


            if json_mode {
                print_json_output(&structural_results, &obligation_results)?;
            }

            // Summary
            if !quiet && !json_mode {
                println!();
                if structural_ok && behavioral_ok {
                    println!("{}", "All checks passed.".green());
                } else {
                    if !structural_ok {
                        println!("{}", "FAIL: structural constraints violated".red());
                    }
                    if !behavioral_ok {
                        println!("{}", "FAIL: behavioral obligations not satisfied".red());
                    }
                }
            }

            if !structural_ok || !behavioral_ok {
                process::exit(1);
            }
        }

        Commands::Structural {
            intent_dir,
            codebase,
        } => {
            let systems = load_systems(&intent_dir)?;
            let results = structural::check(&systems, &codebase)?;

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing structural results")?;
                println!("{json}");
            } else if !quiet {
                print_structural_results(&results);
            }

            if !results.iter().all(|r| r.holds) {
                process::exit(1);
            }
        }

        Commands::Lint {
            paths,
            pedantic,
            allow_unused,
            check_naming,
            hints,
        } => {
            let config = linter::LinterConfig {
                pedantic,
                allow_unused,
                check_naming,
                ..linter::LinterConfig::default()
            };
            let linter_instance = linter::Linter::new(config);

            // Collect all files to lint
            let mut files: Vec<(PathBuf, String)> = Vec::new();
            for path in &paths {
                if path.is_dir() {
                    for entry in WalkDir::new(path)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path()
                                .extension()
                                .is_some_and(|ext| ext == "intent")
                        })
                    {
                        let file_path = entry.path().to_path_buf();
                        if let Ok(source) = std::fs::read_to_string(&file_path) {
                            files.push((file_path, source));
                        }
                    }
                } else if path.extension().is_some_and(|ext| ext == "intent") {
                    if let Ok(source) = std::fs::read_to_string(path) {
                        files.push((path.clone(), source));
                    }
                }
            }

            if files.is_empty() {
                anyhow::bail!("no .intent files found to lint");
            }

            let results = linter_instance.lint_files(&files);

            // Print results
            let mut total_errors = 0;
            let mut total_warnings = 0;

            for result in &results {
                if result.diagnostics.is_empty() {
                    if !quiet {
                        println!("{} {}", "[OK]".green(), result.file.display());
                    }
                } else {
                    println!("{}", result.file.display().bold());

                    for diag in &result.diagnostics.items {
                        let (severity_str, color) = match diag.severity {
                            Severity::Error => {
                                total_errors += 1;
                                ("ERROR", "red")
                            }
                            Severity::Warning => {
                                total_warnings += 1;
                                ("WARN", "yellow")
                            }
                            Severity::Info => ("INFO", "blue"),
                            Severity::Hint => {
                                if !hints {
                                    continue;
                                }
                                ("HINT", "cyan")
                            }
                        };

                        let colored = |s: &str| match color {
                            "red" => s.red().to_string(),
                            "yellow" => s.yellow().to_string(),
                            "blue" => s.blue().to_string(),
                            "cyan" => s.cyan().to_string(),
                            _ => s.to_string(),
                        };

                        println!(
                            "  {} {}: {}",
                            colored(&format!("[{}]", severity_str)),
                            colored(&format!("{}", diag.code)),
                            diag.message
                        );

                        for suggestion in &diag.suggestions {
                            println!("    {} {}", "help:".green(), suggestion);
                        }
                    }
                }
            }

            // Summary
            if !quiet {
                println!();
                if total_errors == 0 && total_warnings == 0 {
                    println!(
                        "{}: {} files checked",
                        "Finished".green(),
                        results.len()
                    );
                } else {
                    println!(
                        "{}: {} errors, {} warnings in {} files",
                        "Finished".yellow(),
                        total_errors,
                        total_warnings,
                        results.len()
                    );
                }
            }

            if total_errors > 0 {
                process::exit(1);
            }
        }

        Commands::Compile { intent_dir, output } => {
            let project_root = find_project_root(&output)?;
            let systems = load_systems(&intent_dir)?;
            // Always compile with Apalache type annotations and .cfg sidecar files.
            // Apalache-typed output is valid for both Apalache and TLC; the .cfg
            // files enable TLC exhaustive verification without extra steps.
            let opts = behavioral::CompileOptions { apalache: true, generate_cfg: true };
            let generated =
                behavioral::compile_with_options(&systems, &output, &project_root, &opts)?;
            if !quiet {
                for path in &generated {
                    println!("generated: {}", path.display());
                }
            }
        }

        Commands::Verify {
            obligations,
            filter,
            mode,
            length,
            temporal,
        } => {
            let _project_root = find_project_root(&obligations)?;

            // Convert mode
            let verification_mode = match mode {
                VerifyMode::Fast => behavioral::VerificationMode::Fast,
                VerifyMode::Exhaustive => behavioral::VerificationMode::Exhaustive,
                VerifyMode::Both => behavioral::VerificationMode::Both,
            };

            // Create config
            let config = behavioral::VerificationConfig {
                mode: verification_mode,
                max_length: length,
                check_temporal: temporal || matches!(mode, VerifyMode::Exhaustive | VerifyMode::Both),
                ..Default::default()
            };

            // Run verification, optionally filtered.
            // obligations can be a directory OR a single .tla file.
            let is_single_file = obligations.extension().and_then(|e| e.to_str()) == Some("tla")
                && obligations.is_file();

            let results = if is_single_file {
                // --obligations points directly at a .tla file
                let module_name = obligations
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let result = behavioral::verify_module(&obligations, &module_name, &config)?;
                vec![result]
            } else {
                match filter {
                    Some(ref f) if f.ends_with(".tla") && PathBuf::from(f).exists() => {
                        // Filter is a single .tla file path
                        let path = PathBuf::from(f);
                        let module_name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let result = behavioral::verify_module(&path, &module_name, &config)?;
                        vec![result]
                    }
                    Some(ref pattern) => {
                        // Substring filter on module names
                        let all = behavioral::verify_directory(&obligations, &config)?;
                        all.into_iter()
                            .filter(|r| r.module.contains(pattern.as_str()))
                            .collect()
                    }
                    None => {
                        // Verify all in directory
                        behavioral::verify_directory(&obligations, &config)?
                    }
                }
            };

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing verification results")?;
                println!("{json}");
            } else if !quiet {
                print_verification_results(&results);
            }

            if results
                .iter()
                .any(|r| r.status != behavioral::VerificationStatus::Pass)
            {
                process::exit(1);
            }
        }

        Commands::Rationale { intent_dir, output } => {
            let systems = load_systems(&intent_dir)?;
            let report = rationale::build_report(&systems, &[], &[]);
            rationale::write_json(&report, &output)?;
            if !quiet {
                println!("written: {}", output.display());
            }
        }

        Commands::Plan { intent_dir } => {
            let systems = load_systems(&intent_dir)?;
            let results = plan::validate(&systems)?;

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing plan results")?;
                println!("{json}");
            } else if !quiet {
                for result in &results {
                    println!("=== {} ===", result.system);
                    for check in &result.checks {
                        if check.passed {
                            println!("  {} {}", "[PASS]".green(), check.name);
                        } else {
                            println!("  {} {}", "[FAIL]".red(), check.name);
                        }
                        if !check.detail.is_empty() {
                            println!("    {}", check.detail.dimmed());
                        }
                    }
                }
            }

            let all_passed = results.iter().all(|r| r.checks.iter().all(|c| c.passed));
            if !all_passed {
                process::exit(1);
            }
        }

        Commands::ExtractBenchmarks { intent_dir, output } => {
            let systems = load_systems(&intent_dir)?;
            let configs = intent::benchmark::extract(&systems);
            let json = serde_json::to_string_pretty(&configs)
                .context("serializing benchmark config")?;
            std::fs::write(&output, &json)?;
            if !quiet {
                let total: usize = configs.iter().map(|c| c.benchmarks.len()).sum();
                println!("Extracted {} benchmarks to {}", total, output.display());
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_systems(
    intent_dir: &PathBuf,
) -> Result<Vec<intent::parser::ast::SystemDecl>> {
    // Collect all .intent file paths first
    let intent_files: Vec<PathBuf> = WalkDir::new(intent_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "intent")
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    // Parse files in parallel
    let file_systems: Vec<Vec<intent::parser::ast::SystemDecl>> = intent_files
        .par_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let top_levels = parser::parse(&source).ok()?;
            let systems: Vec<_> = top_levels
                .into_iter()
                .filter_map(|top| {
                    if let intent::parser::ast::TopLevel::System(sys) = top {
                        Some(sys)
                    } else {
                        None
                    }
                })
                .collect();
            if systems.is_empty() {
                None
            } else {
                Some(systems)
            }
        })
        .collect();

    let systems: Vec<_> = file_systems.into_iter().flatten().collect();

    if systems.is_empty() {
        anyhow::bail!("no system declarations found in {}", intent_dir.display());
    }

    Ok(systems)
}

fn find_project_root(from: &PathBuf) -> Result<PathBuf> {
    let start = if from.is_dir() {
        from.clone()
    } else {
        from.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let mut dir = std::fs::canonicalize(&start)
        .unwrap_or_else(|_| start.clone());

    loop {
        if dir.join("Cargo.toml").exists() && dir.join("formal").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }

    // Fallback: current directory
    Ok(std::env::current_dir().context("getting current directory")?)
}

fn print_structural_results(results: &[structural::ConstraintResult]) {
    use structural::{CheckStatus, VerificationLevel};

    // Group results by verification level
    let mut structural_results: Vec<&structural::ConstraintResult> = Vec::new();
    let mut benchmark_results: Vec<&structural::ConstraintResult> = Vec::new();
    let mut unchecked_results: Vec<&structural::ConstraintResult> = Vec::new();
    let mut formal_results: Vec<&structural::ConstraintResult> = Vec::new();

    for result in results {
        match result.verification_level {
            VerificationLevel::Formal => formal_results.push(result),
            VerificationLevel::Structural => structural_results.push(result),
            VerificationLevel::Benchmark => benchmark_results.push(result),
            VerificationLevel::Unchecked => unchecked_results.push(result),
        }
    }

    // Print formal results first, then structural, then skipped groups
    for result in formal_results.iter().chain(structural_results.iter()) {
        match &result.status {
            CheckStatus::Passed => {
                println!("  {} {}", "[PASS]".green(), result.name);
            }
            CheckStatus::Failed => {
                println!("  {} {}", "[FAIL]".red(), result.name);
            }
            CheckStatus::Skipped { reason } => {
                println!("  {} {} – {}", "[SKIP]".yellow(), result.name, reason);
            }
        }

        for v in &result.violations {
            println!(
                "    {}:{} – references {}",
                v.file.display().dimmed(),
                v.line,
                v.entity
            );
            println!("      {}", v.content.dimmed());
        }
    }

    // Print benchmark and unchecked results grouped
    if !benchmark_results.is_empty() || !unchecked_results.is_empty() {
        println!();
        println!(
            "  {}",
            "Skipped (not structurally verifiable):".dimmed()
        );
        for result in benchmark_results.iter().chain(unchecked_results.iter()) {
            let level_tag = match result.verification_level {
                VerificationLevel::Benchmark => "benchmark",
                VerificationLevel::Unchecked => "unchecked",
                _ => unreachable!(),
            };
            if let CheckStatus::Skipped { reason } = &result.status {
                println!(
                    "    {} {} ({}) – {}",
                    "[SKIP]".yellow(),
                    result.name,
                    level_tag.dimmed(),
                    reason
                );
            } else {
                println!(
                    "    {} {} ({})",
                    "[SKIP]".yellow(),
                    result.name,
                    level_tag.dimmed()
                );
            }
        }
    }
}

fn print_obligation_results(results: &[behavioral::ObligationResult]) {
    for result in results {
        let status_str = result.status.to_string().to_uppercase();
        match result.status {
            behavioral::ObligationStatus::Pass => {
                println!(
                    "  {} {} -> {}",
                    format!("[{status_str}]").green(),
                    result.pattern,
                    result.target
                );
            }
            behavioral::ObligationStatus::Fail => {
                println!(
                    "  {} {} -> {}",
                    format!("[{status_str}]").red(),
                    result.pattern,
                    result.target
                );
            }
            behavioral::ObligationStatus::Skipped => {
                println!(
                    "  {} {} -> {}",
                    format!("[{status_str}]").yellow(),
                    result.pattern,
                    result.target
                );
            }
        }
        if !result.detail.is_empty() {
            println!("    {}", result.detail.dimmed());
        }
    }
}

fn print_verification_results(results: &[behavioral::ModuleVerificationResult]) {
    for result in results {
        let (status_str, status_color) = match result.status {
            behavioral::VerificationStatus::Pass => ("[PASS]", "green"),
            behavioral::VerificationStatus::Fail => ("[FAIL]", "red"),
            behavioral::VerificationStatus::Error => ("[ERROR]", "red"),
            behavioral::VerificationStatus::Timeout => ("[TIMEOUT]", "yellow"),
        };

        let colored_status = match status_color {
            "green" => status_str.green().to_string(),
            "red" => status_str.red().to_string(),
            "yellow" => status_str.yellow().to_string(),
            _ => status_str.to_string(),
        };

        println!("  {} {} ({:.2}s)", colored_status, result.module, result.duration);

        // Type check result
        if let Some(ref type_check) = result.type_check {
            if type_check.passed {
                println!("    {} Type checking", "[✓]".green());
            } else {
                println!("    {} Type checking: {}", "[✗]".red(), type_check.detail);
            }
        }

        // Invariant results
        for inv in &result.invariants {
            if inv.passed {
                let states_info = if let Some(states) = inv.states_checked {
                    format!(" ({} states)", states)
                } else {
                    String::new()
                };
                println!("    {} {}{}", "[✓]".green(), inv.name, states_info);
            } else {
                println!("    {} {}", "[✗]".red(), inv.name);
                if let Some(ref ce) = inv.counterexample {
                    println!("      {}", ce.dimmed());
                }
            }
        }

        // Temporal property results
        for prop in &result.temporal_properties {
            if prop.passed {
                println!("    {} {} ({})", "[✓]".green(), prop.name, prop.checker);
            } else {
                println!("    {} {}: {}", "[✗]".red(), prop.name, prop.detail);
            }
        }
    }

    // Summary
    let total = results.len();
    let passed = results.iter().filter(|r| r.status == behavioral::VerificationStatus::Pass).count();
    let failed = results.iter().filter(|r| r.status == behavioral::VerificationStatus::Fail).count();

    println!();
    if failed == 0 {
        println!("{}: {}/{} modules verified", "Success".green(), passed, total);
    } else {
        println!("{}: {}/{} passed, {} failed", "Results".yellow(), passed, total, failed);
    }
}

fn print_json_output(
    structural_results: &[structural::ConstraintResult],
    obligation_results: &[behavioral::ObligationResult],
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct FullOutput<'a> {
        structural: &'a [structural::ConstraintResult],
        behavioral: &'a [behavioral::ObligationResult],
    }

    let output = FullOutput {
        structural: structural_results,
        behavioral: obligation_results,
    };
    let json = serde_json::to_string_pretty(&output)
        .context("serializing output")?;
    println!("{json}");
    Ok(())
}
