//! A2: core operator control (not C2) over an owner-only UDS.
#![cfg(unix)]

use hatchery_c2_client::connect_core_operator;
use hatchery_core::{BUILD_STAMP, NodeId};
use hatchery_server::node_controller::OperatorLink;
use hatchery_server::operator_control::{serve_local, NodeOperatorMesh};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

struct PrivateSocketDir(PathBuf);

impl PrivateSocketDir {
    fn new() -> Self {
        let root = std::env::temp_dir();
        for _ in 0..100 {
            let serial = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("hq-core-{}-{serial}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o700);
                    return Self(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create private socket directory: {error}"),
            }
        }
        panic!("could not allocate private socket directory")
    }

    fn endpoint(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for PrivateSocketDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_core_operator_exposes_authenticated_control_over_owner_only_uds() {
    let sockets = PrivateSocketDir::new();
    let control_endpoint = sockets.endpoint("core.sock");
    let mesh = NodeOperatorMesh::new();
    let (events_tx, _events_rx) = mpsc::unbounded_channel();
    let (link, _outbound) = OperatorLink::new(Some(events_tx));
    mesh.register("unix-local", link);

    let (stop, rx) = watch::channel(false);
    let endpoint = control_endpoint.clone();
    let mesh2 = mesh.clone();
    tokio::spawn(async move { serve_local(endpoint, "core-token", mesh2, rx).await });

    let (control, _events) = tokio::time::timeout(
        Duration::from_secs(5),
        connect_core_operator(&control_endpoint, "core-token"),
    )
    .await
    .expect("core operator UDS became available")
    .unwrap();

    assert_eq!(control.hello().build_stamp, BUILD_STAMP);
    let compatibility = control.hello().compatibility.as_ref().unwrap();
    assert_eq!(compatibility.host.operating_system.as_str(), std::env::consts::OS);
    assert_eq!(compatibility.host.architecture.as_str(), std::env::consts::ARCH);
    let topology = control.current_topology();
    assert_eq!(topology.nodes.len(), 1);
    assert_eq!(topology.nodes[0].node_id, NodeId::new("unix-local").unwrap());

    assert!(Path::new(&control_endpoint).exists());
    assert_eq!(
        fs::metadata(&control_endpoint).unwrap().permissions().mode() & 0o7777,
        0o600
    );

    let _ = stop.send(true);
    tokio::time::sleep(Duration::from_millis(50)).await;
}
