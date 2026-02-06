//! Hatchery CLI — swarm orchestration for AI coding agents.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use hatchery::types::{HatcheryConfig, Mode};

#[derive(Parser)]
#[command(name = "hatchery", version, about = "Swarm orchestration for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn a swarm from a PRD file.
    Spawn {
        /// Path to the PRD markdown file.
        prd: PathBuf,

        /// Number of worker sessions.
        #[arg(short, long, default_value = "1")]
        workers: usize,

        /// Operation mode.
        #[arg(short, long, value_enum, default_value = "queen")]
        mode: Mode,

        /// Working directory for workers.
        #[arg(long)]
        dir: Option<PathBuf>,

        /// Verification command (e.g. "cargo check --package mylib").
        #[arg(long)]
        verify: Option<String>,

        /// Maximum iterations per worker.
        #[arg(long, default_value = "100")]
        max_iterations: usize,

        /// Consecutive stall iterations before pausing.
        #[arg(long, default_value = "3")]
        stall_threshold: usize,

        /// Progress file path (auto-generated if not specified).
        #[arg(long)]
        progress: Option<PathBuf>,

        /// Show verbose output from worker sessions.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show status of an ongoing or completed run.
    Status {
        /// Path to the PRD markdown file.
        prd: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Spawn {
            prd,
            workers,
            mode,
            dir,
            verify,
            max_iterations,
            stall_threshold,
            progress,
            verbose,
        } => {
            let working_dir = dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            let config = HatcheryConfig {
                prd_path: prd,
                workers,
                mode,
                working_dir,
                verify_cmd: verify,
                max_iterations,
                stall_threshold,
                progress_path: progress,
                verbose,
            };

            match config.mode {
                Mode::Queen => {
                    let result = hatchery::queen::run(&config)?;
                    println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s",
                        result.completed_tasks, result.total_tasks, result.duration_secs);
                }
                Mode::SwarmHost => {
                    let result = hatchery::swarm_host::run(&config)?;
                    println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s",
                        result.completed_tasks, result.total_tasks, result.duration_secs);
                }
                Mode::BroodLord => {
                    let result = hatchery::brood_lord::run(&config)?;
                    println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s",
                        result.completed_tasks, result.total_tasks, result.duration_secs);
                }
            }
        }

        Commands::Status { prd } => {
            let tasks = hatchery::prd::parse_prd(&prd)?;
            let (done, total) = hatchery::prd::progress(&tasks);

            println!("PRD: {}", prd.display());
            println!("{}", hatchery::progress::progress_bar(done, total, 40));
            println!();

            for task in &tasks {
                let marker = if task.done { "✓" } else { "○" };
                println!("  {} Task {}: {}", marker, task.id, task.description);
            }
        }
    }

    Ok(())
}
