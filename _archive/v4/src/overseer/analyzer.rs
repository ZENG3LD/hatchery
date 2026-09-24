//! Metadata-first JSONL analyzer
//!
//! This module implements a two-pass analyzer that extracts rich metadata
//! from Claude Code session JSONL files:
//!
//! **Pass 1: Segment Discovery**
//! - Scan for compact_boundary events
//! - Build segment index (O(segments) memory)
//! - Extract segment metadata
//!
//! **Pass 2: Event Processing**
//! - Process events within each segment
//! - Extract typed events
//! - Populate normalized database tables
//!
//! # Example
//!
//! ```no_run
//! use hatchery::overseer::analyzer::{discover_segments, SegmentParser};
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let segments = discover_segments(Path::new("session.jsonl"))?;
//! println!("Discovered {} segments", segments.len());
//! # Ok(())
//! # }
//! ```

use crate::overseer::error::Result;
use crate::overseer::events::*;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use tracing::{debug, info, warn};

// ============================================================================
// Parser Configuration
// ============================================================================

/// Configuration for session parser
#[derive(Debug, Clone)]
pub struct ParserConfig {
    /// Store full event data in database (can be large)
    pub store_full_events: bool,

    /// Only index text content for FTS (saves space)
    pub fts_text_only: bool,

    /// Batch size for database inserts
    pub batch_size: usize,

    /// Skip events older than this (for incremental parsing)
    pub skip_before: Option<DateTime<Utc>>,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            store_full_events: true,
            fts_text_only: true,
            batch_size: 1000,
            skip_before: None,
        }
    }
}

// ============================================================================
// Segment Boundary
// ============================================================================

/// Segment boundary discovered in Pass 1
#[derive(Debug, Clone)]
pub struct SegmentBoundary {
    /// Segment index (0, 1, 2...)
    pub index: usize,

    /// Byte offsets in JSONL file
    pub start_offset: u64,
    pub end_offset: u64,

    /// Line numbers in JSONL file
    pub start_line: usize,
    pub end_line: usize,

    /// Timestamps
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,

    /// UUIDs
    pub start_uuid: String,
    pub end_uuid: String,

    /// Estimated event count
    pub estimated_events: usize,

    /// Compact boundary metadata
    pub pre_tokens: u64,
    pub trigger: String,

    /// Context snapshot
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub claude_version: Option<String>,
}

// ============================================================================
// Extracted Metadata Structures (re-exported from types module)
// ============================================================================
// NOTE: These types are now imported from crate::overseer::types to avoid duplication.
// The types in types.rs are the canonical definitions with full database support.

use crate::overseer::types::{
    AgentActivity as TypesAgentActivity, ConversationEdge as TypesConversationEdge,
    FileChange as TypesFileChange, ToolActivity as TypesToolActivity,
};

/// Segment statistics
#[derive(Debug, Clone, Default)]
pub struct SegmentStats {
    pub message_count: i64,
    pub user_message_count: i64,
    pub assistant_message_count: i64,
    pub tool_use_count: i64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub estimated_cost_usd: f64,
}

/// Parsed segment with all extracted metadata
#[derive(Debug, Clone)]
pub struct ParsedSegment {
    pub boundary: SegmentBoundary,
    pub stats: SegmentStats,
    pub agents: Vec<TypesAgentActivity>,
    pub tools: Vec<TypesToolActivity>,
    pub files: Vec<TypesFileChange>,
    pub edges: Vec<TypesConversationEdge>,
}

/// Import statistics
#[derive(Debug, Clone, Default)]
pub struct ImportStats {
    pub session_id: String,
    pub segments_processed: usize,
    pub events_imported: usize,
    pub agents_tracked: usize,
    pub tools_tracked: usize,
    pub files_tracked: usize,
    pub total_tokens: i64,
    pub estimated_cost: f64,
}

// ============================================================================
// Progress Reporting
// ============================================================================

