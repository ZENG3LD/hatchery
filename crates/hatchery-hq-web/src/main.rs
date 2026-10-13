//! `hatchery-hq-web`: web HQ for the operator. Configuration is environment only:
//!   HATCHERY_HQ_LISTEN            default 127.0.0.1:18500 (loopback only)
//!   HATCHERY_HQ_C2_API            C2 operator API address (ip:port)
//!   HATCHERY_HQ_C2_CONTROL        C2 control endpoint (unix socket path or tunnel address)
//!   HATCHERY_HQ_C2_TOKEN          C2 operator token (or GATE4AGENT_C2_TOKEN)
//!   HATCHERY_HQ_SERVER            hatchery-server control address (optional: mail and tasks)
//!   HATCHERY_SERVER_CONTROL_TOKEN hatchery-server control bearer (optional)
//!   HATCHERY_HQ_BASE_PATH         asset prefix behind a reverse proxy, e.g. /app (optional)
//!   HATCHERY_HQ_M4A_CLI / _AS / _SOCK / _GROUPS  the operator's m4a account via a local client daemon (optional)
//!   HATCHERY_HQ_CHATS             sidebar chats: label=node-id or label=room:NAME, comma separated (optional)
//!   HATCHERY_HQ_STATE_DIR         directory for the operator's sent-message journal (optional)

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();
    let config = hatchery_hq_web::Config::from_env().unwrap_or_else(|e| {
        eprintln!("hatchery-hq-web: {e}");
        std::process::exit(2)
    });
    let listen = config.listen;
    let (app, state) = hatchery_hq_web::router(config);
    hatchery_hq_web::spawn_mail_watcher(std::sync::Arc::clone(&state));
    hatchery_hq_web::spawn_topology_watcher(state);
    let listener = tokio::net::TcpListener::bind(listen).await.unwrap_or_else(|e| {
        eprintln!("hatchery-hq-web: bind {listen}: {e}");
        std::process::exit(2)
    });
    tracing::info!("web HQ on http://{listen}/ (tokens stay on the server side)");
    let _ = axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await;
}
