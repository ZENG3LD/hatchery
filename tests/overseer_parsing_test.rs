//! Integration tests for Overseer parser on REAL Claude Code session files
//!
//! These tests validate that the Overseer module can parse actual JSONL session files
//! from the user's Claude Code project directory. All tests are marked `#[ignore]` because
//! they depend on local filesystem data.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --package hatchery --test overseer_parsing_test -- --ignored --nocapture
//! ```
//!
//! # Test Sessions
//!
//! 1. Small (100KB): `043a16f7-6ff7-4b2a-8c51-16ba925b4478.jsonl`
//! 2. Medium (4.4MB): `0e98812c-873e-476a-b5df-ceb3c15be248.jsonl`
//! 3. Large with subagents (52MB + 130 subagent files): `05ca8304-4316-4a44-baff-f0f04ef8fa2b.jsonl`

use hatchery::overseer::{
    discover_segments, find_session_files, find_subagent_files, parse_jsonl_events,
    parse_session_with_subagents, ContextExtractor, ProgressData, SessionEvent,
};
use std::path::PathBuf;

const SEP_HEAVY: &str = "================================================================================";
const SEP_LIGHT: &str = "--------------------------------------------------------------------------------";

/// Get the nemo project path (the test subject)
fn get_nemo_project_path() -> PathBuf {
    // The hatchery crate is inside the nemo project
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("hatchery should have a parent directory")
        .to_path_buf()
}

/// Get the Claude Code project directory for nemo
fn get_claude_project_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    home.join(".claude")
        .join("projects")
        .join("C--Users-VA-PC-CODING-ML-TRADING-nemo")
}

/// Get specific session file path
fn get_session_path(session_id: &str) -> PathBuf {
    get_claude_project_dir().join(format!("{}.jsonl", session_id))
}

// ============================================================================
// Test 1: Session Discovery
// ============================================================================

#[test]
#[ignore]
fn test_find_session_files() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 1: Session File Discovery");
    println!("{}\n", SEP_HEAVY);

    let nemo_path = get_nemo_project_path();
    println!("Nemo project path: {:?}", nemo_path);

    let files = find_session_files(&nemo_path).expect("Failed to find session files");

    println!("Total session files found: {}", files.len());
    assert!(
        !files.is_empty(),
        "Should find at least one session file"
    );

    // Calculate total size
    let total_size: u64 = files.iter().map(|f| f.size_bytes).sum();
    let total_mb = total_size as f64 / 1_048_576.0;

    // Find largest and smallest
    let largest = files.iter().max_by_key(|f| f.size_bytes);
    let smallest = files.iter().min_by_key(|f| f.size_bytes);

    println!("\n{}", SEP_LIGHT);
    println!("SUMMARY");
    println!("{}", SEP_LIGHT);
    println!("Total files:      {}", files.len());
    println!("Total size:       {:.2} MB", total_mb);
    println!("Average size:     {:.2} KB", (total_size as f64 / files.len() as f64) / 1024.0);

    if let Some(largest_file) = largest {
        println!(
            "\nLargest file:     {:.2} MB - {:?}",
            largest_file.size_bytes as f64 / 1_048_576.0,
            largest_file.path.file_name()
        );
    }

    if let Some(smallest_file) = smallest {
        println!(
            "Smallest file:    {:.2} KB - {:?}",
            smallest_file.size_bytes as f64 / 1024.0,
            smallest_file.path.file_name()
        );
    }

    // Show first 10 files by modified date (most recent first)
    let mut sorted = files.clone();
    sorted.sort_by(|a, b| {
        b.modified
            .unwrap_or(chrono::Utc::now())
            .cmp(&a.modified.unwrap_or(chrono::Utc::now()))
    });

    println!("\n{}", SEP_LIGHT);
    println!("RECENT SESSIONS (last 10)");
    println!("{}", SEP_LIGHT);
    println!("{:<40} {:>12} {:>20}", "Session ID", "Size", "Modified");
    println!("{}", SEP_LIGHT);

    for file in sorted.iter().take(10) {
        let session_id = file.session_id.as_deref().unwrap_or("unknown");
        let size_str = if file.size_bytes > 1_048_576 {
            format!("{:.1} MB", file.size_bytes as f64 / 1_048_576.0)
        } else {
            format!("{:.1} KB", file.size_bytes as f64 / 1024.0)
        };
        let modified_str = file
            .modified
            .map(|m| m.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());

        println!("{:<40} {:>12} {:>20}", session_id, size_str, modified_str);
    }

    println!("\n");
}