/// Progress reporter trait for monitoring import progress
pub trait ProgressReporter: Send + Sync {
    fn on_segment_start(&self, segment: usize);
    fn on_segment_complete(&self, segment: usize, stats: &SegmentStats);
}

/// Silent progress reporter (no output)
pub struct SilentReporter;

impl ProgressReporter for SilentReporter {
    fn on_segment_start(&self, _segment: usize) {}
    fn on_segment_complete(&self, _segment: usize, _stats: &SegmentStats) {}
}

// ============================================================================
// Pass 1: Segment Discovery
// ============================================================================

/// Discover segment boundaries by scanning for compact_boundary events
///
/// This function scans the JSONL file line by line, looking for compact_boundary
/// events. It tracks byte offsets for efficient seeking in Pass 2.
///
/// # Arguments
/// * `path` - Path to the JSONL file
///
/// # Returns
/// Vector of segment boundaries, sorted chronologically
pub fn discover_segments(path: &Path) -> Result<Vec<SegmentBoundary>> {
    debug!("Pass 1: Discovering segments in {}", path.display());

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut segments = Vec::new();
    let mut line_number = 0;
    let mut byte_offset = 0u64;
    let mut segment_index = 0;
    let mut last_boundary: Option<SegmentBoundary> = None;
    let mut first_event: Option<(usize, u64, DateTime<Utc>, String)> = None;

    let mut line = String::new();
    loop {
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        line_number += 1;
        let line_start_offset = byte_offset;
        byte_offset += bytes_read as u64;

        // Skip empty lines
        if line.trim().is_empty() {
            line.clear();
            continue;
        }

        // Quick check: does this line contain "compact_boundary"?
        if line.contains("compact_boundary") {
            // Parse as system event to check subtype
            match serde_json::from_str::<SessionEvent>(&line) {
                Ok(SessionEvent::System(sys_event)) if sys_event.is_compact_boundary() => {
                    let timestamp = sys_event.timestamp;
                    let uuid = sys_event.uuid.unwrap_or_default();
                    let compact_meta = sys_event.compact_metadata.unwrap_or(CompactMetadata {
                        trigger: "unknown".to_string(),
                        pre_tokens: 0,
                        post_tokens: None,
                    });

                    // Complete previous segment
                    if let Some(prev) = last_boundary.take() {
                        segments.push(SegmentBoundary {
                            index: segment_index,
                            start_offset: prev.end_offset,
                            end_offset: line_start_offset,
                            start_line: prev.end_line + 1,
                            end_line: line_number,
                            start_timestamp: prev.end_timestamp,
                            end_timestamp: timestamp,
                            start_uuid: prev.end_uuid.clone(),
                            end_uuid: uuid.clone(),
                            estimated_events: line_number - prev.end_line,
                            pre_tokens: compact_meta.pre_tokens,
                            trigger: compact_meta.trigger.clone(),
                            cwd: sys_event.cwd.clone(),
                            git_branch: sys_event.git_branch.clone(),
                            claude_version: sys_event.version.clone(),
                        });
                        segment_index += 1;
                    }

                    // Start new segment boundary
                    last_boundary = Some(SegmentBoundary {
                        index: segment_index,
                        start_offset: line_start_offset,
                        end_offset: byte_offset,
                        start_line: line_number,
                        end_line: line_number,
                        start_timestamp: timestamp,
                        end_timestamp: timestamp,
                        start_uuid: uuid.clone(),
                        end_uuid: uuid,
                        estimated_events: 0,
                        pre_tokens: compact_meta.pre_tokens,
                        trigger: compact_meta.trigger,
                        cwd: sys_event.cwd,
                        git_branch: sys_event.git_branch,
                        claude_version: sys_event.version,
                    });
                }
                _ => {}
            }
        } else if first_event.is_none() {
            // Capture first event timestamp and UUID
            if let Ok(event) = serde_json::from_str::<SessionEvent>(&line) {
                if let Some(uuid) = event.uuid() {
                    first_event = Some((
                        line_number,
                        line_start_offset,
                        event.timestamp(),
                        uuid.to_string(),
                    ));
                }
            }
        }

        line.clear();
    }

    // Handle case where there are no compact boundaries
    if segments.is_empty() {
        if let Some((_line, offset, timestamp, uuid)) = first_event {
            info!(
                "No compact boundaries found in {}. Creating single segment for entire session ({} events).",
                path.display(),
                line_number
            );
            // Create a single segment from start to end of file
            segments.push(SegmentBoundary {
                index: 0,
                start_offset: offset,
                end_offset: byte_offset,
                start_line: 1,
                end_line: line_number,
                start_timestamp: timestamp,
                end_timestamp: Utc::now(),
                start_uuid: uuid.clone(),
                end_uuid: uuid,
                estimated_events: line_number,
                pre_tokens: 0,
                trigger: "no_boundaries".to_string(),
                cwd: None,
                git_branch: None,
                claude_version: None,
            });
        } else {
            warn!("No events found in file: {}", path.display());
        }
    } else if let Some(last) = last_boundary {
        // Handle final segment (from last boundary to EOF)
        segments.push(SegmentBoundary {
            index: segment_index,
            start_offset: last.end_offset,
            end_offset: byte_offset,
            start_line: last.end_line + 1,
            end_line: line_number,
            start_timestamp: last.end_timestamp,
            end_timestamp: Utc::now(),
            start_uuid: last.end_uuid,
            end_uuid: "eof".to_string(),
            estimated_events: line_number - last.end_line,
            pre_tokens: 0,
            trigger: "ongoing".to_string(),
            cwd: last.cwd,
            git_branch: last.git_branch,
            claude_version: last.claude_version,
        });
    }

    info!("Discovered {} segments", segments.len());
    Ok(segments)
}

