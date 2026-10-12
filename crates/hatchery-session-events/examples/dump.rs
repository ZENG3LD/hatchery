//! Replays a recorded PTY capture (jsonl) and prints the final screen: `cargo run --example dump <file>`.
use std::io::BufRead;
fn main() {
    let path = std::env::args().nth(1).expect("capture file");
    let mut lines = std::io::BufReader::new(std::fs::File::open(path).unwrap()).lines();
    let meta: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let mut p = vt100::Parser::new(meta["rows"].as_u64().unwrap() as u16, meta["cols"].as_u64().unwrap() as u16, 0);
    let mut n = 0;
    for l in lines {
        let v: serde_json::Value = serde_json::from_str(&l.unwrap()).unwrap();
        if let Some(o) = v["out"].as_str() {
            use std::io::Write;
            let bytes = b64(o);
            p.process(&bytes);
            n += 1;
            if std::env::var("EACH").is_ok() { println!("--- t={} ---\n{}", v["t"], p.screen().contents()); }
            std::io::stdout().flush().ok();
        }
    }
    println!("=== final after {n} chunks ===\n{}", p.screen().contents());
}
fn b64(s: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new(); let mut acc = 0u32; let mut bits = 0;
    for c in s.bytes().filter(|c| *c != b'=') { acc = (acc << 6) | T.iter().position(|t| *t == c).unwrap() as u32; bits += 6; if bits >= 8 { bits -= 8; out.push((acc >> bits) as u8); acc &= (1 << bits) - 1; } }
    out
}