// ============================================================================
// Test 2: Parse Small Session
// ============================================================================

#[test]
#[ignore]
fn test_parse_small_session() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 2: Parse Small Session (100KB)");
    println!("{}\n", SEP_HEAVY);

    let session_path = get_session_path("043a16f7-6ff7-4b2a-8c51-16ba925b4478");
    println!("Session file: {:?}", session_path);

    if !session_path.exists() {
        println!("SKIP: Session file not found (may have been deleted)");
        return;
    }

    let events = parse_jsonl_events(&session_path).expect("Failed to parse events");
    println!("Total events: {}", events.len());
    assert!(!events.is_empty(), "Should have at least one event");

    // Count events by type
    let mut counts = std::collections::HashMap::new();
    for event in &events {
        let type_name = match event {
            SessionEvent::User(_) => "User",
            SessionEvent::Assistant(_) => "Assistant",
            SessionEvent::Progress(_) => "Progress",
            SessionEvent::System(_) => "System",
            SessionEvent::FileSnapshot(_) => "FileSnapshot",
            SessionEvent::QueueOperation(_) => "QueueOperation",
            SessionEvent::Summary(_) => "Summary",
            SessionEvent::Unknown => "Unknown",
        };
        *counts.entry(type_name).or_insert(0) += 1;
    }

    println!("\n{}", SEP_LIGHT);
    println!("EVENT DISTRIBUTION");
    println!("{}", SEP_LIGHT);
    println!("{:<20} {:>10} {:>10}", "Event Type", "Count", "Percentage");
    println!("{}", SEP_LIGHT);

    let total = events.len() as f64;
    let mut sorted_counts: Vec<_> = counts.iter().collect();
    sorted_counts.sort_by(|a, b| b.1.cmp(a.1));

    for (event_type, count) in sorted_counts {
        let percentage = (*count as f64 / total) * 100.0;
        println!(
            "{:<20} {:>10} {:>9.1}%",
            event_type, count, percentage
        );
    }

    // Sample first and last events
    println!("\n{}", SEP_LIGHT);
    println!("SAMPLE EVENTS");
    println!("{}", SEP_LIGHT);

    if let Some(first) = events.first() {
        println!("First event: {}", first);
        println!("  Timestamp: {}", first.timestamp());
        if let Some(uuid) = first.uuid() {
            println!("  UUID: {}", uuid);
        }
    }

    if let Some(last) = events.last() {
        println!("\nLast event: {}", last);
        println!("  Timestamp: {}", last.timestamp());
        if let Some(uuid) = last.uuid() {
            println!("  UUID: {}", uuid);
        }
    }

    println!("\n");
}

// ============================================================================
// Test 3: Parse Medium Session with Context Extraction
// ============================================================================