// ============================================================================
// Pass 2: Event Processing
// ============================================================================

/// Segment parser (database-free)
#[allow(dead_code)]
pub struct SegmentParser {
    config: ParserConfig,
}

impl SegmentParser {
    /// Create a new segment parser
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
        }
    }

    /// Create parser with custom configuration
    pub fn with_config(config: ParserConfig) -> Self {
        Self { config }
    }

    /// Parse a segment from the JSONL file
    ///
    /// This function seeks to the segment's start offset and reads events
    /// until the end offset is reached. It extracts all metadata and
    /// populates the database.
    ///
    /// # Arguments
    /// * `path` - Path to the JSONL file
    /// * `segment` - Segment boundary to parse
    /// * `session_id` - Session ID for database insertion
    ///
    /// # Returns
    /// Parsed segment with all extracted metadata
    pub async fn parse_segment(
        &self,
        path: &Path,
        segment: &SegmentBoundary,
        _session_id: &str,
    ) -> Result<ParsedSegment> {
        debug!("Pass 2: Processing segment {}", segment.index);

        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(segment.start_offset))?;

        let reader = BufReader::new(file);

        // Segment accumulators
        let mut agents = Vec::new();
        let mut tools: HashMap<String, TypesToolActivity> = HashMap::new();
        let mut files = Vec::new();
        let mut edges = Vec::new();

        // Statistics
        let mut stats = SegmentStats::default();

        let mut current_offset = segment.start_offset;

        for line_result in reader.lines() {
            let line_content = line_result?;
            let line_bytes = line_content.len() as u64 + 1; // +1 for newline

            // Check if we've reached the end of this segment
            if current_offset >= segment.end_offset {
                break;
            }

            current_offset += line_bytes;

            if line_content.trim().is_empty() {
                continue;
            }

            // Parse typed event
            let event: SessionEvent = match serde_json::from_str(&line_content) {
                Ok(e) => e,
                Err(err) => {
                    warn!(
                        "Failed to parse event at offset {}: {}",
                        current_offset - line_bytes,
                        err
                    );
                    continue;
                }
            };

            // Add to conversation graph
            if let Some(uuid) = event.uuid() {
                edges.push(TypesConversationEdge {
                    uuid: uuid.to_string(),
                    parent_uuid: event.parent_uuid().map(String::from),
                    logical_parent_uuid: None, // Extracted from system events
                    event_type: match &event {
                        SessionEvent::User(_) => "user".to_string(),
                        SessionEvent::Assistant(_) => "assistant".to_string(),
                        SessionEvent::Progress(_) => "progress".to_string(),
                        SessionEvent::System(_) => "system".to_string(),
                        _ => "other".to_string(),
                    },
                    timestamp: event.timestamp().timestamp(),
                    session_id: String::new(), // Will be filled by caller
                    segment_id: None,
                    is_sidechain: event.metadata().map(|m| m.is_sidechain).unwrap_or(false),
                });
            }

            // Process by event type
            match event {
                SessionEvent::User(user_event) => {
                    stats.message_count += 1;
                    stats.user_message_count += 1;

                    // Extract tool results
                    if let Some(tool_result) = user_event.try_parse_tool_result() {
                        // Link result to tool invocation
                        if let Some(source_uuid) = &user_event.source_tool_assistant_uuid {
                            // Find matching tool invocation and update result
                            for tool in tools.values_mut() {
                                if tool.invocation_uuid == *source_uuid {
                                    tool.result_timestamp =
                                        Some(user_event.timestamp().timestamp());
                                    tool.result_uuid = Some(user_event.uuid().to_string());
                                    if let Some(duration) = tool.result_timestamp {
                                        tool.duration_ms =
                                            Some((duration - tool.invocation_timestamp) * 1000);
                                    }
                                    break;
                                }
                            }
                        }

                        // Extract file changes
                        if let Some(file_path) = tool_result.file_path() {
                            files.push(TypesFileChange {
                                id: None,
                                session_id: String::new(), // Will be filled by caller
                                segment_id: 0, // Will be filled by caller
                                tool_activity_id: None,
                                file_path: file_path.to_string(),
                                operation: if tool_result.is_create() {
                                    "create"
                                } else if tool_result.is_update() {
                                    "update"
                                } else {
                                    "unknown"
                                }
                                .to_string(),
                                timestamp: user_event.timestamp().timestamp(),
                                message_uuid: user_event.uuid().to_string(),
                                previous_content_hash: None,
                                new_content_hash: None,
                                size_bytes: None,
                            });
                        }
                    }
                }

                SessionEvent::Assistant(assistant_event) => {
                    stats.message_count += 1;
                    stats.assistant_message_count += 1;

                    // Extract token usage
                    if let Some(usage) = &assistant_event.message.usage {
                        stats.total_input_tokens += usage.input_tokens;
                        stats.total_output_tokens += usage.output_tokens;
                        stats.total_cache_write_tokens += usage.cache_creation_input_tokens;
                        stats.total_cache_read_tokens += usage.cache_read_input_tokens;
                    }

                    // Extract tool uses
                    for content in &assistant_event.message.content {
                        if let Some((tool_id, tool_name, params)) = content.as_tool_use() {
                            stats.tool_use_count += 1;

                            tools.insert(
                                tool_id.to_string(),
                                TypesToolActivity {
                                    id: None,
                                    session_id: String::new(), // Will be filled by caller
                                    segment_id: 0, // Will be filled by caller
                                    tool_use_id: tool_id.to_string(),
                                    tool_name: tool_name.to_string(),
                                    invocation_timestamp: assistant_event.timestamp().timestamp(),
                                    invocation_uuid: assistant_event.uuid().to_string(),
                                    result_timestamp: None,
                                    result_uuid: None,
                                    duration_ms: None,
                                    parameters: Some(serde_json::to_string(params).unwrap_or_default()),
                                    result_type: None,
                                    result_summary: None,
                                    file_path: None,
                                    operation_type: None,
                                },
                            );
                        }
                    }
                }

                SessionEvent::Progress(progress_event) => {
                    // Extract agent progress
                    if let ProgressData::AgentProgress(agent_data) = &progress_event.data {
                        agents.push(TypesAgentActivity {
                            id: None,
                            session_id: String::new(), // Will be filled by caller
                            segment_id: 0, // Will be filled by caller
                            agent_id: agent_data.agent_id.clone(),
                            agent_slug: progress_event.metadata.slug.clone(),
                            prompt: agent_data.prompt.clone(),
                            spawn_timestamp: progress_event.timestamp().timestamp(),
                            spawn_uuid: progress_event.uuid().to_string(),
                            parent_tool_use_id: progress_event.parent_tool_use_id.clone(),
                            result_uuid: None,
                            result_timestamp: None,
                            success: None,
                            error_message: None,
                            subagent_file: None,
                            total_input_tokens: None,
                            total_output_tokens: None,
                            estimated_cost_usd: None,
                        });
                    }
                }

                SessionEvent::System(sys_event) => {
                    // Handle logical parent UUID for compact boundaries
                    if sys_event.is_compact_boundary() {
                        if let Some(logical_parent) = &sys_event.logical_parent_uuid {
                            if let Some(edge) = edges.last_mut() {
                                edge.logical_parent_uuid = Some(logical_parent.clone());
                            }
                        }
                    }
                }

                SessionEvent::FileSnapshot(snapshot) => {
                    // Extract file changes from snapshot
                    // Use top-level timestamp or fall back to snapshot.timestamp
                    let timestamp = snapshot.timestamp.unwrap_or(snapshot.snapshot.timestamp);
                    for file_path in snapshot.snapshot.tracked_file_backups.keys() {
                        // Check if we already have this file change
                        if !files.iter().any(|f| {
                            f.file_path == *file_path
                                && f.timestamp == timestamp.timestamp()
                        }) {
                            files.push(TypesFileChange {
                                id: None,
                                session_id: String::new(), // Will be filled by caller
                                segment_id: 0, // Will be filled by caller
                                tool_activity_id: None,
                                file_path: file_path.clone(),
                                operation: "snapshot".to_string(),
                                timestamp: timestamp.timestamp(),
                                message_uuid: snapshot.message_id.clone(),
                                previous_content_hash: None,
                                new_content_hash: None,
                                size_bytes: None,
                            });
                        }
                    }
                }

                _ => {}
            }
        }

        // Calculate estimated cost (using Sonnet pricing as default)
        let usage = TokenUsage {
            input_tokens: stats.total_input_tokens,
            output_tokens: stats.total_output_tokens,
            cache_creation_input_tokens: stats.total_cache_write_tokens,
            cache_read_input_tokens: stats.total_cache_read_tokens,
            ..Default::default()
        };
        stats.estimated_cost_usd = usage.calculate_cost("sonnet").unwrap_or(0.0);

        Ok(ParsedSegment {
            boundary: segment.clone(),
            stats,
            agents,
            tools: tools.into_values().collect(),
            files,
            edges,
        })
    }
}

