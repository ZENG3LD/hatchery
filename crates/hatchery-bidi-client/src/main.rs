//! HQ entry. Dials C2 only. No node address.

use std::path::PathBuf;

use hatchery_bidi_client::{encode_click, pull_frame_and_click, pull_login, MouseButton};

#[tokio::main]
async fn main() {
    let mut c2 = None;
    let mut session = None;
    let mut out = None;
    let mut before = None;
    let mut after = None;
    let mut clock = None;
    let mut min_frames = 5usize;
    let mut clicks: Vec<(f64, f64, MouseButton)> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--c2" => c2 = Some(need("--c2", args.next())),
            "--session" => session = Some(need("--session", args.next())),
            "--out" => out = Some(PathBuf::from(need("--out", args.next()))),
            "--before" => before = Some(PathBuf::from(need("--before", args.next()))),
            "--after" => after = Some(PathBuf::from(need("--after", args.next()))),
            "--clock" => clock = Some(PathBuf::from(need("--clock", args.next()))),
            "--min-frames" => {
                min_frames = need("--min-frames", args.next())
                    .parse()
                    .unwrap_or_else(|_| fail("--min-frames is not an integer"));
            }
            "--click" => clicks.push(parse_click(&need("--click", args.next()))),
            "--help" | "-h" => {
                eprintln!(
                    "hatchery-bidi --c2 http://HOST:PORT --session ID --out FILE [--click X,Y,left]"
                );
                return;
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    let c2 = c2.unwrap_or_else(|| fail("--c2 is required"));
    let session = session.unwrap_or_else(|| fail("--session is required"));
    if before.is_some() || after.is_some() {
        let before = before.unwrap_or_else(|| fail("--before is required"));
        let after = after.unwrap_or_else(|| fail("--after is required"));
        let clock = clock.unwrap_or_else(|| fail("--clock is required"));
        if clicks.is_empty() {
            fail("--click is required");
        }
        let (png_before, png_after) = pull_login(&c2, &session, &clicks, min_frames, &clock)
            .await
            .unwrap_or_else(|err| fail(&err.to_string()));
        write_png(&before, &png_before);
        write_png(&after, &png_after);
        eprintln!(
            "[hatchery-bidi] wrote before={} bytes={} after={} bytes={} clicks={}",
            before.display(),
            png_before.len(),
            after.display(),
            png_after.len(),
            clicks.len()
        );
        return;
    }
    let out = out.unwrap_or_else(|| fail("--out is required"));
    let (x, y, button) = if clicks.is_empty() {
        (120.0, 90.0, MouseButton::Left)
    } else if clicks.len() == 1 {
        clicks[0]
    } else {
        fail("multiple --click values need --before and --after");
    };
    let png = pull_frame_and_click(&c2, &session, x, y, button)
        .await
        .unwrap_or_else(|err| fail(&err.to_string()));
    write_png(&out, &png);
    let wire = encode_click(x, y, button);
    eprintln!(
        "[hatchery-bidi] wrote {} bytes={} click_bytes={}",
        out.display(),
        png.len(),
        wire.len()
    );
}

fn write_png(path: &std::path::Path, png: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|err| fail(&err.to_string()));
    }
    std::fs::write(path, png).unwrap_or_else(|err| fail(&err.to_string()));
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
