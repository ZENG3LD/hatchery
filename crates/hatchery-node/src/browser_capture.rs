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
    decode_browser_input, BrowserFrameRegion, BrowserFrameTrace, BrowserInput, BrowserMouseButton, BrowserStreamEvent,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

const DEFAULT_URL: &str = "about:blank";
const CAPTURE_EVERY: Duration = Duration::from_millis(250);
const META_EVERY: Duration = Duration::from_millis(1000);
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
            // Re-open from HQ (panel reopened / reload): reuse the running browser.
            let _ = event_tx.send(BrowserStreamEvent::Started { node_id, session: session.clone(), url });
            if let Some(h) = guard.get(&session) { h.push_input(BrowserInput::Navigate { url: String::new() }); }
            return Ok(());
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
        // Headful on Xvfb when a DISPLAY is present (less bot-detection); else headless.
        headless: std::env::var("HATCHERY_D2B_HEADFUL").map(|v| v != "1").unwrap_or(true)
            || std::env::var_os("DISPLAY").is_none(),
        window_size: (VIEW_W, VIEW_H),
        profile: BrowserProfile::Persistent(profile_dir.clone()),
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
    let _ = page.set_viewport(VIEW_W, VIEW_H, 1.0).await;
    // Headful: keep our page in front (a background tab never paints).
    let _ = page.cdp_call("Page.bringToFront", None).await;
    let _ = page
        .cdp_call("Emulation.setFocusEmulationEnabled", Some(json!({ "enabled": true })))
        .await;
    let _ = page.cdp_call("Page.navigate", Some(json!({ "url": start_url }))).await;
    let _ = event_tx.send(BrowserStreamEvent::Started {
        node_id: node_id.clone(),
        session: session.clone(),
        url: start_url.clone(),
    });

    // True screencast over a side CDP connection to our page target (push frames,
    // ack each); falls back to polling captureScreenshot if it can't be set up.
    let (sc_tx, mut sc_rx) = mpsc::channel::<Vec<u8>>(4);
    let screencast = match screencast::start(&profile_dir, &page, sc_tx).await {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::warn!(%session, error = %e, "d2b screencast unavailable; polling");
            None
        }
    };

    let mut tick = tokio::time::interval(if screencast.is_some() { META_EVERY } else { CAPTURE_EVERY });
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_hash: u64 = 0;
    let (mut url, mut title) = (start_url.clone(), String::new());
    let mut input_n: u64 = 0;
    // (input_n, received, cdp_ms, dispatched)
    let mut pending: Option<(u64, std::time::Instant, u32, std::time::Instant, u32)> = None;
    let mut last_frame: Option<Vec<u8>> = None;
    let mut delta = delta::Delta::default();
    loop {
        let frame: Option<(Vec<u8>, &'static str)> = tokio::select! {
            _ = &mut stop_rx => break,
            f = sc_rx.recv(), if screencast.is_some() => f.map(|b| (b, "screencast")),
            _ = tick.tick() => {
                if screencast.is_some() {
                    // Screencast pushes pixels; the tick only refreshes url/title.
                    match tokio::time::timeout(Duration::from_secs(3), page_meta(&page)).await {
                        Ok(Ok((u, ti))) if u != url || ti != title => {
                            url = u; title = ti;
                            last_frame.clone().map(|b| (b, "meta"))
                        }
                        _ => None,
                    }
                } else {
                    if let Ok(Ok((u, ti))) = tokio::time::timeout(Duration::from_secs(3), page_meta(&page)).await { url = u; title = ti; }
                    match tokio::time::timeout(Duration::from_secs(5), capture_frame(&page)).await {
                        Ok(Ok(bytes)) => {
                            let hash = { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); bytes.hash(&mut h); h.finish() };
                            if hash != last_hash { last_hash = hash; Some((bytes, "poll")) } else { None }
                        }
                        _ => None,
                    }
                }
            }
            input = input_rx.recv() => {
                let Some(input) = input else { break };
                let received = std::time::Instant::now();
                input_n += 1;
                let queue_ms = 0u32;
                if matches!(&input, BrowserInput::Navigate { url } if url.is_empty()) {
                    // Re-open: resend the last frame so a new viewer paints at once.
                    last_frame.clone().map(|b| (b, "replay"))
                } else {
                    let t0 = std::time::Instant::now();
                    match apply_input(&page, &input).await {
                        Err(err) => { tracing::warn!(%session, error = %err, "browser input failed"); None }
                        Ok(()) => {
                            let cdp_ms = t0.elapsed().as_millis() as u32;
                            pending = Some((input_n, received, cdp_ms, std::time::Instant::now(), queue_ms));
                            if screencast.is_none() {
                                last_hash = 0;
                                tick.reset_immediately();
                            }
                            None
                        }
                    }
                }
            }
        };
        let Some((bytes, source)) = frame else { continue };
        // Screencast frames become deltas (changed rectangle only) when small enough:
        // a few KB fit one TCP window on every long hop instead of several RTTs.
        let (bytes, region) = if source == "screencast" {
            match delta.encode(&bytes) {
                delta::Out::Unchanged => continue,
                delta::Out::Full => (bytes, None),
                delta::Out::Region(b, r) => (b, Some(r)),
            }
        } else if source == "replay" || source == "meta" {
            (delta.full_jpeg().unwrap_or(bytes), None)
        } else {
            (bytes, None)
        };
        if region.is_none() && (source == "screencast" || source == "poll") {
            last_frame = Some(bytes.clone());
        }
        let trace = if source == "screencast" || source == "poll" {
            pending.take().map(|(n, received, cdp_ms, dispatched, queue_ms)| BrowserFrameTrace {
                input_n: n,
                queue_ms,
                cdp_ms,
                to_frame_ms: dispatched.elapsed().as_millis() as u32,
                node_ms: received.elapsed().as_millis() as u32,
                source: if region.is_some() { "screencast-delta".to_owned() } else { source.to_owned() },
            })
        } else { None };
        let _ = event_tx.send(BrowserStreamEvent::Frame {
            node_id: node_id.clone(),
            session: session.clone(),
            bytes,
            width: VIEW_W,
            height: VIEW_H,
            url: url.clone(),
            title: title.clone(),
            trace,
            region,
        });
    }
    if let Some(h) = screencast { h.abort(); }
    let _ = browser.close().await;
    Ok(())
}

