//! C2 forwarder. Not a browser: it never launches Chrome and never reads
//! click fields. stdout and --log receive opaque byte counts only.

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use hatchery_bidi_relay::{bind, serve, RelayLog};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut listen: SocketAddr = "127.0.0.1:18443"
        .parse()
        .expect("built-in listen address");
    let mut log_path = None;
    let mut limits = hatchery_bidi_relay::RelayLimits::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => {
                let value = need("--listen", args.next());
                listen = value
                    .parse()
                    .unwrap_or_else(|err| fail(&format!("--listen is invalid: {err}")));
            }
            "--log" => log_path = Some(need("--log", args.next())),
            "--max-pending-frames" => {
                limits.max_pending_frames = need("--max-pending-frames", args.next())
                    .parse()
                    .unwrap_or_else(|err| fail(&format!("--max-pending-frames is invalid: {err}")));
            }
            "--max-pending-bytes" => {
                limits.max_pending_bytes = need("--max-pending-bytes", args.next())
                    .parse()
                    .unwrap_or_else(|err| fail(&format!("--max-pending-bytes is invalid: {err}")));
            }
            "--help" | "-h" => {
                eprintln!("hatchery-bidi-relay --listen IP:PORT [--log FILE] [--max-pending-frames N] [--max-pending-bytes N]");
                return;
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }

    let file = match log_path {
        Some(path) => Some(Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap_or_else(|err| fail(&format!("open {path}: {err}"))),
        ))),
        None => None,
    };
    let log = RelayLog::new(move |line| {
        eprintln!("{line}");
        if let Some(file) = &file {
            if let Ok(mut file) = file.lock() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
    })
    .with_limits(limits);

    let (listener, local) = bind(listen)
        .await
        .unwrap_or_else(|err| fail(&format!("bind {listen}: {err}")));
    eprintln!("bidi-relay listening {local}");
    if let Err(err) = serve(listener, log).await {
        fail(&err.to_string());
    }
}

fn need(flag: &str, value: Option<String>) -> String {
    value.unwrap_or_else(|| fail(&format!("{flag} requires a value")))
}

fn fail(message: &str) -> ! {
    eprintln!("hatchery-bidi-relay: {message}");
    std::process::exit(2);
}