// ============================================================================
// High-Level API (Database Integration - COMMENTED OUT)
// ============================================================================

// TODO: Database integration removed to eliminate circular dependencies
// Restore when storage layer is properly separated

/*
/// Import a session JSONL file into the database
///
/// This function performs a complete two-pass import:
/// 1. Discovers all segment boundaries
/// 2. Parses each segment and extracts metadata
/// 3. Inserts everything into the database
///
/// # Arguments
/// * `db` - Database manager
/// * `path` - Path to the JSONL file
///
/// # Returns
/// Import statistics
pub async fn import_session_file(db: &DatabaseManager, path: &Path) -> Result<ImportStats> {
    import_session_file_with_reporter(db, path, &SilentReporter).await
}
*/

/*
/// Import a session JSONL file with progress reporting
///
/// # Arguments
/// * `db` - Database manager
/// * `path` - Path to the JSONL file
/// * `reporter` - Progress reporter
///
/// # Returns
/// Import statistics
pub async fn import_session_file_with_reporter(
    db: &DatabaseManager,
    path: &Path,
    reporter: &dyn ProgressReporter,
) -> Result<ImportStats> {
    info!("Importing session from: {}", path.display());

    // Pass 1: Discover segments
    let segments = discover_segments(path)?;

    if segments.is_empty() {
        return Err(MemoryError::InvalidSession(format!(
            "No segments found in {}",
            path.display()
        )));
    }

    // Extract session ID from first segment UUID
    let session_id = extract_session_id_from_uuid(&segments[0].start_uuid);

    // Pass 2: Process each segment
    let parser = SegmentParser::new();
    let mut stats = ImportStats {
        session_id: session_id.clone(),
        ..Default::default()
    };

    for segment_boundary in &segments {
        reporter.on_segment_start(segment_boundary.index);

        let parsed = parser
            .parse_segment(path, segment_boundary, &session_id)
            .await?;

        // Update statistics
        stats.segments_processed += 1;
        stats.events_imported += parsed.stats.message_count as usize;
        stats.agents_tracked += parsed.agents.len();
        stats.tools_tracked += parsed.tools.len();
        stats.files_tracked += parsed.files.len();
        stats.total_tokens +=
            parsed.stats.total_input_tokens as i64 + parsed.stats.total_output_tokens as i64;
        stats.estimated_cost += parsed.stats.estimated_cost_usd;

        reporter.on_segment_complete(segment_boundary.index, &parsed.stats);

        // Extract and insert context summary for this segment
        match extract_and_insert_context(
            db,
            &session_id,
            segment_boundary.index,
            &parsed,
            segment_boundary,
        )
        .await
        {
            Ok(()) => {
                debug!(
                    "Context extracted for segment {}: {} agents, {} files",
                    segment_boundary.index,
                    parsed.agents.len(),
                    parsed.files.len()
                );
            }
            Err(e) => {
                warn!(
                    "Failed to extract context for segment {}: {}",
                    segment_boundary.index, e
                );
            }
        }
    }

    info!(
        "Import complete: {} segments, {} events, ${:.4} estimated cost",
        stats.segments_processed, stats.events_imported, stats.estimated_cost
    );

    Ok(stats)
}
*/