#[test]
#[ignore]
fn test_parse_medium_session() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 3: Parse Medium Session (4.4MB) + Context Extraction");
    println!("{}\n", SEP_HEAVY);

    let session_path = get_session_path("0e98812c-873e-476a-b5df-ceb3c15be248");
    println!("Session file: {:?}", session_path);

    if !session_path.exists() {
        println!("SKIP: Session file not found (may have been deleted)");
        return;
    }

    let events = parse_jsonl_events(&session_path).expect("Failed to parse events");
    println!("Total events: {}", events.len());

    // Count events by type
    let mut counts = std::collections::HashMap::new();
    let mut progress_subtypes = std::collections::HashMap::new();

    for event in &events {
        let type_name = match event {
            SessionEvent::User(_) => "User",
            SessionEvent::Assistant(_) => "Assistant",
            SessionEvent::Progress(p) => {
                // Track progress subtypes
                let subtype = match &p.data {
                    ProgressData::BashProgress(_) => "BashProgress",
                    ProgressData::HookProgress(_) => "HookProgress",
                    ProgressData::AgentProgress(_) => "AgentProgress",
                    ProgressData::QueryUpdate(_) => "QueryUpdate",
                    ProgressData::SearchResultsReceived(_) => "SearchResultsReceived",
                    ProgressData::WaitingForTask(_) => "WaitingForTask",
                    ProgressData::Unknown => "UnknownProgress",
                };
                *progress_subtypes.entry(subtype).or_insert(0) += 1;
                "Progress"
            }
            SessionEvent::System(_) => "System",
            SessionEvent::FileSnapshot(_) => "FileSnapshot",
            SessionEvent::QueueOperation(_) => "QueueOperation",
            SessionEvent::Summary(_) => "Summary",
            SessionEvent::Unknown => "Unknown",
        };
        *counts.entry(type_name).or_insert(0) += 1;
    }

    println!("\n{}", SEP_LIGHT);
    println!("EVENT DISTRIBUTION");
    println!("{}", SEP_LIGHT);
    println!("{:<20} {:>10} {:>10}", "Event Type", "Count", "Percentage");
    println!("{}", SEP_LIGHT);

    let total = events.len() as f64;
    let mut sorted_counts: Vec<_> = counts.iter().collect();
    sorted_counts.sort_by(|a, b| b.1.cmp(a.1));

    for (event_type, count) in sorted_counts {
        let percentage = (*count as f64 / total) * 100.0;
        println!(
            "{:<20} {:>10} {:>9.1}%",
            event_type, count, percentage
        );
    }

    println!("\n{}", SEP_LIGHT);
    println!("PROGRESS EVENT SUBTYPES");
    println!("{}", SEP_LIGHT);

    let mut sorted_progress: Vec<_> = progress_subtypes.iter().collect();
    sorted_progress.sort_by(|a, b| b.1.cmp(a.1));

    for (subtype, count) in sorted_progress {
        println!("{:<30} {:>10}", subtype, count);
    }

    // Context extraction
    println!("\n{}", SEP_LIGHT);
    println!("CONTEXT EXTRACTION");
    println!("{}", SEP_LIGHT);

    let mut extractor = ContextExtractor::new();
    let context = extractor.process_events(&events);

    println!("Session ID:       {}", context.session_id);
    println!("Segment index:    {}", context.segment_index);
    println!("Agent tasks:      {}", context.agent_tasks.len());
    println!("Decisions:        {}", context.decisions.len());
    println!("Files modified:   {}", context.files_modified.len());
    if let Some(pre_tokens) = context.pre_tokens {
        println!("Pre-compact tokens: {}", pre_tokens);
    }
    if let Some(ref cwd) = context.cwd {
        println!("Working directory: {}", cwd);
    }
    if let Some(ref branch) = context.git_branch {
        println!("Git branch:       {}", branch);
    }

    // Sample agent tasks
    if !context.agent_tasks.is_empty() {
        println!("\n{}", SEP_LIGHT);
        println!("SAMPLE AGENT TASKS (first 5)");
        println!("{}", SEP_LIGHT);

        for (i, task) in context.agent_tasks.iter().take(5).enumerate() {
            println!("\nTask {}:", i + 1);
            println!("  Agent ID: {}", task.agent_id);
            println!("  Timestamp: {}", task.timestamp);
            println!("  Prompt (first 100 chars): {}",
                task.prompt.chars().take(100).collect::<String>());
            if let Some(ref slug) = task.slug {
                println!("  Slug: {}", slug);
            }
        }
    }

    // Sample decisions
    if !context.decisions.is_empty() {
        println!("\n{}", SEP_LIGHT);
        println!("SAMPLE DECISIONS (first 3)");
        println!("{}", SEP_LIGHT);

        for (i, decision) in context.decisions.iter().take(3).enumerate() {
            println!("\nDecision {}:", i + 1);
            println!("  Timestamp: {}", decision.timestamp);
            println!("  Question (first 80 chars): {}",
                decision.question.chars().take(80).collect::<String>());
            println!("  Answer (first 80 chars): {}",
                decision.answer.chars().take(80).collect::<String>());
        }
    }

    println!("\n");
}

