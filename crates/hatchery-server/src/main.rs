use hatchery_server::{build_subservices, control, ServerConfig, Supervisor};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    if std::env::args().skip(1).any(|a| a == "--help" || a == "-h") {
        println!(
            "hatchery-server: configure with HATCHERY_SERVER_DATA_DIR (required), HATCHERY_SERVER_CONFIG (optional TOML), \
             HATCHERY_SERVER_CONTROL_LISTEN (loopback), HATCHERY_SERVER_CONTROL_TOKEN (bearer for start/stop)"
        );
        return;
    }
    let config = ServerConfig::from_env().unwrap_or_else(|error| fail(&error.to_string()));
    let supervisor = Supervisor::new(build_subservices(&config));
    for (name, error) in supervisor.start_autostart() {
        tracing::error!(subservice = name, %error, "subservice failed to start");
    }
    let listener = control::bind(config.control_listen)
        .await
        .unwrap_or_else(|error| fail(&format!("control listener: {error}")));
    tracing::info!("hatchery-server control listening (loopback)");
    let serving = tokio::spawn(control::serve(listener, supervisor.clone(), config.control_token.clone()));
    shutdown_signal().await;
    tracing::info!("shutting down");
    supervisor.stop_all();
    serving.abort();
}

fn fail(message: &str) -> ! {
    eprintln!("hatchery-server: {message}");
    std::process::exit(2);
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("sigterm handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
