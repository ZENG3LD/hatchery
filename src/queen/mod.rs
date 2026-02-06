//! Queen mode — simple Ralph-style iteration.
//!
//! Each worker independently iterates through PRD checkboxes:
//! 1. Read PRD → find next uncompleted task
//! 2. Invoke Claude via PipeProcess with task + progress context
//! 3. Wait for completion
//! 4. Run verification command
//! 5. On success: mark checkbox, git commit, record progress
//! 6. On failure: record error, next iteration
//! 7. Repeat until all done or max iterations

use crate::prd;
use crate::progress::{self, ProgressTracker};
use crate::types::{HatcheryConfig, IterationResult, SwarmResult};
use anyhow::{Context, Result};
use std::process::Command;
use std::time::{Duration, Instant};
use zengeld_hub_core::{CliEvent, CliTool, PipeProcess};

/// Embedded prompts — compiled into the binary, zero runtime cost.
const ITERATION_PROMPT: &str = include_str!("prompts/iteration.md");
const COMPACT_PROMPT: &str = include_str!("prompts/compact.md");

/// Run Queen mode: spawn workers and iterate through PRD.
pub fn run(config: &HatcheryConfig) -> Result<SwarmResult> {
    let start = Instant::now();

    // Parse PRD
    let tasks = prd::parse_prd(&config.prd_path)?;
    let (done, total) = prd::progress(&tasks);

    println!(
        "[HATCHERY] Queen mode: {} tasks ({} done, {} remaining)",
        total,
        done,
        total - done
    );
    println!(
        "[HATCHERY] Workers: {}, Max iterations: {}",
        config.workers, config.max_iterations
    );
    println!("[HATCHERY] PRD: {}", config.prd_path.display());
    println!(
        "[HATCHERY] {}",
        progress::progress_bar(done, total, 30)
    );
    println!();

    if done == total {
        println!("[HATCHERY] All tasks already complete!");
        return Ok(SwarmResult {
            total_tasks: total,
            completed_tasks: done,
            total_iterations: 0,
            duration_secs: 0,
            workers_used: 0,
        });
    }

    if config.workers == 1 {
        run_single_worker(config, start)
    } else {
        run_multi_worker(config, start)
    }
}

/// Return the compaction prompt template (for use by ProgressTracker).
pub fn compact_prompt() -> &'static str {
    COMPACT_PROMPT
}

