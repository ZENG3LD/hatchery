//! `hatchery-build-host`: run on the build machine.
//!   hatchery-build-host keygen KEY_FILE            create the signing key (0600), print the public key
//!   hatchery-build-host --listen ADDR --recipes FILE --signing-key FILE --c2-api ADDR
//! Secrets come from the environment only: HATCHERY_BUILD_TOKEN (shared with C2) and
//! HATCHERY_C2_TOKEN (operator token, used for the admission lookup).

use hatchery_build::{serve, BuildHost, C2Admission, Recipes, Signer};
use std::sync::Arc;

fn fail(message: &str) -> ! {
    eprintln!("hatchery-build-host: {message}");
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("keygen") {
        let path = args.get(1).unwrap_or_else(|| fail("keygen needs a key file path"));
        if std::path::Path::new(path).exists() {
            fail("key file already exists; refusing to overwrite");
        }
        let signer = Signer::generate_to_file(std::path::Path::new(path)).unwrap_or_else(|e| fail(&e));
        println!("{}", signer.public_key());
        return;
    }
    let (mut listen, mut recipes, mut key, mut c2) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = |n: &str| it.next().cloned().unwrap_or_else(|| fail(&format!("{n} needs a value")));
        match a.as_str() {
            "--listen" => listen = Some(v("--listen")),
            "--recipes" => recipes = Some(v("--recipes")),
            "--signing-key" => key = Some(v("--signing-key")),
            "--c2-api" => c2 = Some(v("--c2-api")),
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    let bearer = std::env::var("HATCHERY_BUILD_TOKEN").unwrap_or_else(|_| fail("HATCHERY_BUILD_TOKEN is required"));
    let c2_token = std::env::var("HATCHERY_C2_TOKEN").unwrap_or_else(|_| fail("HATCHERY_C2_TOKEN is required"));
    std::env::remove_var("HATCHERY_BUILD_TOKEN");
    std::env::remove_var("HATCHERY_C2_TOKEN");
    let listen: std::net::SocketAddr = listen.unwrap_or_else(|| fail("--listen is required")).parse().unwrap_or_else(|e| fail(&format!("--listen: {e}")));
    let api: std::net::SocketAddr = c2.unwrap_or_else(|| fail("--c2-api is required")).parse().unwrap_or_else(|e| fail(&format!("--c2-api: {e}")));
    let recipes = Recipes::load(std::path::Path::new(&recipes.unwrap_or_else(|| fail("--recipes is required")))).unwrap_or_else(|e| fail(&e));
    let signer = Signer::from_file(std::path::Path::new(&key.unwrap_or_else(|| fail("--signing-key is required")))).unwrap_or_else(|e| fail(&e));
    tracing::info!(public_key = %signer.public_key(), "build host starting");
    let host = Arc::new(BuildHost::new(recipes, Arc::new(C2Admission { api, token: c2_token }), signer));
    let listener = tokio::net::TcpListener::bind(listen).await.unwrap_or_else(|e| fail(&format!("bind: {e}")));
    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(true);
    });
    if let Err(error) = serve(listener, host, bearer, rx).await {
        fail(&error.to_string());
    }
}