/*
/// Extract context from parsed segment and insert into database
async fn extract_and_insert_context(
    db: &DatabaseManager,
    session_id: &str,
    segment_index: usize,
    parsed: &ParsedSegment,
    boundary: &SegmentBoundary,
) -> Result<()> {
    use super::types::{AgentTask, ContextSummary};

    debug!(
        "Extracting context for session {} segment {}",
        session_id, segment_index
    );

    // Convert parsed agents to agent tasks
    let agent_tasks: Vec<AgentTask> = parsed
        .agents
        .iter()
        .map(|agent| {
            let outcome = if let Some(success) = agent.success {
                if success {
                    Some("Success".to_string())
                } else if let Some(ref err) = agent.error_message {
                    Some(format!("Error: {}", err))
                } else {
                    Some("Failed".to_string())
                }
            } else {
                None
            };

            AgentTask {
                agent_id: agent.agent_id.clone(),
                prompt: agent.prompt.clone(),
                timestamp: chrono::DateTime::from_timestamp(agent.spawn_timestamp, 0)
                    .unwrap_or_else(chrono::Utc::now),
                slug: agent.agent_slug.clone(),
                outcome,
            }
        })
        .collect();

    // Extract file paths from file changes
    let files_modified: Vec<String> = parsed
        .files
        .iter()
        .map(|file| file.file_path.clone())
        .collect();

    // Create compact summary based on segment stats and boundary
    let compact_summary = if boundary.trigger == "no_boundaries" {
        Some(format!(
            "Session segment without compact boundaries. {} messages processed.",
            parsed.stats.message_count
        ))
    } else {
        Some(format!(
            "Compacted at {} tokens (trigger: {}). {} messages, {} agents.",
            boundary.pre_tokens,
            boundary.trigger,
            parsed.stats.message_count,
            parsed.agents.len()
        ))
    };

    // Create context summary
    let summary = ContextSummary {
        id: None,
        session_id: session_id.to_string(),
        segment_index: segment_index as usize,
        compact_summary,
        agent_tasks,
        decisions: Vec::new(), // TODO: Extract from user/assistant pairs
        files_modified,
        start_timestamp: boundary.start_timestamp.timestamp(),
        end_timestamp: boundary.end_timestamp.timestamp(),
        pre_tokens: Some(boundary.pre_tokens),
        cwd: boundary.cwd.clone(),
        git_branch: boundary.git_branch.clone(),
    };

    // Insert into database
    db.insert_context_summary(&summary).await?;

    debug!(
        "Context extracted: {} agents, {} files",
        summary.agent_tasks.len(),
        summary.files_modified.len()
    );

    Ok(())
}
*/