// ============================================================================
// Test 4: Parse Session with Subagents
// ============================================================================

#[test]
#[ignore]
fn test_parse_session_with_subagents() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 4: Parse Session with Subagents (52MB + 130 subagent files)");
    println!("{}\n", SEP_HEAVY);

    let session_path = get_session_path("05ca8304-4316-4a44-baff-f0f04ef8fa2b");
    println!("Session file: {:?}", session_path);

    if !session_path.exists() {
        println!("SKIP: Session file not found (may have been deleted)");
        return;
    }

    // First check how many subagent files exist
    let subagent_files = find_subagent_files(&session_path);
    println!("Subagent files found: {}", subagent_files.len());

    // Parse full session with subagents
    println!("Parsing main session + all subagents (this may take a moment)...");
    let session = parse_session_with_subagents(&session_path)
        .expect("Failed to parse session with subagents");

    println!("\n{}", SEP_LIGHT);
    println!("SESSION OVERVIEW");
    println!("{}", SEP_LIGHT);
    println!("Session ID:       {}", session.session_id);
    println!("Main events:      {}", session.events.len());
    println!("Subagents:        {}", session.subagents.len());

    assert!(
        !session.subagents.is_empty(),
        "Should have at least one subagent"
    );

    // Analyze subagents
    println!("\n{}", SEP_LIGHT);
    println!("SUBAGENT DETAILS");
    println!("{}", SEP_LIGHT);
    println!(
        "{:<12} {:<15} {:<20} {:>10} {:>10}",
        "Agent ID", "Model", "Type", "Events", "Tokens"
    );
    println!("{}", SEP_LIGHT);

    let mut total_subagent_events = 0;
    let mut total_tokens = 0;

    for subagent in &session.subagents {
        let agent_type = subagent.subagent_type.as_deref().unwrap_or("unknown");
        let short_model = if subagent.model.contains("sonnet") {
            "sonnet"
        } else if subagent.model.contains("opus") {
            "opus"
        } else if subagent.model.contains("haiku") {
            "haiku"
        } else {
            "other"
        };

        println!(
            "{:<12} {:<15} {:<20} {:>10} {:>10}",
            subagent.agent_id,
            short_model,
            agent_type,
            subagent.events.len(),
            subagent.total_tokens
        );

        total_subagent_events += subagent.events.len();
        total_tokens += subagent.total_tokens;
    }

    println!("{}", SEP_LIGHT);
    println!(
        "{:<12} {:<15} {:<20} {:>10} {:>10}",
        "TOTALS", "", "", total_subagent_events, total_tokens
    );

    // Model distribution
    let mut model_counts = std::collections::HashMap::new();
    for subagent in &session.subagents {
        let model_key = if subagent.model.contains("sonnet") {
            "sonnet"
        } else if subagent.model.contains("opus") {
            "opus"
        } else if subagent.model.contains("haiku") {
            "haiku"
        } else {
            "other"
        };
        *model_counts.entry(model_key).or_insert(0) += 1;
    }

    println!("\n{}", SEP_LIGHT);
    println!("MODEL DISTRIBUTION");
    println!("{}", SEP_LIGHT);
    for (model, count) in model_counts {
        println!("{:<15} {:>10}", model, count);
    }

    // Sample a subagent's events
    if let Some(first_subagent) = session.subagents.first() {
        println!("\n{}", SEP_LIGHT);
        println!("SAMPLE SUBAGENT EVENTS (Agent: {})", first_subagent.agent_id);
        println!("{}", SEP_LIGHT);

        let mut event_counts = std::collections::HashMap::new();
        for event in &first_subagent.events {
            let type_name = match event {
                SessionEvent::User(_) => "User",
                SessionEvent::Assistant(_) => "Assistant",
                SessionEvent::Progress(_) => "Progress",
                SessionEvent::System(_) => "System",
                SessionEvent::FileSnapshot(_) => "FileSnapshot",
                SessionEvent::QueueOperation(_) => "QueueOperation",
                SessionEvent::Summary(_) => "Summary",
                SessionEvent::Unknown => "Unknown",
            };
            *event_counts.entry(type_name).or_insert(0) += 1;
        }

        println!("{:<20} {:>10}", "Event Type", "Count");
        println!("{}", SEP_LIGHT);
        for (event_type, count) in event_counts {
            println!("{:<20} {:>10}", event_type, count);
        }
    }

    println!("\n");
}