/// Single-worker Queen: classic Ralph loop.
fn run_single_worker(config: &HatcheryConfig, start: Instant) -> Result<SwarmResult> {
    let mut tracker = ProgressTracker::new(&config.prd_path, config.progress_path.clone());
    let mut total_iterations = 0;

    for iteration in 1..=config.max_iterations {
        // Re-read PRD each iteration (it changes as checkboxes are marked)
        let tasks = prd::parse_prd(&config.prd_path)?;
        let (done, total) = prd::progress(&tasks);

        // Check completion
        let next_task = match prd::first_uncompleted(&tasks) {
            Some(task) => task.clone(),
            None => {
                println!("\n[HATCHERY] All tasks complete!");
                return Ok(SwarmResult {
                    total_tasks: total,
                    completed_tasks: total,
                    total_iterations: iteration - 1,
                    duration_secs: start.elapsed().as_secs(),
                    workers_used: 1,
                });
            }
        };

        // Stall detection
        if tracker.is_stalled(config.stall_threshold) {
            println!(
                "[HATCHERY] Stalled ({} iterations without progress). Pausing 10s...",
                tracker.stall_count()
            );
            std::thread::sleep(Duration::from_secs(10));
        }

        // Progress bar
        print!(
            "\r[HATCHERY] Iteration {}/{} | {} | Task {}: {}",
            iteration,
            config.max_iterations,
            progress::progress_bar(done, total, 20),
            next_task.id,
            truncate(&next_task.description, 50),
        );
        println!();

        // Build prompt for Claude
        let progress_context = tracker.read().unwrap_or_default();
        let prd_content =
            std::fs::read_to_string(&config.prd_path).unwrap_or_default();
        let prompt = build_iteration_prompt(
            &prd_content,
            &next_task.description,
            &progress_context,
            iteration,
            tracker.path(),
        );

        // Invoke Claude via PipeProcess
        let result = invoke_claude(config, &prompt)?;
        total_iterations = iteration;

        match result {
            IterationResult::Progress {
                task_id: _,
                ref description,
            } => {
                // Verify if needed
                if let Some(ref verify) = config.verify_cmd {
                    if !run_verify(verify, &config.working_dir)? {
                        println!("[HATCHERY]   ✗ Verification failed: {}", verify);
                        tracker.record_failure(&format!(
                            "Task '{}' — verification failed",
                            description
                        ))?;
                        continue;
                    }
                    println!("[HATCHERY]   ✓ Verification passed");
                }

                // Mark checkbox in PRD
                let fresh_tasks = prd::parse_prd(&config.prd_path)?;
                if let Some(task) = fresh_tasks.iter().find(|t| t.id == next_task.id && !t.done) {
                    prd::mark_complete(&config.prd_path, task)?;
                    println!("[HATCHERY]   ✓ Marked task {} complete", next_task.id);
                }

                // Git commit
                git_commit(config, &next_task.description)?;
                tracker.record_success(&next_task.description)?;
            }
            IterationResult::NoProgress { ref reason } => {
                println!("[HATCHERY]   ○ No progress: {}", reason);
                tracker.record_no_progress(reason)?;
            }
            IterationResult::Error { ref message } => {
                println!("[HATCHERY]   ✗ Error: {}", message);
                tracker.record_failure(message)?;
            }
            IterationResult::AllDone => {
                break;
            }
        }
    }

    let tasks = prd::parse_prd(&config.prd_path)?;
    let (done, total) = prd::progress(&tasks);

    println!(
        "\n[HATCHERY] Finished: {} | {} iterations in {}s",
        progress::progress_bar(done, total, 30),
        total_iterations,
        start.elapsed().as_secs()
    );

    Ok(SwarmResult {
        total_tasks: total,
        completed_tasks: done,
        total_iterations,
        duration_secs: start.elapsed().as_secs(),
        workers_used: 1,
    })
}

