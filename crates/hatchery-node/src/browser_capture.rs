//! Linux dig2browser capture inside the node (feature `d2b`).
//!
//! Owns a headless Chromium via dig2browser, captures viewport frames (JPEG via
//! CDP `Page.captureScreenshot`, PNG fallback), and applies dig2-wire input.
//! Transport is the restream G4aControl sidecar — this module never dials HQ.

#![cfg(feature = "d2b")]

use dig2browser::browser::{
    BrowserPreference, BrowserProfile, LaunchConfig, StealthBrowser, StealthConfig, StealthPage,
};
use dig2browser::detect::args::RendererSandboxMode;
use hatchery_core::{
    decode_browser_input, BrowserInput, BrowserMouseButton, BrowserStreamEvent,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

const DEFAULT_URL: &str = "about:blank";
const CAPTURE_EVERY: Duration = Duration::from_millis(250);
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 720;

pub struct BrowserSessionHandle {
    pub session: String,
    input_tx: mpsc::UnboundedSender<BrowserInput>,
    stop_tx: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl BrowserSessionHandle {
    pub fn push_input(&self, input: BrowserInput) {
        let _ = self.input_tx.send(input);
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

/// Registry of live browser sessions on this node.
#[derive(Default)]
pub struct BrowserSessionMap {
    inner: Mutex<std::collections::BTreeMap<String, BrowserSessionHandle>>,
}

impl BrowserSessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(
        &self,
        node_id: impl Into<String>,
        session: String,
        url: String,
        profile_dir: PathBuf,
        event_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
    ) -> Result<(), String> {
        hatchery_core::validate_browser_session_id(&session)?;
        let node_id = node_id.into();
        let mut guard = self.inner.lock().await;
        if guard.contains_key(&session) {
            return Err("browser session already running".into());
        }
        let handle = spawn_session(
            node_id,
            session.clone(),
            if url.trim().is_empty() {
                DEFAULT_URL.to_owned()
            } else {
                url
            },
            profile_dir,
            event_tx,
        )?;
        guard.insert(session, handle);
        Ok(())
    }

    pub async fn input(&self, session: &str, bytes: &[u8]) -> Result<(), String> {
        let input = decode_browser_input(bytes)?;
        let guard = self.inner.lock().await;
        let Some(handle) = guard.get(session) else {
            return Err("browser session not found".into());
        };
        handle.push_input(input);
        Ok(())
    }

    pub async fn stop(&self, session: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        let Some(handle) = guard.remove(session) else {
            return Err("browser session not found".into());
        };
        drop(guard);
        handle.stop().await;
        Ok(())
    }

    pub async fn stop_all(&self) {
        let mut guard = self.inner.lock().await;
        let handles: Vec<_> = std::mem::take(&mut *guard).into_values().collect();
        drop(guard);
        for h in handles {
            h.stop().await;
        }
    }
}

fn spawn_session(
    node_id: String,
    session: String,
    start_url: String,
    profile_dir: PathBuf,
    event_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
) -> Result<BrowserSessionHandle, String> {
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = oneshot::channel();
    let session_for_task = session.clone();
    let join = tokio::spawn(async move {
        if let Err(err) = run_session(
            node_id.clone(),
            session_for_task.clone(),
            start_url,
            profile_dir,
            input_rx,
            stop_rx,
            event_tx.clone(),
        )
        .await
        {
            let _ = event_tx.send(BrowserStreamEvent::Error {
                node_id: node_id.clone(),
                session: session_for_task.clone(),
                message: err,
            });
        }
        let _ = event_tx.send(BrowserStreamEvent::Stopped {
            node_id,
            session: session_for_task,
            reason: "closed".into(),
        });
    });
    Ok(BrowserSessionHandle {
        session,
        input_tx,
        stop_tx: Some(stop_tx),
        join: Some(join),
    })
}

async fn run_session(
    node_id: String,
    session: String,
    start_url: String,
    profile_dir: PathBuf,
    mut input_rx: mpsc::UnboundedReceiver<BrowserInput>,
    mut stop_rx: oneshot::Receiver<()>,
    event_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
) -> Result<(), String> {
    std::fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;
    let launch = LaunchConfig {
        headless: true,
        window_size: (VIEW_W, VIEW_H),
        profile: BrowserProfile::Persistent(profile_dir),
        browser_pref: BrowserPreference::ChromeOnly,
        renderer_sandbox: RendererSandboxMode::CompatibilityDisabled,
        extra_args: vec![
            "--force-device-scale-factor=1".into(),
            "--disable-dev-shm-usage".into(),
        ],
        ..LaunchConfig::default()
    };
    let browser = StealthBrowser::launch_with(launch, StealthConfig::default())
        .await
        .map_err(|e| e.to_string())?;
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| e.to_string())?;
    // Best-effort viewport.
    let _ = page.set_viewport(VIEW_W, VIEW_H, 1.0).await;
    if let Err(err) = tokio::time::timeout(Duration::from_secs(40), page.goto(&start_url)).await {
        tracing::warn!(%session, error = %err, "browser start navigation timed out");
    }
    let _ = event_tx.send(BrowserStreamEvent::Started {
        node_id: node_id.clone(),
        session: session.clone(),
        url: start_url.clone(),
    });

    let mut tick = tokio::time::interval(CAPTURE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_hash: u64 = 0;
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            _ = tick.tick() => {
                if let Ok((bytes, w, h, url, title)) = capture_frame(&page).await {
                    let hash = {
                        use std::hash::{Hash, Hasher};
                        let mut hsh = std::collections::hash_map::DefaultHasher::new();
                        bytes.hash(&mut hsh);
                        hsh.finish()
                    };
                    if hash == last_hash { continue; }
                    last_hash = hash;
                    let _ = event_tx.send(BrowserStreamEvent::Frame {
                        node_id: node_id.clone(),
                        session: session.clone(),
                        bytes,
                        width: w,
                        height: h,
                        url,
                        title,
                    });
                }
            }
            input = input_rx.recv() => {
                let Some(input) = input else { break };
                if let Err(err) = apply_input(&page, &input).await {
                    tracing::warn!(%session, error = %err, "browser input failed");
                } else {
                    // Force a fresh frame after input so nav/click show up promptly.
                    last_hash = 0;
                }
            }
        }
    }
    let _ = browser.close().await;
    Ok(())
}

async fn capture_frame(page: &StealthPage) -> Result<(Vec<u8>, u32, u32, String, String), String> {
    // Prefer JPEG via CDP (matches the owner's screencast brief); fall back to PNG screenshot.
    let jpeg = page
        .cdp_call(
            "Page.captureScreenshot",
            Some(json!({ "format": "jpeg", "quality": 55 })),
        )
        .await;
    let bytes = match jpeg {
        Ok(v) => {
            let b64 = v.get("data").and_then(|x| x.as_str()).ok_or("no jpeg data")?;
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| e.to_string())?
        }
        Err(_) => page.screenshot().await.map_err(|e| e.to_string())?,
    };
    let url = page
        .eval("location.href")
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let title = page
        .eval("document.title")
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    Ok((bytes, VIEW_W, VIEW_H, url, title))
}