async fn page_meta(page: &StealthPage) -> Result<(String, String), String> {
    let v = page.eval("[location.href, document.title]").await.map_err(|e| e.to_string())?;
    let a = v.as_array().cloned().unwrap_or_default();
    Ok((
        a.first().and_then(|x| x.as_str()).unwrap_or_default().to_owned(),
        a.get(1).and_then(|x| x.as_str()).unwrap_or_default().to_owned(),
    ))
}

/// Delta frames: diff each decoded frame against the previous one in 32px tiles
/// and send only the changed rectangle when it is a small part of the screen.
mod delta {
    use super::BrowserFrameRegion;
    use image::{codecs::jpeg::JpegEncoder, ImageReader, RgbImage};

    const TILE: u32 = 32;
    #[derive(Default)]
    pub struct Delta { prev: Option<RgbImage> }
    pub enum Out { Unchanged, Full, Region(Vec<u8>, BrowserFrameRegion) }

    impl Delta {
        pub fn encode(&mut self, jpeg: &[u8]) -> Out {
            let Ok(img) = ImageReader::new(std::io::Cursor::new(jpeg)).with_guessed_format().map_err(|_| ()).and_then(|r| r.decode().map_err(|_| ())) else { return Out::Full };
            let cur = img.to_rgb8();
            let Some(prev) = self.prev.as_ref().filter(|p| p.dimensions() == cur.dimensions()) else {
                self.prev = Some(cur);
                return Out::Full;
            };
            let (w, h) = cur.dimensions();
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
            let (pr, cr) = (prev.as_raw(), cur.as_raw());
            for ty in (0..h).step_by(TILE as usize) {
                for tx in (0..w).step_by(TILE as usize) {
                    let tw = TILE.min(w - tx); let th = TILE.min(h - ty);
                    let mut changed = false;
                    for yy in ty..ty + th {
                        let a = ((yy * w + tx) * 3) as usize; let b = a + (tw * 3) as usize;
                        // JPEG noise tolerance: a tile changes when any byte moves > 24.
                        if pr[a..b].iter().zip(&cr[a..b]).any(|(p, c)| p.abs_diff(*c) > 24) { changed = true; break; }
                    }
                    if changed { x0 = x0.min(tx); y0 = y0.min(ty); x1 = x1.max(tx + tw); y1 = y1.max(ty + th); }
                }
            }
            if x0 == u32::MAX { return Out::Unchanged; }
            let (rw, rh) = (x1 - x0, y1 - y0);
            if (rw * rh) as u64 * 2 > (w * h) as u64 {
                self.prev = Some(cur);
                return Out::Full;
            }
            let crop = image::imageops::crop_imm(&cur, x0, y0, rw, rh).to_image();
            let mut out = Vec::new();
            if JpegEncoder::new_with_quality(&mut out, 70).encode_image(&crop).is_err() { self.prev = Some(cur); return Out::Full; }
            // Base the next diff on what the viewer now shows (prev + this region).
            let mut next = prev.clone();
            image::imageops::replace(&mut next, &crop, x0 as i64, y0 as i64);
            self.prev = Some(next);
            Out::Region(out, BrowserFrameRegion { x: x0, y: y0, w: rw, h: rh, src_w: w, src_h: h })
        }

