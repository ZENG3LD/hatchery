#![cfg(unix)]
use hatchery_server::subservice::{ManagedProcess, State, Status, Subservice};
use hatchery_server::{control, ServerError, Supervisor};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Sleeper {
    process: ManagedProcess,
    on_demand: bool,
}

impl Subservice for Sleeper {
    fn name(&self) -> &'static str {
        "sleeper"
    }
    fn on_demand(&self) -> bool {
        self.on_demand
    }
    fn start(&mut self) -> Result<(), ServerError> {
        let mut command = tokio::process::Command::new("sleep");
        command.arg("30");
        self.process.spawn(command)
    }
    fn stop(&mut self) {
        self.process.stop();
    }
    fn status(&mut self) -> Status {
        let state = if self.process.alive() { State::Running } else { State::Stopped };
        Status { name: "sleeper", state, port: None, detail: None }
    }
}

fn sleeper(on_demand: bool) -> Supervisor {
    Supervisor::new(vec![Box::new(Sleeper { process: ManagedProcess::new(), on_demand })])
}

#[tokio::test]
async fn autostart_skips_on_demand_and_start_stop_by_name() {
    let supervisor = sleeper(true);
    assert!(supervisor.start_autostart().is_empty());
    assert_eq!(supervisor.statuses()[0].state, State::Stopped);
    assert_eq!(supervisor.start_named("sleeper").unwrap().state, State::Running);
    assert_eq!(supervisor.stop_named("sleeper").unwrap().state, State::Stopped);
    assert!(supervisor.start_named("missing").is_err());
    supervisor.stop_all();
}

#[tokio::test]
async fn autostart_starts_regular_subservices_and_stop_all_kills() {
    let supervisor = sleeper(false);
    assert!(supervisor.start_autostart().is_empty());
    assert_eq!(supervisor.statuses()[0].state, State::Running);
    supervisor.stop_all();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(supervisor.statuses()[0].state, State::Stopped);
}

async fn request(addr: std::net::SocketAddr, raw: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn control_health_open_mutation_needs_bearer() {
    let supervisor = sleeper(true);
    let listener = control::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(control::serve(listener, supervisor.clone(), Some("secret-token-value".into())));
    let health = request(addr, "GET /health HTTP/1.1\r\n\r\n").await;
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains("\"sleeper\""));
    let denied = request(addr, "POST /sub/sleeper/start HTTP/1.1\r\n\r\n").await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    let wrong = request(addr, "POST /sub/sleeper/start HTTP/1.1\r\nAuthorization: Bearer nope\r\n\r\n").await;
    assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
    let ok = request(
        addr,
        "POST /sub/sleeper/start HTTP/1.1\r\nAuthorization: Bearer secret-token-value\r\n\r\n",
    )
    .await;
    assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("running"), "{ok}");
    supervisor.stop_all();
}

#[tokio::test]
#[should_panic(expected = "loopback")]
async fn control_refuses_non_loopback_bind() {
    let _ = control::bind("0.0.0.0:0".parse().unwrap()).await;
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn memory_is_registered_but_never_autostarted() {
    let config = hatchery_server::ServerConfig::resolve(
        Default::default(),
        |name| (name == "HATCHERY_SERVER_DATA_DIR").then(|| "/nonexistent-data".to_owned()),
    )
    .unwrap();
    let supervisor = Supervisor::new(hatchery_server::build_subservices(&config));
    assert!(supervisor.start_autostart().is_empty());
    let statuses = supervisor.statuses();
    let memory = statuses.iter().find(|s| s.name == "memory").expect("memory registered");
    assert_eq!(memory.state, State::Stopped);
    assert!(memory.port.is_none());
}