async fn apply_input(page: &StealthPage, input: &BrowserInput) -> Result<(), String> {
    match input {
        BrowserInput::Click { x, y, button } => match button {
            BrowserMouseButton::Left => page.click_at(*x, *y).await.map_err(|e| e.to_string())?,
            BrowserMouseButton::Right => {
                page.right_click_at(*x, *y).await.map_err(|e| e.to_string())?
            }
            BrowserMouseButton::Middle => {
                // dig2browser has no middle-click helper; synthesize via CDP.
                page.cdp_call(
                    "Input.dispatchMouseEvent",
                    Some(json!({
                        "type": "mousePressed", "x": x, "y": y,
                        "button": "middle", "clickCount": 1
                    })),
                )
                .await
                .map_err(|e| e.to_string())?;
                page.cdp_call(
                    "Input.dispatchMouseEvent",
                    Some(json!({
                        "type": "mouseReleased", "x": x, "y": y,
                        "button": "middle", "clickCount": 1
                    })),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        },
        BrowserInput::Scroll { x, y, dx, dy } => {
            page.wheel(*x, *y, *dx, *dy)
                .await
                .map_err(|e| e.to_string())?
        }
        BrowserInput::Key { key } => page.key_press(key).await.map_err(|e| e.to_string())?,
        BrowserInput::Text { text } => {
            // Prefer CDP insertText; fall back to per-character key_press.
            match page
                .cdp_call("Input.insertText", Some(json!({ "text": text })))
                .await
            {
                Ok(_) => {}
                Err(_) => {
                    for ch in text.chars() {
                        page.key_press(&ch.to_string())
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        BrowserInput::Navigate { url } => {
            match tokio::time::timeout(Duration::from_secs(40), page.goto(url)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => return Err("navigate timed out".into()),
            }
        }
    }
    Ok(())
}

/// Shared map for the restream bridge (process-global for the node binary).
pub fn shared_map() -> Arc<BrowserSessionMap> {
    use std::sync::OnceLock;
    static MAP: OnceLock<Arc<BrowserSessionMap>> = OnceLock::new();
    MAP.get_or_init(|| Arc::new(BrowserSessionMap::new())).clone()
}