        /// Current composite as a full JPEG (for a viewer that just attached).
        pub fn full_jpeg(&self) -> Option<Vec<u8>> {
            let img = self.prev.as_ref()?;
            let mut out = Vec::new();
            JpegEncoder::new_with_quality(&mut out, 60).encode_image(img).ok()?;
            Some(out)
        }
    }
}

/// Side CDP websocket to our page target for Page.startScreencast.
mod screencast {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    /// DevTools port of the Chrome that owns `profile_dir`: DevToolsActivePort if
    /// present, else the --remote-debugging-port of the process using that profile.
    fn devtools_port(profile_dir: &std::path::Path) -> Option<u16> {
        if let Ok(s) = std::fs::read_to_string(profile_dir.join("DevToolsActivePort")) {
            if let Some(p) = s.lines().next().and_then(|l| l.trim().parse().ok()) { return Some(p); }
        }
        let want = format!("--user-data-dir={}", profile_dir.display());
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else { continue };
            let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
            if !args.iter().any(|a| a == &want) { continue; }
            if let Some(p) = args.iter().find_map(|a| a.strip_prefix("--remote-debugging-port=").and_then(|v| v.parse().ok())) {
                return Some(p);
            }
        }
        None
    }

    /// Max JPEG bytes per frame so one frame fits a single restream message.
    const BUDGET: usize = 40 * 1024;

    pub async fn start(
        profile_dir: &std::path::Path,
        page: &StealthPage,
        out: mpsc::Sender<Vec<u8>>,
    ) -> Result<tokio::task::JoinHandle<()>, String> {
        let port = devtools_port(profile_dir).ok_or("devtools port not found")?;
        let info = page.cdp_call("Target.getTargetInfo", None).await.map_err(|e| e.to_string())?;
        let target = info["targetInfo"]["targetId"].as_str().ok_or("no target id")?.to_owned();
        let url = format!("ws://127.0.0.1:{port}/devtools/page/{target}");
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.map_err(|e| e.to_string())?;
        let (mut tx, mut rx) = ws.split();
        let mut quality: u32 = 60;
        let mut max_w: u32 = VIEW_W;
        let mut id: u64 = 1;
        let start_msg = |id: u64, q: u32, w: u32| json!({ "id": id, "method": "Page.startScreencast",
            "params": { "format": "jpeg", "quality": q, "maxWidth": w, "maxHeight": w * VIEW_H / VIEW_W, "everyNthFrame": 1 } }).to_string();
        tx.send(Message::text(json!({ "id": id, "method": "Page.enable" }).to_string())).await.map_err(|e| e.to_string())?;
        id += 1;
        tx.send(Message::text(start_msg(id, quality, max_w))).await.map_err(|e| e.to_string())?;
        Ok(tokio::spawn(async move {
            while let Some(Ok(msg)) = rx.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                if v["method"] != "Page.screencastFrame" { continue; }
                let sid = v["params"]["sessionId"].clone();
                // Ack first so Chrome can start producing the next frame immediately.
                id += 1;
                let _ = tx.send(Message::text(json!({ "id": id, "method": "Page.screencastFrameAck", "params": { "sessionId": sid } }).to_string())).await;
                let Some(b64) = v["params"]["data"].as_str() else { continue };
                use base64::Engine;
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else { continue };
                if bytes.len() > BUDGET {
                    // Too big for one restream message: step quality, then size, and restart.
                    if quality > 25 { quality -= 15 } else if max_w > 640 { max_w = max_w * 3 / 4 } else { continue }
                    id += 1;
                    let _ = tx.send(Message::text(json!({ "id": id, "method": "Page.stopScreencast" }).to_string())).await;
                    id += 1;
                    let _ = tx.send(Message::text(start_msg(id, quality, max_w))).await;
                    continue;
                }
                // Keep latency low: if the consumer is behind, drop rather than queue.
                let _ = out.try_send(bytes);
            }
        }))
    }
}