/// Multi-worker Queen: distribute tasks, run in parallel threads.
fn run_multi_worker(config: &HatcheryConfig, start: Instant) -> Result<SwarmResult> {
    let tasks = prd::parse_prd(&config.prd_path)?;
    let total = tasks.len();
    let initial_done = tasks.iter().filter(|t| t.done).count();

    let buckets = prd::distribute_tasks(&tasks, config.workers);

    println!(
        "[HATCHERY] Distributing {} uncompleted tasks among {} workers",
        total - initial_done,
        config.workers
    );
    for (i, bucket) in buckets.iter().enumerate() {
        println!(
            "[HATCHERY]   Worker {}: {} tasks",
            i + 1,
            bucket.len()
        );
    }
    println!();

    let handles: Vec<_> = buckets
        .into_iter()
        .enumerate()
        .filter(|(_, bucket)| !bucket.is_empty())
        .map(|(worker_id, bucket)| {
            let config = config.clone();
            let worker_name = format!("worker-{}", worker_id + 1);

            std::thread::spawn(move || -> Result<usize> {
                let progress_path = config
                    .prd_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(format!(
                        "progress-{}-{}.txt",
                        config
                            .prd_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        worker_name
                    ));
                let mut tracker =
                    ProgressTracker::new(&config.prd_path, Some(progress_path));
                let mut completed = 0;

                for task in &bucket {
                    // Check if task is already done (another worker might have done it)
                    let current_tasks = prd::parse_prd(&config.prd_path)?;
                    if current_tasks
                        .iter()
                        .find(|t| t.id == task.id)
                        .map(|t| t.done)
                        .unwrap_or(true)
                    {
                        continue;
                    }

                    for attempt in 1..=config.max_iterations {
                        if tracker.is_stalled(config.stall_threshold) {
                            println!(
                                "[{}] Stalled, pausing 10s...",
                                worker_name
                            );
                            std::thread::sleep(Duration::from_secs(10));
                        }

                        println!(
                            "[{}] Task {} attempt {}: {}",
                            worker_name,
                            task.id,
                            attempt,
                            truncate(&task.description, 60)
                        );

                        let prd_content =
                            std::fs::read_to_string(&config.prd_path)
                                .unwrap_or_default();
                        let progress_context =
                            tracker.read().unwrap_or_default();
                        let prompt = build_iteration_prompt(
                            &prd_content,
                            &task.description,
                            &progress_context,
                            attempt,
                            tracker.path(),
                        );

                        let result = invoke_claude(&config, &prompt)?;

                        match result {
                            IterationResult::Progress { .. } => {
                                if let Some(ref verify) = config.verify_cmd {
                                    if !run_verify(verify, &config.working_dir)? {
                                        tracker.record_failure(&format!(
                                            "Verification failed for: {}",
                                            task.description
                                        ))?;
                                        continue;
                                    }
                                }

                                let fresh = prd::parse_prd(&config.prd_path)?;
                                if let Some(t) =
                                    fresh.iter().find(|t| t.id == task.id && !t.done)
                                {
                                    prd::mark_complete(&config.prd_path, t)?;
                                }

                                git_commit(&config, &task.description)?;
                                tracker.record_success(&task.description)?;
                                completed += 1;
                                break;
                            }
                            IterationResult::NoProgress { ref reason } => {
                                tracker.record_no_progress(reason)?;
                            }
                            IterationResult::Error { ref message } => {
                                tracker.record_failure(message)?;
                            }
                            IterationResult::AllDone => break,
                        }
                    }
                }

                Ok(completed)
            })
        })
        .collect();

    for handle in handles {
        match handle.join() {
            Ok(Ok(completed)) => {
                println!("[HATCHERY] Worker finished: {} tasks completed", completed);
            }
            Ok(Err(e)) => eprintln!("[HATCHERY] Worker error: {}", e),
            Err(_) => eprintln!("[HATCHERY] Worker panicked"),
        }
    }

    let final_tasks = prd::parse_prd(&config.prd_path)?;
    let (done, _) = prd::progress(&final_tasks);

    println!(
        "\n[HATCHERY] Finished: {} | {}s",
        progress::progress_bar(done, total, 30),
        start.elapsed().as_secs()
    );

    Ok(SwarmResult {
        total_tasks: total,
        completed_tasks: done,
        total_iterations: 0,
        duration_secs: start.elapsed().as_secs(),
        workers_used: config.workers,
    })
}

/// Build the prompt for a single iteration using the embedded template.
fn build_iteration_prompt(
    prd_content: &str,
    task_description: &str,
    progress: &str,
    iteration: usize,
    progress_path: &std::path::Path,
) -> String {
    let mut prompt = String::with_capacity(prd_content.len() + progress.len() + ITERATION_PROMPT.len() + 500);

    // PRD content first (like @$PRD in the shell version)
    prompt.push_str("## PRD\n\n");
    prompt.push_str(prd_content);

    // Progress notes (like @$PROGRESS in the shell version)
    if !progress.is_empty() {
        prompt.push_str("\n\n## Progress from Previous Iterations\n\n");
        if progress.len() > 5000 {
            let start = progress.len() - 5000;
            let adjusted = progress[start..]
                .find('\n')
                .map(|pos| start + pos + 1)
                .unwrap_or(start);
            prompt.push_str(&progress[adjusted..]);
        } else {
            prompt.push_str(progress);
        }
    }

    // The iteration system prompt
    prompt.push_str("\n\n");
    prompt.push_str(ITERATION_PROMPT);

    // Context for this specific iteration
    prompt.push_str(&format!(
        "\n\nProgress file: {}\nIteration: {}\nYour task: {}",
        progress_path.display(),
        iteration,
        task_description,
    ));

    prompt
}

