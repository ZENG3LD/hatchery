use hatchery_server::control::HubHandle;
use hatchery_server::{build_subservices, control, mesh_runtime, ServerConfig, Supervisor};

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
             HATCHERY_SERVER_CONTROL_LISTEN (loopback), HATCHERY_SERVER_CONTROL_TOKEN (bearer for start/stop)\n\
             optional core mesh: HATCHERY_CORE_DRAIN_ADDR, HATCHERY_CORE_OPERATOR_LISTEN, HATCHERY_CORE_NODE_IDS;\n\
             tokens from HATCHERY_CORE_DRAIN_TOKEN and GATE4AGENT_CORE_OPERATOR_TOKEN (var names overridable)"
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

    let hub: HubHandle = open_hub(&config).await;

    let mesh = match mesh_runtime::MeshConfig::from_env(&config.data_dir) {
        Ok(Some(mesh_cfg)) => {
            let m4a_handler = m4a_drain_handler(&hub);
            Some(mesh_runtime::MeshRuntime::start_with_m4a(mesh_cfg, m4a_handler))
        }
        Ok(None) => None,
        Err(error) => fail(&format!("core mesh config: {error}")),
    };

    let serving = tokio::spawn(control::serve_with_hub(
        listener,
        supervisor.clone(),
        config.control_token.clone(),
        hub,
    ));
    shutdown_signal().await;
    tracing::info!("shutting down");
    if let Some(mesh) = &mesh {
        mesh.shutdown();
    }
    supervisor.stop_all();
    serving.abort();
}

#[cfg(feature = "m4a")]
async fn open_hub(config: &ServerConfig) -> HubHandle {
    if config.m4a_enabled && config.provision_c2_addr.is_some() {
        let dir = config.data_dir.clone();
        match tokio::task::spawn_blocking(move || hatchery_server::hub::Hub::open(&dir))
            .await
            .expect("hub open task")
        {
            Ok(hub) => {
                let hub = std::sync::Arc::new(hub);
                Some(hub)
            }
            Err(error) => {
                tracing::error!(%error, "tasks and keystore not started");
                None
            }
        }
    } else {
        None
    }
}

#[cfg(not(feature = "m4a"))]
async fn open_hub(_config: &ServerConfig) -> HubHandle {
    None
}

#[cfg(feature = "m4a")]
fn m4a_drain_handler(hub: &HubHandle) -> Option<hatchery_server::drain_consumer::CoreHandler> {
    hub.as_ref().map(|h| {
        std::sync::Arc::new(hatchery_server::node_sink::HubNodeSink::new(std::sync::Arc::clone(h)))
            .handler()
    })
}

#[cfg(not(feature = "m4a"))]
fn m4a_drain_handler(_hub: &HubHandle) -> Option<hatchery_server::drain_consumer::CoreHandler> {
    None
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
