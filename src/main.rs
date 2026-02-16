use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use walkdir::WalkDir;

use intent::{behavioral, parser, plan, rationale, structural};

#[derive(Parser)]
#[command(name = "intent", about = "Static analysis for Intent design constraints")]
struct Cli {
    /// Output format
    #[arg(long, default_value = "text", global = true)]
    format: OutputFormat,

    /// Suppress non-error output
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all phases: structural, compile, verify, rationale
    Check {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Path to the codebase source root
        #[arg(long)]
        codebase: PathBuf,
        /// TLA+ spec directory (defaults to formal/tla/ relative to project root)
        #[arg(long)]
        specs: Option<PathBuf>,
    },
    /// Run structural constraint verification only
    Structural {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Path to the codebase source root
        #[arg(long)]
        codebase: PathBuf,
    },
    /// Generate TLA+ obligation modules from apply...refines blocks
    Compile {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Output directory for generated TLA+ files
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify TLA+ obligation modules with Apalache
    Verify {
        /// Directory containing generated obligation TLA+ files
        #[arg(long)]
        obligations: PathBuf,
    },
    /// Extract rationale JSON from concern metadata
    Rationale {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Output path for rationale JSON
        #[arg(long)]
        output: PathBuf,
    },
    /// Run plan-mode validation (no codebase required)
    Plan {
        /// Directory containing .intent files
        intent_dir: PathBuf,
    },
    /// Generate skeleton code from constraints (planned feature)
    Skeleton {
        /// Directory containing .intent files
        intent_dir: PathBuf,
        /// Path to the codebase source root
        #[arg(long)]
        codebase: PathBuf,
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
            let concerns = load_concerns(&intent_dir)?;

            // Phase 1: Structural
            if !quiet {
                println!("=== Phase 1: Structural verification ===");
            }
            let structural_results = structural::check(&concerns, &codebase)?;
            if !quiet && !json_mode {
                print_structural_results(&structural_results);
            }
            let structural_ok = structural_results.iter().all(|r| r.passed);

            // Phase 2: Behavioral compilation
            if !quiet {
                println!("\n=== Phase 2: Behavioral compilation ===");
            }
            let obligations_dir = project_root.join("formal/tla/obligations");
            let generated = behavioral::compile(&concerns, &obligations_dir, &project_root)?;
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
                rationale::build_report(&concerns, &structural_results, &obligation_results);
            let rationale_path = intent_dir.join("rationale.json");
            rationale::write_json(&report, &rationale_path)?;
            if !quiet && !json_mode {
                println!("  written: {}", rationale_path.display().dimmed());
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
            let concerns = load_concerns(&intent_dir)?;
            let results = structural::check(&concerns, &codebase)?;

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing structural results")?;
                println!("{json}");
            } else if !quiet {
                print_structural_results(&results);
            }

            if !results.iter().all(|r| r.passed) {
                process::exit(1);
            }
        }

        Commands::Compile { intent_dir, output } => {
            let project_root = find_project_root(&output)?;
            let concerns = load_concerns(&intent_dir)?;
            let generated = behavioral::compile(&concerns, &output, &project_root)?;
            if !quiet {
                for path in &generated {
                    println!("generated: {}", path.display());
                }
            }
        }

        Commands::Verify { obligations } => {
            let project_root = find_project_root(&obligations)?;
            let results = behavioral::verify(&obligations, &project_root)?;

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing obligation results")?;
                println!("{json}");
            } else if !quiet {
                print_obligation_results(&results);
            }

            if results
                .iter()
                .any(|r| r.status == behavioral::ObligationStatus::Fail)
            {
                process::exit(1);
            }
        }

        Commands::Rationale { intent_dir, output } => {
            let concerns = load_concerns(&intent_dir)?;
            let report = rationale::build_report(&concerns, &[], &[]);
            rationale::write_json(&report, &output)?;
            if !quiet {
                println!("written: {}", output.display());
            }
        }

        Commands::Plan { intent_dir } => {
            let concerns = load_concerns(&intent_dir)?;
            let results = plan::validate(&concerns)?;

            if json_mode {
                let json = serde_json::to_string_pretty(&results)
                    .context("serializing plan results")?;
                println!("{json}");
            } else if !quiet {
                for result in &results {
                    println!("=== {} ===", result.concern);
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

        Commands::Skeleton { intent_dir, codebase: _ } => {
            let _concerns = load_concerns(&intent_dir)?;
            if !quiet {
                println!("{}", "Skeleton mode is not yet implemented.".yellow());
                println!("This command will generate code stubs for planned constraints.");
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_concerns(
    intent_dir: &PathBuf,
) -> Result<Vec<intent::parser::ast::Concern>> {
    let mut all_concerns = Vec::new();

    for entry in WalkDir::new(intent_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "intent")
        })
    {
        let source = std::fs::read_to_string(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        let concerns = parser::parse_concerns(&source)
            .with_context(|| format!("parsing {}", entry.path().display()))?;
        all_concerns.extend(concerns);
    }

    if all_concerns.is_empty() {
        anyhow::bail!("no .intent files found in {}", intent_dir.display());
    }

    Ok(all_concerns)
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
    for result in results {
        if result.passed {
            println!("  {} {}", "[PASS]".green(), result.name);
        } else {
            println!("  {} {}", "[FAIL]".red(), result.name);
        }

        for v in &result.violations {
            println!(
                "    {}:{} — references {}",
                v.file.display().dimmed(),
                v.line,
                v.entity
            );
            println!("      {}", v.content.dimmed());
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