// ============================================================================
// Test 5: Discover Segments
// ============================================================================

#[test]
#[ignore]
fn test_discover_segments() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 5: Discover Segments (Parser V2)");
    println!("{}\n", SEP_HEAVY);

    let session_path = get_session_path("0e98812c-873e-476a-b5df-ceb3c15be248");
    println!("Session file: {:?}", session_path);

    if !session_path.exists() {
        println!("SKIP: Session file not found (may have been deleted)");
        return;
    }

    let segments = discover_segments(&session_path).expect("Failed to discover segments");

    println!("Total segments: {}", segments.len());

    if segments.is_empty() {
        println!("No segments found (file may be small with no compact boundaries)");
        return;
    }

    println!("\n{}", SEP_LIGHT);
    println!("SEGMENT BOUNDARIES");
    println!("{}", SEP_LIGHT);
    println!(
        "{:<8} {:>12} {:>12} {:>12} {:>10}",
        "Index", "Start Line", "End Line", "Est. Events", "Pre-Tokens"
    );
    println!("{}", SEP_LIGHT);

    for segment in &segments {
        println!(
            "{:<8} {:>12} {:>12} {:>12} {:>10}",
            segment.index,
            segment.start_line,
            segment.end_line,
            segment.estimated_events,
            segment.pre_tokens
        );
    }

    // Show segment timeline
    println!("\n{}", SEP_LIGHT);
    println!("SEGMENT TIMELINE");
    println!("{}", SEP_LIGHT);

    for segment in &segments {
        println!("\nSegment {}:", segment.index);
        println!("  Time range: {} to {}",
            segment.start_timestamp.format("%Y-%m-%d %H:%M:%S"),
            segment.end_timestamp.format("%Y-%m-%d %H:%M:%S"));
        println!("  Duration: {:.1} minutes",
            (segment.end_timestamp - segment.start_timestamp).num_seconds() as f64 / 60.0);
        println!("  Trigger: {}", segment.trigger);
        if let Some(ref cwd) = segment.cwd {
            println!("  CWD: {}", cwd);
        }
        if let Some(ref branch) = segment.git_branch {
            println!("  Git branch: {}", branch);
        }
    }

    println!("\n");
}

// ============================================================================
// Test 6: Context Extraction with Subagents (Full Detail)
// ============================================================================