/*
/// Import session incrementally (only new segments)
///
/// # Arguments
/// * `db` - Database manager
/// * `path` - Path to the JSONL file
/// * `last_segment` - Last processed segment index (None = start from beginning)
///
/// # Returns
/// Import statistics
pub async fn import_session_incremental(
    db: &DatabaseManager,
    path: &Path,
    last_segment: Option<usize>,
) -> Result<ImportStats> {
    info!(
        "Incremental import from: {} (after segment {:?})",
        path.display(),
        last_segment
    );

    // Discover all segments
    let segments = discover_segments(path)?;

    // Filter to only new segments
    let new_segments: Vec<_> = segments
        .into_iter()
        .filter(|s| {
            if let Some(last) = last_segment {
                s.index > last
            } else {
                true
            }
        })
        .collect();

    if new_segments.is_empty() {
        info!("No new segments to import");
        return Ok(ImportStats::default());
    }

    // Extract session ID
    let session_id = extract_session_id_from_uuid(&new_segments[0].start_uuid);

    // Process new segments
    let parser = SegmentParser::new();
    let mut stats = ImportStats {
        session_id: session_id.clone(),
        ..Default::default()
    };

    for segment_boundary in &new_segments {
        let parsed = parser
            .parse_segment(path, segment_boundary, &session_id)
            .await?;

        stats.segments_processed += 1;
        stats.events_imported += parsed.stats.message_count as usize;
        stats.agents_tracked += parsed.agents.len();
        stats.tools_tracked += parsed.tools.len();
        stats.files_tracked += parsed.files.len();
        stats.total_tokens +=
            parsed.stats.total_input_tokens as i64 + parsed.stats.total_output_tokens as i64;
        stats.estimated_cost += parsed.stats.estimated_cost_usd;

        // TODO: Insert into database
    }

    info!(
        "Incremental import complete: {} new segments",
        stats.segments_processed
    );

    Ok(stats)
}
*/

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract session ID from UUID (first part before first hyphen)
#[allow(dead_code)]
fn extract_session_id_from_uuid(uuid: &str) -> String {
    uuid.split('-').next().unwrap_or(uuid).to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_extract_session_id() {
        let uuid = "4e0b5d3d-c6d1-497d-9c6f-96e83980c7a0";
        let session_id = extract_session_id_from_uuid(uuid);
        assert_eq!(session_id, "4e0b5d3d");
    }

    #[test]
    fn test_discover_segments_no_boundaries() {
        // Create a test file with no compact boundaries
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"test-uuid","timestamp":"2026-01-24T12:00:00Z","parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/test","sessionId":"test","version":"2.1.19","gitBranch":"main","slug":"test-slug","message":{{"role":"user","content":[{{"type":"text","text":"hello"}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let segments = discover_segments(file.path()).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].trigger, "no_boundaries");
    }

    #[tokio::test]
    async fn test_parse_segment() {
        // This test requires a real JSONL file with proper events
        // For now, it's a placeholder
    }

    // Database integration tests commented out
    /*
    #[tokio::test]
    async fn test_import_session_file() {
        // This test requires a real JSONL file and database
        // For now, it's a placeholder
    }
    */
}