/// Invoke Claude via PipeProcess and collect the result.
fn invoke_claude(config: &HatcheryConfig, prompt: &str) -> Result<IterationResult> {
    let mut process = PipeProcess::new(CliTool::ClaudeCode, &config.working_dir, prompt)
        .context("Failed to spawn Claude process")?;

    let mut parser = zengeld_hub_core::create_ndjson_parser(CliTool::ClaudeCode);
    let mut assistant_text = String::new();
    let mut has_error = false;
    let mut error_msg = String::new();

    let timeout = Instant::now();
    let max_wait = Duration::from_secs(600); // 10 minute timeout

    loop {
        if timeout.elapsed() > max_wait {
            let _ = process.kill();
            return Ok(IterationResult::Error {
                message: "Iteration timed out (10 min)".to_string(),
            });
        }

        if let Some(line) = process.try_recv() {
            let events = parser.parse_line(&line);
            for event in events {
                match event {
                    CliEvent::AssistantText { text, .. } => {
                        assistant_text.push_str(&text);
                        if config.verbose {
                            print!("{}", text);
                        }
                    }
                    CliEvent::Error { message } => {
                        has_error = true;
                        error_msg = message;
                    }
                    CliEvent::SessionEnd { is_error, .. } => {
                        if is_error {
                            has_error = true;
                        }
                    }
                    CliEvent::ToolCallStart { name, .. } => {
                        if config.verbose {
                            println!("[tool: {}]", name);
                        }
                    }
                    _ => {}
                }
            }
        }

        if !process.is_running() {
            while let Some(line) = process.try_recv() {
                let events = parser.parse_line(&line);
                for event in events {
                    if let CliEvent::AssistantText { text, .. } = event {
                        assistant_text.push_str(&text);
                    }
                }
            }
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    if has_error {
        return Ok(IterationResult::Error {
            message: if error_msg.is_empty() {
                "Unknown error".to_string()
            } else {
                error_msg
            },
        });
    }

    // Check for COMPLETE signal
    if assistant_text.contains("<promise>COMPLETE</promise>") {
        return Ok(IterationResult::AllDone);
    }

    if assistant_text.is_empty() {
        Ok(IterationResult::NoProgress {
            reason: "No output from Claude".to_string(),
        })
    } else {
        Ok(IterationResult::Progress {
            task_id: 0,
            description: truncate(&assistant_text, 200).to_string(),
        })
    }
}

/// Run a verification command and return whether it succeeded.
fn run_verify(cmd: &str, working_dir: &std::path::Path) -> Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(true);
    }

    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(working_dir)
            .output()
    } else {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(working_dir)
            .output()
    };

    match output {
        Ok(out) => Ok(out.status.success()),
        Err(e) => {
            eprintln!("[HATCHERY] Verify command failed to run: {}", e);
            Ok(false)
        }
    }
}

/// Git commit with a descriptive message.
fn git_commit(config: &HatcheryConfig, task_desc: &str) -> Result<()> {
    let msg = format!("feat(hatchery): {}", truncate(task_desc, 72));

    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(&config.working_dir)
        .output();

    if let Err(e) = output {
        eprintln!("[HATCHERY] git add failed: {}", e);
        return Ok(());
    }

    let output = Command::new("git")
        .args(["commit", "-m", &msg])
        .current_dir(&config.working_dir)
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("[HATCHERY]   ✓ Committed: {}", truncate(&msg, 60));
            }
        }
        Err(e) => {
            eprintln!("[HATCHERY] git commit failed: {}", e);
        }
    }

    Ok(())
}

/// Truncate string to max length.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max.saturating_sub(3);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