#[test]
#[ignore]
fn test_context_extraction_with_subagents() {
    println!("\n{}", SEP_HEAVY);
    println!("TEST 6: Detailed Context Extraction (Subagent Session)");
    println!("{}\n", SEP_HEAVY);

    let session_path = get_session_path("05ca8304-4316-4a44-baff-f0f04ef8fa2b");
    println!("Session file: {:?}", session_path);

    if !session_path.exists() {
        println!("SKIP: Session file not found (may have been deleted)");
        return;
    }

    println!("Parsing session...");
    let session = parse_session_with_subagents(&session_path)
        .expect("Failed to parse session with subagents");

    println!("Extracting context from main session events...");
    let mut extractor = ContextExtractor::new();
    let context = extractor.process_events(&session.events);

    println!("\n{}", SEP_LIGHT);
    println!("CONTEXT SUMMARY");
    println!("{}", SEP_LIGHT);
    println!("Session ID:       {}", context.session_id);
    println!("Segment index:    {}", context.segment_index);
    println!("Start timestamp:  {}",
        chrono::DateTime::<chrono::Utc>::from_timestamp(context.start_timestamp, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d %H:%M:%S"));
    println!("End timestamp:    {}",
        chrono::DateTime::<chrono::Utc>::from_timestamp(context.end_timestamp, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d %H:%M:%S"));

    if let Some(ref summary) = context.compact_summary {
        println!("\nCompact Summary:");
        println!("  {}", summary);
    }

    if let Some(pre_tokens) = context.pre_tokens {
        println!("\nPre-compact tokens: {}", pre_tokens);
    }

    if let Some(ref cwd) = context.cwd {
        println!("Working directory: {}", cwd);
    }

    if let Some(ref branch) = context.git_branch {
        println!("Git branch: {}", branch);
    }

    // Agent tasks
    println!("\n{}", SEP_LIGHT);
    println!("AGENT TASKS ({})", context.agent_tasks.len());
    println!("{}", SEP_LIGHT);

    if context.agent_tasks.is_empty() {
        println!("No agent tasks found.");
    } else {
        println!(
            "{:<12} {:<20} {:<25} {:>80}",
            "Agent ID", "Timestamp", "Slug", "Prompt Preview"
        );
        println!("{}", SEP_LIGHT);

        for task in context.agent_tasks.iter().take(10) {
            let slug = task.slug.as_deref().unwrap_or("none");
            let prompt_preview = task.prompt.chars().take(80).collect::<String>();
            println!(
                "{:<12} {:<20} {:<25} {}",
                task.agent_id,
                task.timestamp.format("%Y-%m-%d %H:%M:%S"),
                slug,
                prompt_preview
            );
        }

        if context.agent_tasks.len() > 10 {
            println!("... and {} more", context.agent_tasks.len() - 10);
        }
    }

    // Decisions
    println!("\n{}", SEP_LIGHT);
    println!("DECISIONS ({})", context.decisions.len());
    println!("{}", SEP_LIGHT);

    if context.decisions.is_empty() {
        println!("No decisions found.");
    } else {
        for (i, decision) in context.decisions.iter().take(5).enumerate() {
            println!("\nDecision {}:", i + 1);
            println!("  Timestamp: {}", decision.timestamp.format("%Y-%m-%d %H:%M:%S"));
            println!("  Question: {}",
                decision.question.chars().take(120).collect::<String>());
            println!("  Answer: {}",
                decision.answer.chars().take(120).collect::<String>());

            if let Some(ref ctx) = decision.context {
                if let Some(ref cwd) = ctx.cwd {
                    println!("  Context CWD: {}", cwd);
                }
                if let Some(ref branch) = ctx.git_branch {
                    println!("  Context Branch: {}", branch);
                }
            }
        }

        if context.decisions.len() > 5 {
            println!("\n... and {} more decisions", context.decisions.len() - 5);
        }
    }

    // File modifications
    println!("\n{}", SEP_LIGHT);
    println!("FILES MODIFIED ({})", context.files_modified.len());
    println!("{}", SEP_LIGHT);

    if context.files_modified.is_empty() {
        println!("No file modifications found.");
    } else {
        for (i, file_path) in context.files_modified.iter().take(20).enumerate() {
            println!("{}. {}", i + 1, file_path);
        }

        if context.files_modified.len() > 20 {
            println!("... and {} more files", context.files_modified.len() - 20);
        }
    }

    // Cross-reference with subagents
    println!("\n{}", SEP_LIGHT);
    println!("SUBAGENT CORRELATION");
    println!("{}", SEP_LIGHT);
    println!("Agent tasks extracted: {}", context.agent_tasks.len());
    println!("Subagent files found:  {}", session.subagents.len());

    if context.agent_tasks.len() != session.subagents.len() {
        println!("\nNote: Task count != subagent file count.");
        println!("This is normal - some tasks may be inline or some subagent files may be orphaned.");
    }

    // Match agent IDs
    let task_agent_ids: std::collections::HashSet<_> =
        context.agent_tasks.iter().map(|t| t.agent_id.as_str()).collect();
    let subagent_agent_ids: std::collections::HashSet<_> =
        session.subagents.iter().map(|s| s.agent_id.as_str()).collect();

    let matched = task_agent_ids.intersection(&subagent_agent_ids).count();
    let task_only = task_agent_ids.difference(&subagent_agent_ids).count();
    let subagent_only = subagent_agent_ids.difference(&task_agent_ids).count();

    println!("\nAgent ID matching:");
    println!("  Matched:       {} (task and subagent file both exist)", matched);
    println!("  Task only:     {} (task recorded but no subagent file)", task_only);
    println!("  Subagent only: {} (subagent file but no task record)", subagent_only);

    println!("\n");
}
