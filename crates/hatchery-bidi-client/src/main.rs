//! HQ entry. Dials C2, writes one PNG, sends one click. No node address.

use std::path::PathBuf;

use hatchery_bidi_client::{encode_click, pull_frame_and_click, MouseButton};

#[tokio::main]
async fn main() {
    let mut c2 = None;
    let mut session = None;
    let mut out = None;
    let mut click = (120.0_f64, 90.0_f64, MouseButton::Left);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--c2" => c2 = Some(need("--c2", args.next())),
            "--session" => session = Some(need("--session", args.next())),
            "--out" => out = Some(PathBuf::from(need("--out", args.next()))),
            "--click" => {
                click = parse_click(&need("--click", args.next()));
            }
            "--help" | "-h" => {
                eprintln!(
                    "hatchery-bidi --c2 http://HOST:PORT --session ID --out FILE [--click X,Y,left|middle|right]"
                );
                return;
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    let c2 = c2.unwrap_or_else(|| fail("--c2 is required"));
    let session = session.unwrap_or_else(|| fail("--session is required"));
    let out = out.unwrap_or_else(|| fail("--out is required"));
    let (x, y, button) = click;
    let png = pull_frame_and_click(&c2, &session, x, y, button)
        .await
        .unwrap_or_else(|err| fail(&err.to_string()));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|err| fail(&err.to_string()));
    }
    std::fs::write(&out, &png).unwrap_or_else(|err| fail(&err.to_string()));
    let wire = encode_click(x, y, button);
    eprintln!(
        "[hatchery-bidi] wrote {} bytes={} click_bytes={}",
        out.display(),
        png.len(),
        wire.len()
    );
}

fn parse_click(raw: &str) -> (f64, f64, MouseButton) {
    let mut parts = raw.split(',');
    let x: f64 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| fail("--click needs x,y,button"));
    let y: f64 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| fail("--click needs x,y,button"));
    let button = match parts.next().unwrap_or("left") {
        "left" => MouseButton::Left,
        "middle" => MouseButton::Middle,
        "right" => MouseButton::Right,
        other => fail(&format!("unknown button {other}")),
    };
    (x, y, button)
}

fn need(flag: &str, value: Option<String>) -> String {
    value.unwrap_or_else(|| fail(&format!("{flag} requires a value")))
}

fn fail(message: &str) -> ! {
    eprintln!("hatchery-bidi: {message}");
    std::process::exit(2);
}
