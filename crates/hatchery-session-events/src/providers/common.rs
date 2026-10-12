use super::{Item, Parsed};
use crate::event::{ActionStatus, EventKind, PromptKind};

pub struct Profile {
    pub answer_markers: &'static [char],
    pub thought_markers: &'static [char],
    pub result_markers: &'static [char],
    pub tool_verbs: &'static [&'static str],
    pub working_needles: &'static [&'static str],
    pub banner_needles: &'static [&'static str],
    /// (phrase on screen, prompt kind, blocks the agent)
    pub prompts: &'static [(&'static str, PromptKind, bool)],
}

pub const GENERIC_WORKING: &[&str] = &["esc to interrupt", "ctrl+c to interrupt", "esc to cancel", "Waiting for approval"];
pub const GENERIC_PROMPTS: &[(&str, PromptKind, bool)] = &[
    ("(y/n)", PromptKind::Confirm, true),
    ("[y/n]", PromptKind::Confirm, true),
    ("do you want to proceed", PromptKind::Permission, true),
    ("do you trust", PromptKind::Trust, true),
    ("allow once", PromptKind::Permission, true),
    ("press enter to continue", PromptKind::Confirm, true),
];

fn strip_box(row: &str) -> (String, bool) {
    let t = row.trim_end();
    let boxed = t.trim_start().starts_with('│') || t.trim_start().starts_with('|');
    let mut s = t.to_owned();
    if boxed {
        s = s.trim_start().trim_start_matches(['│', '|']).to_owned();
        s = s.trim_end().trim_end_matches(['│', '|']).trim_end().to_owned();
    }
    (s, boxed)
}

fn is_border(row: &str) -> bool {
    let t = row.trim();
    !t.is_empty() && t.chars().all(|c| "╭╮╰╯─━═┌┐└┘├┤┬┴┼ ".contains(c)) && t.chars().any(|c| "╭╮╰╯─━═┌┐└┘".contains(c))
}

fn option_text(row: &str) -> Option<String> {
    let t = row.trim().trim_start_matches(['❯', '>', '●', '○', '*', '›', ' ']);
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 2 { return None; }
    let rest = t[digits.len()..].strip_prefix('.')?;
    Some(format!("{digits}. {}", rest.trim()))
}

fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn tool_call(text: &str) -> Option<(String, String)> {
    let open = text.find('(')?;
    let name = &text[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')) || !text.trim_end().ends_with(')') { return None; }
    Some((name.to_owned(), text[open + 1..text.trim_end().len() - 1].to_owned()))
}