async fn capture_frame(page: &StealthPage) -> Result<Vec<u8>, String> {
    // JPEG via CDP, sized to fit one Noise transport message (64 KiB incl. envelope):
    // step quality down, then scale, until under FRAME_BUDGET. Oversized frames used to
    // kill the restream uplink ("noise: input error") on busy pages.
    const FRAME_BUDGET: usize = 40 * 1024;
    let mut bytes = Vec::new();
    'fit: for scale in [1.0f64, 0.75, 0.5] {
        for q in [60u32, 45, 32, 22, 14] {
            let mut params = json!({ "format": "jpeg", "quality": q });
            if scale < 1.0 {
                params["clip"] = json!({ "x": 0, "y": 0, "width": VIEW_W, "height": VIEW_H, "scale": scale });
            }
            let v = page.cdp_call("Page.captureScreenshot", Some(params)).await.map_err(|e| e.to_string())?;
            let b64 = v.get("data").and_then(|x| x.as_str()).ok_or("no jpeg data")?;
            use base64::Engine;
            bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| e.to_string())?;
            if bytes.len() <= FRAME_BUDGET {
                break 'fit;
            }
        }
    }
    if bytes.len() > FRAME_BUDGET {
        return Err(format!("frame too large ({} bytes)", bytes.len()));
    }
    Ok(bytes)
}

async fn apply_input(page: &StealthPage, input: &BrowserInput) -> Result<(), String> {
    match input {
        BrowserInput::Click { x, y, button } => {
            // Raw CDP, viewport CSS pixels (DPR 1): move, press, release — a real
            // trusted click that also reaches cross-origin iframes (e.g. Turnstile).
            let b = match button {
                BrowserMouseButton::Left => "left",
                BrowserMouseButton::Right => "right",
                BrowserMouseButton::Middle => "middle",
            };
            tracing::info!(x = *x, y = *y, button = b, "d2b click");
            page.cdp_call(
                "Input.dispatchMouseEvent",
                Some(json!({ "type": "mouseMoved", "x": x, "y": y, "button": "none" })),
            )
            .await
            .map_err(|e| e.to_string())?;
            for ty in ["mousePressed", "mouseReleased"] {
                page.cdp_call(
                    "Input.dispatchMouseEvent",
                    Some(json!({ "type": ty, "x": x, "y": y, "button": b, "buttons": if ty == "mousePressed" { 1 } else { 0 }, "clickCount": 1 })),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }
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
        BrowserInput::Navigate { url } if url.is_empty() => {}
        BrowserInput::Navigate { url } => {
            // Raw Page.navigate returns once the request commits; frames keep
            // streaming while the page loads (goto blocked the loop on load).
            tracing::info!(%url, "d2b navigate");
            let _ = page.cdp_call("Page.bringToFront", None).await;
            let r = tokio::time::timeout(
                Duration::from_secs(20),
                page.cdp_call("Page.navigate", Some(json!({ "url": url }))),
            )
            .await
            .map_err(|_| "navigate timed out".to_string())?
            .map_err(|e| e.to_string())?;
            if let Some(err) = r.get("errorText").and_then(|v| v.as_str()) {
                if !err.is_empty() {
                    return Err(format!("navigate: {err}"));
                }
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