pub fn scan(rows: &[String], p: &Profile) -> Parsed {
    let mut out = Parsed::default();
    let joined = rows.join("\n");
    let lower = joined.to_lowercase();

    // blocking / soft prompts: the phrase that appears highest on the screen wins (its menu starts there)
    let hit = p
        .prompts
        .iter()
        .chain(GENERIC_PROMPTS.iter())
        .filter_map(|(phrase, kind, blocking)| rows.iter().position(|r| r.to_lowercase().contains(&phrase.to_lowercase())).map(|at| (at, *kind, *blocking)))
        .min_by_key(|(at, _, _)| *at);
    if let Some((at, kind, blocking)) = hit {
        let text = strip_box(&rows[at]).0.trim().to_owned();
        let options: Vec<String> = rows.iter().skip(at).take(14).filter_map(|r| option_text(&strip_box(r).0)).collect();
        out.items.push(Item { key: format!("prompt:{kind:?}"), kind: EventKind::PromptNeeded { prompt: kind, text, options } });
        if blocking {
            out.needs_input = true;
            out.prompt_hint = format!("{kind:?}").to_lowercase();
        }
    }
    // working marker
    for needle in p.working_needles.iter().chain(GENERIC_WORKING.iter()) {
        if let Some(r) = rows.iter().find(|r| r.contains(needle)) {
            out.working = Some(short(r.trim().trim_start_matches(|c: char| !c.is_alphanumeric()), 80));
            break;
        }
    }
    if out.working.is_none() {
        // spinner glyph (braille or star) followed by a verb
        for r in rows {
            let t = strip_box(r).0;
            let t = t.trim();
            let mut ch = t.chars();
            if let Some(c) = ch.next() {
                let rest: String = ch.collect();
                let spinner = ('\u{2800}'..='\u{28ff}').contains(&c) || "✻✢✳✶✽·".contains(c);
                if spinner && rest.trim_start().chars().next().map(|x| x.is_alphabetic()).unwrap_or(false) && rest.contains('…') {
                    out.working = Some(short(rest.trim(), 80));
                    break;
                }
            }
        }
    }

    let mut cur: Option<usize> = None; // index in out.items of the open answer/thought
    let mut last_action: Option<usize> = None;
    let mut diff: Option<usize> = None;
    for raw in rows {
        if is_border(raw) { cur = None; continue; }
        let (line, boxed) = strip_box(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() { cur = None; diff = None; continue; }
        // diffs
        let starts_diff = trimmed.starts_with("diff --git") || trimmed.starts_with("@@ ");
        let diff_body = diff.is_some() && (line.starts_with(' ') || line.starts_with('+') || line.starts_with('-') || line.starts_with('@') || line.starts_with("index ") );
        if starts_diff || diff_body {
            let idx = match diff {
                Some(i) => i,
                None => {
                    out.items.push(Item { key: String::new(), kind: EventKind::Diff { files: vec![], patch: String::new() } });
                    let i = out.items.len() - 1;
                    diff = Some(i);
                    i
                }
            };
            if let EventKind::Diff { files, patch } = &mut out.items[idx].kind {
                if let Some(rest) = trimmed.strip_prefix("diff --git a/") {
                    if let Some(f) = rest.split(" b/").next() { files.push(f.to_owned()); }
                }
                if !patch.is_empty() { patch.push('\n'); }
                patch.push_str(&line);
            }
            cur = None;
            continue;
        }
        diff = None;
        let first = trimmed.chars().next().unwrap();
        let after = trimmed[first.len_utf8()..].trim_start();
        // tool results
        if p.result_markers.contains(&first) {
            if let Some(i) = last_action {
                if let EventKind::Action { result, status, .. } = &mut out.items[i].kind {
                    let joined = match result.take() { Some(r) => format!("{r} {after}"), None => after.to_owned() };
                    let l = joined.to_lowercase();
                    *status = if l.contains("error") || l.contains("failed") || l.contains("refused") { ActionStatus::Failed } else { ActionStatus::Done };
                    *result = Some(joined);
                }
            }
            cur = None;
            continue;
        }
        if boxed && (trimmed.starts_with('>') || trimmed.starts_with('❯')) { cur = None; continue; } // input field
        if p.thought_markers.contains(&first) {
            let t = after.trim_start_matches("Thinking").trim_start_matches(['…', ':', '.', ' ']).to_owned();
            let t = if t.is_empty() { "Thinking…".to_owned() } else { t };
            out.items.push(Item { key: String::new(), kind: EventKind::Thought { text: t } });
            cur = Some(out.items.len() - 1);
            continue;
        }
        if p.answer_markers.contains(&first) && !after.is_empty() {
            let verb = after.split_whitespace().next().unwrap_or("");
            if let Some((name, args)) = tool_call(after) {
                out.items.push(Item { key: String::new(), kind: EventKind::Action { name, args, result: None, status: ActionStatus::Running } });
                last_action = Some(out.items.len() - 1);
                cur = None;
            } else if p.tool_verbs.contains(&verb) {
                out.items.push(Item { key: String::new(), kind: EventKind::Action { name: verb.to_owned(), args: after[verb.len()..].trim().to_owned(), result: None, status: ActionStatus::Running } });
                last_action = Some(out.items.len() - 1);
                cur = None;
            } else if p.banner_needles.iter().any(|b| after.contains(b)) {
                out.items.push(Item { key: String::new(), kind: EventKind::Banner { text: after.to_owned() } });
                cur = None;
            } else {
                out.items.push(Item { key: String::new(), kind: EventKind::Answer { text: after.to_owned() } });
                cur = Some(out.items.len() - 1);
            }
            continue;
        }
        if p.banner_needles.iter().any(|b| trimmed.contains(b)) && !boxed {
            out.items.push(Item { key: String::new(), kind: EventKind::Banner { text: trimmed.to_owned() } });
            cur = None;
            continue;
        }
        if (trimmed.starts_with("> ") || trimmed.starts_with("❯ ")) && !boxed && option_text(trimmed).is_none() && !out.needs_input && trimmed.len() > 2 {
            out.items.push(Item { key: String::new(), kind: EventKind::UserMessage { text: trimmed[first.len_utf8()..].trim().to_owned() } });
            cur = None;
            continue;
        }
        // continuation of an open answer/thought: indented text
        if let (Some(i), true) = (cur, line.starts_with("  ")) {
            match &mut out.items[i].kind {
                EventKind::Answer { text } | EventKind::Thought { text } => { text.push(' '); text.push_str(trimmed); }
                _ => {}
            }
            continue;
        }
        cur = None;
    }
    // stable keys: kind + prefix of the text; identical prefixes get an ordinal
    let mut seen: std::collections::HashMap<String, u32> = Default::default();
    for it in &mut out.items {
        if it.key.is_empty() {
            let (tag, text) = match &it.kind {
                EventKind::Answer { text } => ("answer", text.clone()),
                EventKind::Thought { text } => ("thought", text.clone()),
                EventKind::UserMessage { text } => ("user", text.clone()),
                EventKind::Banner { text } => ("banner", text.clone()),
                EventKind::Action { name, args, .. } => ("action", format!("{name}({args}")),
                EventKind::Diff { files, patch } => ("diff", format!("{}{}", files.join(","), short(patch, 40))),
                _ => ("other", String::new()),
            };
            let base = format!("{tag}:{}", short(&text, 24));
            let n = seen.entry(base.clone()).or_insert(0);
            it.key = if *n == 0 { base } else { format!("{base}#{n}") };
            *n += 1;
        }
    }
    out
}
