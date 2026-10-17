use hatchery_node::protocol::{
    ManagedWorktreeRetention, NodeId, SessionRecordRetentionConfig, WorkspaceId, WorktreeProfileId,
    WorktreeProfileRevision,
};
use hatchery_node::{
    default_node_endpoint, default_state_path, resolve_network_allowlist_catalog,
    HistorySourceLayout, ManagedWorktreeProfile, NativeHistoryConfig, NativeHistoryRoot,
    NodeSecretReference, NodeSecretResolveError, NodeSecretResolver, NodeSecretValue, NodeServer,
    NodeServerConfig, WorkspaceConfig, WorktreeServiceMode, NETWORK_ALLOWLIST_CATALOG_ENV,
};
use gate4agent_types::AdapterId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NODE_TOKEN_ENV: &str = "GATE4AGENT_NODE_TOKEN";
/// Optional node-local bridge secret (distinct from NODE_TOKEN). Never logged.
const BRIDGE_TOKEN_ENV: &str = "GATE4AGENT_BRIDGE_TOKEN";
/// Tip 6: 64 hex chars = 32-byte underlay AEAD key (never logged; ≠ BRIDGE_TOKEN).
const MESH_UNDERLAY_KEY_ENV: &str = "GATE4AGENT_MESH_UNDERLAY_KEY";
/// Tip 6: underlay probe/auth token barrier (never logged; ≠ BRIDGE_TOKEN).
const MESH_UNDERLAY_TOKEN_ENV: &str = "GATE4AGENT_MESH_UNDERLAY_TOKEN";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    #[cfg(unix)]
    {
        let mut homes: Vec<std::path::PathBuf> = std::env::var_os("HOME").map(Into::into).into_iter().collect();
        if let Some(extra) = std::env::var_os("HATCHERY_PROVIDER_HOMES") {
            homes.extend(std::env::split_paths(&extra));
        }
        let shim = homes.first().cloned().unwrap_or_else(std::env::temp_dir).join(".hatchery-provider-bin");
        for (provider, bin) in hatchery_node::provider_inventory::install_shims(&shim, &homes) {
            tracing::info!(%provider, bin = %bin.display(), "provider binary resolved via alias");
        }
    }
    let mut endpoint = default_node_endpoint()
        .and_then(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| hatchery_node::NodeServerError::InvalidEndpoint)
        })
        .unwrap_or_else(|error| fail(&error.to_string()));
    let mut api_listen = "127.0.0.1:18310"
        .parse()
        .expect("the built-in node API listen address must be valid");
    // Opt-in: default off. Loopback-only when set (enforced by with_bridge_listen).
    let mut bridge_listen: Option<std::net::SocketAddr> = None;
    // Tip 6: optional underlay UDP accept (may be non-loopback); relays to bridge_listen.
    let mut bridge_underlay_listen: Option<std::net::SocketAddr> = None;
    let mut call_home: Option<std::net::SocketAddr> = None;
    // Off by default. When set, check for claude/codex/kimi/grok and download
    // only missing binaries. Never runs provider logins.
    let mut install_provider_clis = false;
    let mut node_id = None;
    let mut workspaces = Vec::new();
    let mut worktree_modes = BTreeMap::new();
    let mut managed_profiles = Vec::new();
    let mut history_roots = Vec::new();
    let mut harness_mcp_helper: Option<PathBuf> = None;
    let mut network_allowlist_catalog: Option<PathBuf> = None;
    let mut session_record_retention = SessionRecordRetentionConfig::default();
    let mut plugin_root: Option<PathBuf> = None;
    let mut plugin_keys: Option<PathBuf> = None;
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    match raw_args.first().map(String::as_str) {
        // plugin verify|install|list|sign|keygen  (signed session plugins; see plugins.rs)
        Some("plugin") => {
            use hatchery_node::plugins;
            let a: Vec<&str> = raw_args[1..].iter().map(String::as_str).collect();
            let opt = |name: &str| a.iter().position(|x| *x == name).and_then(|i| a.get(i + 1)).map(|s| PathBuf::from(*s));
            let keys = || {
                let f = opt("--trusted-keys").unwrap_or_else(|| fail("--trusted-keys FILE is required"));
                plugins::load_trusted_keys(&f).unwrap_or_else(|e| fail(&e))
            };
            match a.first().copied() {
                Some("verify") => {
                    let pkg = a.get(1).unwrap_or_else(|| fail("plugin verify PKG_DIR --trusted-keys FILE"));
                    let (m, signer) = plugins::verify(Path::new(pkg), &keys()).unwrap_or_else(|e| fail(&e));
                    println!("ok {} {} signer={}", m.name, m.version, signer);
                }
                Some("install") => {
                    let pkg = a.get(1).unwrap_or_else(|| fail("plugin install PKG_DIR --root DIR --trusted-keys FILE"));
                    let root = opt("--root").unwrap_or_else(|| fail("--root DIR is required"));
                    let i = plugins::install(Path::new(pkg), &root, &keys()).unwrap_or_else(|e| fail(&e));
                    println!("installed {} {} signer={}", i.name, i.version, i.signer);
                }
                Some("list") => {
                    let root = opt("--root").unwrap_or_else(|| fail("--root DIR is required"));
                    for item in plugins::list(&root, &keys()) {
                        match item {
                            Ok(i) => println!("ok {} {} signer={}", i.name, i.version, i.signer),
                            Err((p, e)) => println!("INVALID {} {}", p.display(), e),
                        }
                    }
                }
                Some("sign") => {
                    // plugin sign PKG_DIR NAME VERSION --key SECRET_FILE [--providers a,b]
                    let (Some(pkg), Some(name), Some(version)) = (a.get(1), a.get(2), a.get(3)) else {
                        fail("plugin sign PKG_DIR NAME VERSION --key SECRET_FILE [--providers a,b]")
                    };
                    let key_file = opt("--key").unwrap_or_else(|| fail("--key SECRET_FILE is required"));
                    let secret = std::fs::read_to_string(&key_file).unwrap_or_else(|e| fail(&e.to_string()));
                    let providers = opt("--providers").map(|p| p.to_string_lossy().split(',').map(str::to_owned).collect()).unwrap_or_default();
                    let public = plugins::sign(Path::new(pkg), name, version, providers, &secret).unwrap_or_else(|e| fail(&e));
                    println!("signed {name} {version} public={public}");
                }
                Some("keygen") => {
                    // plugin keygen OUT_SECRET_FILE  (0600; prints only the public key)
                    let out = a.get(1).unwrap_or_else(|| fail("plugin keygen OUT_SECRET_FILE"));
                    let (secret, public) = plugins::keygen();
                    let mut o = std::fs::OpenOptions::new();
                    o.write(true).create_new(true);
                    #[cfg(unix)]
                    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
                    use std::io::Write;
                    o.open(out).and_then(|mut f| f.write_all(secret.as_bytes())).unwrap_or_else(|e| fail(&e.to_string()));
                    println!("public={public}");
                }
                _ => fail("plugin verify|install|list|sign|keygen"),
            }
            return;
        }
        Some("exec-as") => {
            // exec-as USER -- program args...  (used by --run-as for provider sessions)
            let user = raw_args.get(1).cloned().unwrap_or_else(|| fail("exec-as needs a user"));
            let rest = match raw_args.iter().position(|a| a == "--") {
                Some(i) => &raw_args[i + 1..],
                None => fail("exec-as needs `--` before the program"),
            };
            #[cfg(unix)]
            match hatchery_node::isolation::exec_as(&user, rest) {
                Ok(never) => match never {},
                Err(message) => fail(&message),
            }
            #[cfg(not(unix))]
            fail("exec-as is linux-only");
        }
        // drop-privileges --user USER -- program args...  (root, once; see isolation::drop_privileges)
        Some("drop-privileges") => {
            let user = match (raw_args.get(1).map(String::as_str), raw_args.get(2)) {
                (Some("--user"), Some(user)) => user.clone(),
                _ => fail("drop-privileges needs --user USER"),
            };
            let rest = match raw_args.iter().position(|a| a == "--") {
                Some(i) => &raw_args[i + 1..],
                None => fail("drop-privileges needs `--` before the program"),
            };
            #[cfg(unix)]
            match hatchery_node::isolation::drop_privileges(&user, rest) {
                Ok(never) => match never {},
                Err(message) => fail(&message),
            }
            #[cfg(not(unix))]
            fail("drop-privileges is linux-only");
        }
        // net-up --enroll-state DIR [--wg-key FILE] [--interface IF] [--kill-switch]  (root)
        Some("net-up") => {
            let (mut dir, mut key, mut iface, mut ks) = (None::<PathBuf>, None::<PathBuf>, "hatch0".to_owned(), false);
            let mut it = raw_args[1..].iter();
            while let Some(a) = it.next() {
                let mut v = |n: &str| it.next().cloned().unwrap_or_else(|| fail(&format!("{n} requires a value")));
                match a.as_str() {
                    "--enroll-state" => dir = Some(v("--enroll-state").into()),
                    "--wg-key" => key = Some(v("--wg-key").into()),
                    "--interface" => iface = v("--interface"),
                    "--kill-switch" => ks = true,
                    other => fail(&format!("unknown net-up argument: {other}")),
                }
            }
            let dir = dir.unwrap_or_else(|| fail("--enroll-state is required"));
            #[cfg(unix)]
            match hatchery_node::isolation::net_up(&dir, key.as_deref(), &iface, ks) {
                Ok(()) => println!("net-up ok interface={iface} kill_switch={ks}"),
                Err(message) => fail(&message),
            }
            #[cfg(not(unix))]
            fail("net-up is linux-only");
            return;
        }
        // call --c2-enroll ADDR --state-dir DIR --op mail.send --payload JSON
        Some("call") => {
            let (mut addr, mut dir, mut op, mut payload) = (None, hatchery_node::enroll_client::default_enroll_dir(), None, "{}".to_owned());
            let mut it = raw_args[1..].iter();
            while let Some(a) = it.next() {
                let mut v = |n: &str| it.next().cloned().unwrap_or_else(|| fail(&format!("{n} requires a value")));
                match a.as_str() {
                    "--c2-enroll" => addr = Some(v("--c2-enroll").pipe_resolve("--c2-enroll")),
                    "--state-dir" => dir = Some(v("--state-dir").into()),
                    "--op" => op = Some(v("--op")),
                    "--payload" => payload = v("--payload"),
                    other => fail(&format!("unknown call argument: {other}")),
                }
            }
            let dir: PathBuf = dir.unwrap_or_else(|| fail("no state directory (set --state-dir)"));
            let (identity, state) = hatchery_node::enroll_client::load_enrolled_signing_only(&dir).unwrap_or_else(|e| fail(&e.to_string()));
            let payload: serde_json::Value = serde_json::from_str(&payload).unwrap_or_else(|e| fail(&format!("--payload is not JSON: {e}")));
            match hatchery_node::enroll_client::node_call(
                addr.or_else(|| default_enroll_addr().map(|a| a.pipe_resolve("HATCHERY_ENROLL_ADDR"))).unwrap_or_else(|| fail("--c2-enroll is required")),
                &identity,
                &state.node_id,
                &op.unwrap_or_else(|| fail("--op is required")),
                &payload,
            )
            .await
            {
                Ok(reply) => println!("{reply}"),
                Err(error) => fail(&error.to_string()),
            }
            return;
        }
        // build --c2-enroll ADDR --state-dir DIR --recipe NAME --trusted-signer KEY --out FILE [--source-ref REF]
        Some("build") => {
            run_build(&raw_args[1..]).await;
            return;
        }
        // kill-switch --wg-if IF --endpoint IP:PORT [--apply]  (prints the ruleset, or installs it)
        Some("kill-switch") => {
            run_kill_switch(&raw_args[1..]);
            return;
        }
        // isolation-profile KIND  (prints the defaults of a node kind as JSON)
        Some("isolation-profile") => {
            let kind = match raw_args.get(1).map(String::as_str) {
                Some("local") => hatchery_enroll::NodeKind::Local,
                Some("docker") => hatchery_enroll::NodeKind::Docker,
                Some("qemu") => hatchery_enroll::NodeKind::Qemu,
                Some("remote") => hatchery_enroll::NodeKind::Remote,
                _ => fail("isolation-profile needs local, docker, qemu or remote"),
            };
            println!("{}", serde_json::to_string_pretty(&hatchery_enroll::IsolationProfile::for_kind(kind)).expect("profile serializes"));
            return;
        }
        _ => {}
    }
    if raw_args.first().map(String::as_str) == Some("enroll") {
        run_enroll(&raw_args[1..]).await;
        return;
    }
    let mut enrolled = false;
    let mut enroll_state: Option<PathBuf> = None;
    let mut no_wg = false;
    let mut door_listen: Option<std::net::SocketAddr> = None;
    let mut door_c2: Option<std::net::SocketAddr> = None;
    let mut restream_uplink: Option<std::net::SocketAddr> = None;
    let mut wake_policy_path: Option<PathBuf> = None;
    let mut kill_switch = std::env::var("HATCHERY_KILL_SWITCH").is_ok_and(|v| v == "1");
    let mut run_as: Option<String> = std::env::var("HATCHERY_RUN_AS").ok().filter(|v| !v.is_empty());
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            // Run as an enrolled node: identity and tunnel settings come from the state
            // written by `enroll`; call-home authenticates by signature.
            "--enrolled" => enrolled = true,
            "--enroll-state" => enroll_state = Some(required_value("--enroll-state", args.next()).into()),
            // The tunnel is managed outside the node (OS WireGuard client).
            "--no-wg" => no_wg = true,
            // Loopback door for agents: mail/task operations signed by this node and sent to C2.
            "--door-listen" => door_listen = Some(required_value("--door-listen", args.next()).parse().unwrap_or_else(|e| fail(&format!("--door-listen is invalid: {e}")))),
            "--door-c2" => door_c2 = Some(required_value("--door-c2", args.next()).pipe_resolve("--door-c2")),
            "--restream-uplink" => restream_uplink = Some(required_value("--restream-uplink", args.next()).parse()
                .unwrap_or_else(|e| fail(&format!("--restream-uplink is invalid: {e}")))),
            // Letter pipeline policy (deliver / terminal grant / routines / node mail), JSON.
            "--wake-policy" => wake_policy_path = Some(required_value("--wake-policy", args.next()).into()),
            // Egress is dropped unless it goes through the tunnel (fails closed).
            "--kill-switch" => kill_switch = true,
            // Provider sessions run as this unprivileged user; the node keeps the keys.
            "--run-as" => run_as = Some(required_value("--run-as", args.next())),
            "--endpoint" => endpoint = required_value("--endpoint", args.next()),
            // mail4agent courier bind (same as MAIL4AGENT_BIND). `127.0.0.1:0` = any free port.
            "--m4a-bind" => {
                let value = required_value("--m4a-bind", args.next());
                let _: std::net::SocketAddr = value
                    .parse()
                    .unwrap_or_else(|error| fail(&format!("--m4a-bind is invalid: {error}")));
                std::env::set_var(hatchery_node::kit::COURIER_BIND_ENV, value);
            }
            "--api-listen" => {
                let value = required_value("--api-listen", args.next());
                api_listen = value
                    .parse()
                    .unwrap_or_else(|error| fail(&format!("--api-listen is invalid: {error}")));
            }
            // Node-envelope HTTP+WS bridge (separate door from --api-listen ops HTTP).
            // Loopback only. Off unless supplied. Optional GATE4AGENT_BRIDGE_TOKEN.
            "--bridge-listen" => {
                let value = required_value("--bridge-listen", args.next());
                let addr = value
                    .parse()
                    .unwrap_or_else(|error| fail(&format!("--bridge-listen is invalid: {error}")));
                if bridge_listen.replace(addr).is_some() {
                    fail("--bridge-listen may only be supplied once");
                }
            }
            // Tip 6: mesh underlay UDP door → TCP relay to local --bridge-listen.
            // Requires BRIDGE_TOKEN + MESH_UNDERLAY_KEY + MESH_UNDERLAY_TOKEN.
            // HQ dial-only (node accepts as NodePeer). Not a WireGuard daemon.
            "--bridge-underlay-listen" => {
                let value = required_value("--bridge-underlay-listen", args.next());
                let addr = value.parse().unwrap_or_else(|error| {
                    fail(&format!("--bridge-underlay-listen is invalid: {error}"))
                });
                if bridge_underlay_listen.replace(addr).is_some() {
                    fail("--bridge-underlay-listen may only be supplied once");
                }
            }
            // Both default to `0` (disabled) via `SessionRecordRetentionConfig::default`
            // above -- a fresh node must never start deleting durable
            // records until an operator has chosen real values here.
            "--session-record-retention-age-ms" => {
                let value = required_value("--session-record-retention-age-ms", args.next());
                session_record_retention.age_ms = parse_session_record_retention_age_ms(&value)
                    .unwrap_or_else(|error| fail(&error));
            }
            "--session-record-retention-keep" => {
                let value = required_value("--session-record-retention-keep", args.next());
                session_record_retention.keep_per_workspace =
                    parse_session_record_retention_keep(&value)
                        .unwrap_or_else(|error| fail(&error));
            }
            // Dial the relay instead of only waiting to be dialled -- for
            // a node the relay has no way to reach. The node stays the
            // wire's server either way; only who places the call changes.
            "--c2-dial" => {
                let value = required_value("--c2-dial", args.next());
                call_home = Some(
                    value
                        .parse()
                        .unwrap_or_else(|error| fail(&format!("--c2-dial is invalid: {error}"))),
                );
            }
            "--node-id" => {
                let value = required_value("--node-id", args.next());
                let parsed = NodeId::new(value).unwrap_or_else(|error| fail(&error.to_string()));
                if node_id.replace(parsed).is_some() {
                    fail("--node-id may only be supplied once");
                }
            }
            "--workspace" => {
                let value = required_value("--workspace", args.next());
                let (id, root) = value
                    .split_once('=')
                    .unwrap_or_else(|| fail("--workspace requires ID=ABSOLUTE_PATH"));
                let workspace_id =
                    WorkspaceId::new(id).unwrap_or_else(|error| fail(&error.to_string()));
                workspaces.push(
                    WorkspaceConfig::new(workspace_id, root)
                        .unwrap_or_else(|error| fail(&error.to_string())),
                );
            }
            "--worktree-mode" => {
                let value = required_value("--worktree-mode", args.next());
                let (id, mode) = value.split_once('=').unwrap_or_else(|| {
                    fail("--worktree-mode requires WORKSPACE_ID=manual|managed|off")
                });
                let workspace_id =
                    WorkspaceId::new(id).unwrap_or_else(|error| fail(&error.to_string()));
                let mode = match mode {
                    "manual" => WorktreeServiceMode::Manual,
                    "managed" => WorktreeServiceMode::Managed,
                    "off" => WorktreeServiceMode::Off,
                    _ => fail("--worktree-mode requires manual, managed, or off"),
                };
                if worktree_modes.insert(workspace_id, mode).is_some() {
                    fail("--worktree-mode may only be supplied once per workspace");
                }
            }
            "--managed-worktree-profile" => {
                let value = required_value("--managed-worktree-profile", args.next());
                let (workspace, fields) = value.split_once('=').unwrap_or_else(|| {
                    fail("--managed-worktree-profile requires WORKSPACE_ID=PROFILE|REVISION|ABS_ROOT|BRANCH_PREFIX|BASE|RETENTION")
                });
                let workspace_id =
                    WorkspaceId::new(workspace).unwrap_or_else(|error| fail(&error.to_string()));
                let fields = fields.split('|').collect::<Vec<_>>();
                if fields.len() != 6 {
                    fail("--managed-worktree-profile requires exactly six pipe-separated profile fields");
                }
                let retention = match fields[5] {
                    "remove-when-released" => ManagedWorktreeRetention::RemoveWhenReleased,
                    "retain" => ManagedWorktreeRetention::Retain,
                    _ => fail("managed worktree retention must be remove-when-released or retain"),
                };
                let profile = ManagedWorktreeProfile::new(
                    WorktreeProfileId::new(fields[0])
                        .unwrap_or_else(|error| fail(&error.to_string())),
                    WorktreeProfileRevision::new(fields[1])
                        .unwrap_or_else(|error| fail(&error.to_string())),
                    fields[2],
                    fields[3],
                    fields[4],
                    retention,
                )
                .unwrap_or_else(|error| fail(&error));
                managed_profiles.push((workspace_id, profile));
            }
            "--plugin-root" => {
                plugin_root = Some(PathBuf::from(required_value("--plugin-root", args.next())));
            }
            "--plugin-trusted-keys" => {
                plugin_keys = Some(PathBuf::from(required_value("--plugin-trusted-keys", args.next())));
            }
            "--history-root" => {
                let value = required_value("--history-root", args.next());
                history_roots.push(parse_history_root(&value));
            }
            "--harness-mcp-helper" => {
                let value = PathBuf::from(required_value("--harness-mcp-helper", args.next()));
                if harness_mcp_helper.replace(value).is_some() {
                    fail("--harness-mcp-helper may only be supplied once");
                }
            }
            // Retained as a compatibility no-op. History discovery is opt-in;
            // Gate4Agent never derives provider storage from the process home.
            "--no-default-history" => {}
            // Station network allowlist catalog file (opaque ids, one per line).
            // Empty/unset keeps the empty-default catalog (unknown ids refuse).
            // Dig2 station probe is a separate optional feature — this flag is network ids only.
            "--network-allowlist-catalog" => {
                let value =
                    PathBuf::from(required_value("--network-allowlist-catalog", args.next()));
                if network_allowlist_catalog.replace(value).is_some() {
                    fail("--network-allowlist-catalog may only be supplied once");
                }
            }
            "--install-provider-clis" => {
                install_provider_clis = true;
            }
            "--help" | "-h" => {
                println!("gate4agent-node --node-id ID --workspace ID=ABSOLUTE_PATH [--worktree-mode ID=manual|managed|off] [--managed-worktree-profile 'ID=PROFILE|REVISION|ABS_ROOT|BRANCH_PREFIX|BASE|RETENTION'] [--history-root 'ADAPTER|LAYOUT|ABS_ROOT'] [--harness-mcp-helper ABSOLUTE_REGULAR_FILE] [--network-allowlist-catalog ABSOLUTE_REGULAR_FILE] [--endpoint ABSOLUTE_LOCAL_ENDPOINT] [--api-listen 127.0.0.1:PORT] [--bridge-listen 127.0.0.1:PORT] [--bridge-underlay-listen ADDR] [--c2-dial 127.0.0.1:PORT] [--install-provider-clis] [--session-record-retention-age-ms MILLISECONDS] [--session-record-retention-keep COUNT]");
                println!("RETENTION: remove-when-released or retain");
                println!("--session-record-retention-age-ms/--session-record-retention-keep: retire dead Unavailable managed session records; both default to 0 (disabled)");
                println!("LAYOUT: single-ndjson|single-json|json-or-ndjson|ndjson-with-optional-index|summary-json-with-sibling-ndjson|metadata-json-with-sibling-json|session-json-with-sibling-message-json|readonly-sqlite-projection|state-json-with-index-and-sibling-ndjson");
                println!("control token: {NODE_TOKEN_ENV} environment variable");
                println!("--bridge-listen: opt-in node-envelope HTTP+WS bridge (loopback only; separate from --api-listen)");
                println!("optional bridge token: {BRIDGE_TOKEN_ENV} (never logged; distinct from control token)");
                println!("--bridge-underlay-listen: tip 6 mesh underlay UDP accept → TCP relay to --bridge-listen (requires {BRIDGE_TOKEN_ENV} + {MESH_UNDERLAY_KEY_ENV} + {MESH_UNDERLAY_TOKEN_ENV}; HQ dial-only; no WG daemon)");
                println!("network allowlist catalog: --network-allowlist-catalog or {NETWORK_ALLOWLIST_CATALOG_ENV} (opaque ids, one per line; empty default)");
                println!("session environment: materialized beside the state file (<state-file>.session-environments); secret references fail closed");
                println!(
                    "--c2-dial: dial a relay's call-home listener instead of waiting to be dialled"
                );
                println!(
                    "--install-provider-clis: off by default; when set, download missing claude/codex/kimi/grok packages only (no logins)"
                );
                println!("node kit (default): dig2browser, mail4agent, session-restore, kernel WireGuard. Cores are individual features (m4a, d2b, session-restore, wireguard)");
                #[cfg(feature = "wireguard")]
                println!("kernel WireGuard peer (either side may dial): set {wg_iface} to a name plus {wg_key} (path only), {wg_addr}, {wg_peer_addr}, {wg_peer_key}; optional {wg_listen} (default 51820), {wg_endpoint} (omit so C2 dials this node). Not HQ. Not the UDP mesh stub.",
                    wg_iface = hatchery_node::WG_INTERFACE_ENV,
                    wg_key = hatchery_node::WG_PRIVATE_KEY_ENV,
                    wg_addr = hatchery_node::WG_ADDRESS_ENV,
                    wg_peer_addr = hatchery_node::WG_PEER_ADDRESS_ENV,
                    wg_peer_key = hatchery_node::WG_PEER_KEY_ENV,
                    wg_listen = hatchery_node::WG_LISTEN_PORT_ENV,
                    wg_endpoint = hatchery_node::WG_PEER_ENDPOINT_ENV,
                );
                return;
            }
            unknown => fail(&format!("unknown argument: {unknown}")),
        }
    }
    let mut call_home_identity = None;
    if enrolled {
        let dir = enroll_state
            .clone()
            .or_else(hatchery_node::enroll_client::default_enroll_dir)
            .unwrap_or_else(|| fail("no enrollment state directory (set --enroll-state)"));
        let loaded = if no_wg {
            // Tunnel and kill-switch were set up by `net-up`; this process never reads the WireGuard key.
            hatchery_node::enroll_client::load_enrolled_signing_only(&dir)
        } else {
            hatchery_node::enroll_client::load_enrolled(&dir)
        };
        let (identity, state) = loaded.unwrap_or_else(|error| fail(&error.to_string()));
        if call_home.is_none() {
            call_home = state.response.c2_call_home.as_deref().map(|addr| {
                addr.parse().unwrap_or_else(|error| fail(&format!("stored call-home address is invalid: {error}")))
            });
        }
        if !no_wg {
            for (key, value) in hatchery_node::enroll_client::wireguard_env_from_state(&dir, &state, "hatch0") {
                if std::env::var_os(&key).is_none() {
                    std::env::set_var(key, value);
                }
            }
        }
        if node_id.is_none() {
            node_id = Some(NodeId::new(state.node_id.clone()).unwrap_or_else(|error| fail(&error.to_string())));
        }
        let identity = Arc::new(identity);
        hatchery_node::m4a_accounts::set_node_signer(state.node_id.clone(), Arc::clone(&identity));
        call_home_identity = Some(identity);
    }
    #[cfg(unix)]
    {
        if let Some(user) = run_as.take() {
            hatchery_node::isolation::set_run_as(&user).unwrap_or_else(|error| fail(&error.to_string()));
            std::env::remove_var("HATCHERY_RUN_AS");
        }
        if kill_switch {
            let interface = std::env::var("GATE4AGENT_WG_INTERFACE").unwrap_or_else(|_| "hatch0".to_owned());
            let endpoint = std::env::var("GATE4AGENT_WG_PEER_ENDPOINT")
                .unwrap_or_else(|_| fail("--kill-switch needs the tunnel endpoint (enrolled state or GATE4AGENT_WG_PEER_ENDPOINT)"));
            let ks = hatchery_enroll::KillSwitch::new(&interface, &endpoint).unwrap_or_else(|e| fail(&e.to_string()));
            match hatchery_node::isolation::apply_kill_switch(&ks) {
                Ok(tool) => tracing::info!(tool, interface = %interface, "egress kill-switch installed"),
                Err(error) => fail(&error.to_string()),
            }
        }
    }
    // An enrolled node has no standing token; local clients of its own endpoint still need
    // one, so an unset variable gets a random per-process value that is never printed.
    let token = match std::env::var(NODE_TOKEN_ENV) {
        Ok(value) => value,
        Err(_) if enrolled => random_token().unwrap_or_else(|error| fail(&error)),
        Err(_) => fail(&format!("{NODE_TOKEN_ENV} is required")),
    };
    std::env::remove_var(NODE_TOKEN_ENV);
    let node_id = node_id.unwrap_or_else(|| fail("--node-id is required"));
    for workspace in &mut workspaces {
        if let Some(mode) = worktree_modes.remove(workspace.workspace_id()) {
            *workspace = workspace.clone().with_worktree_service_mode(mode);
        }
    }
    if !worktree_modes.is_empty() {
        fail("--worktree-mode references an unknown workspace");
    }
    for (workspace_id, profile) in managed_profiles {
        let workspace = workspaces
            .iter_mut()
            .find(|workspace| workspace.workspace_id() == &workspace_id)
            .unwrap_or_else(|| fail("--managed-worktree-profile references an unknown workspace"));
        *workspace = workspace
            .clone()
            .with_managed_worktree_profile(profile)
            .unwrap_or_else(|error| fail(&error.to_string()));
    }
    let node_id_for_door = Some(node_id.to_string());
    let state_path = match std::env::var_os("GATE4AGENT_NODE_STATE") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => default_state_path(&node_id).unwrap_or_else(|error| fail(&error.to_string())),
    };
    let endpoint_for_bridge = endpoint.clone();
    let token_for_bridge = token.clone();
    let node_id_for_bridge = node_id.to_string();
    // Resolve restream uplink early so we can disable legacy call-home when the
    // node-initiated bidirectional restreamer carries both read and write.
    let restream_uplink_early = restream_uplink.or_else(|| {
        std::env::var("HATCHERY_RESTREAM_UPLINK").ok().and_then(|v| v.parse().ok())
    });
    // Reuse, never replace: a node already serving this ops address keeps
    // running unless it is older (or HATCHERY_NODE_TAKEOVER=1) and offers a
    // graceful handoff; then it shuts itself down and this one takes over.
    {
        use hatchery_node::local_infra::{self as li, Probe, Upgrade};
        match li::probe(api_listen, "gate4agent-node", std::time::Duration::from_secs(2)) {
            Probe::Ours(info) => {
                let force = std::env::var("HATCHERY_NODE_TAKEOVER").is_ok_and(|v| v == "1");
                let plan = li::upgrade_plan(&info, env!("CARGO_PKG_VERSION"), force);
                let version = info.version.clone().unwrap_or_else(|| "?".to_owned());
                match plan {
                    Upgrade::Handoff if li::auto_upgrade_allowed() => {
                        tracing::info!(%api_listen, %version, pid = info.pid.unwrap_or(0), "running node is older or takeover requested; asking it to hand over");
                        if let Err(error) = li::handoff(api_listen, &info, 300, std::time::Duration::from_secs(60)) {
                            tracing::warn!(%error, "node handoff failed; the running node keeps serving, this one exits");
                            std::process::exit(0);
                        }
                        tracing::info!(%api_listen, "previous node handed over; continuing start");
                    }
                    _ => {
                        if plan == Upgrade::ManualOnly {
                            tracing::warn!(%version, "running node is older and has no control channel; stop it yourself to upgrade");
                        }
                        tracing::info!(%api_listen, %version, pid = info.pid.unwrap_or(0), "a gate4agent-node already serves this address; reusing it, not starting a second");
                        std::process::exit(0);
                    }
                }
            }
            Probe::Foreign(detail) => {
                tracing::warn!(%api_listen, %detail, "ops address held by something that is not gate4agent-node; choose another --api-listen");
            }
            Probe::Free => {}
        }
    }
    if let Err(error) = hatchery_node::node_control::install(hatchery_node::node_control::token_file_for(&state_path)) {
        tracing::warn!(cause = %error, "control token not written; control channel disabled");
    }
    let config = NodeServerConfig::new(endpoint, token, node_id, workspaces)
        .and_then(|config| config.with_state_path(&state_path))
        .and_then(|config| config.with_api_listen(api_listen))
        .and_then(|config| match bridge_listen {
            Some(addr) => config.with_bridge_listen(addr),
            None => Ok(config),
        })
        .and_then(|config| match (call_home, restream_uplink_early) {
            // Restreamer is the long-lived duplex stream; skip call-home to stop
            // the 5s auth-churn against a restreamer-only C2.
            (Some(_), Some(_)) => {
                tracing::info!("restream uplink configured; skipping legacy call-home");
                Ok(config)
            }
            (Some(relay), None) => config.with_call_home(relay),
            (None, _) => Ok(config),
        })
        .unwrap_or_else(|error| fail(&error.to_string()));
    let call_home_identity_for_door = call_home_identity.clone();
    let config = match call_home_identity {
        Some(identity) => config.with_call_home_identity(identity),
        None => config,
    };
    // Optional bridge secret — read then scrub from the process environment.
    // Never print. Distinct from NODE_TOKEN (already removed above).
    let config = match std::env::var(BRIDGE_TOKEN_ENV) {
        Ok(value) => {
            std::env::remove_var(BRIDGE_TOKEN_ENV);
            config
                .with_bridge_token(value)
                .unwrap_or_else(|error| fail(&error.to_string()))
        }
        Err(_) => config,
    };
    let config = match bridge_underlay_listen {
        Some(bind) => {
            let key_hex = std::env::var(MESH_UNDERLAY_KEY_ENV).unwrap_or_else(|_| {
                fail(&format!(
                    "{MESH_UNDERLAY_KEY_ENV} is required with --bridge-underlay-listen (64 hex digits)"
                ))
            });
            std::env::remove_var(MESH_UNDERLAY_KEY_ENV);
            let underlay_token = std::env::var(MESH_UNDERLAY_TOKEN_ENV).unwrap_or_else(|_| {
                fail(&format!(
                    "{MESH_UNDERLAY_TOKEN_ENV} is required with --bridge-underlay-listen"
                ))
            });
            std::env::remove_var(MESH_UNDERLAY_TOKEN_ENV);
            let transport_key =
                parse_underlay_key_hex(&key_hex).unwrap_or_else(|error| fail(&error));
            config
                .with_bridge_underlay(bind, transport_key, underlay_token)
                .unwrap_or_else(|error| fail(&error.to_string()))
        }
        None => config,
    };
    let config = if let Some(history) =
        explicit_history_config(history_roots).unwrap_or_else(|error| fail(&error))
    {
        config.with_history(history)
    } else {
        config
    };
    let config = if let Some(helper) = harness_mcp_helper {
        config
            .with_harness_mcp_helper(helper)
            .unwrap_or_else(|error| fail(&error.to_string()))
    } else {
        config
    };
    let config = config.with_session_record_retention(session_record_retention);
    // Station network allowlist catalog: CLI path wins, else env path, else empty.
    // Dig2 station probe is optional (`dig2-station-probe`). Never print GATE4AGENT_NODE_TOKEN.
    let allowlist_catalog = resolve_network_allowlist_catalog(network_allowlist_catalog)
        .unwrap_or_else(|error| fail(&error.to_string()));
    let config = config
        .with_network_allowlist_catalog(allowlist_catalog)
        .unwrap_or_else(|error| fail(&error.to_string()));
    // Same API the materialization tests use. The stock CLI has no secret
    // store: a reference resolves Unavailable and materialize stays fail-closed.
    // Root sits beside the durable state file so a bundle spawn can prepare
    // a session environment before the child exists.
    let config = with_cli_session_environment_materialization(config, &state_path)
        .unwrap_or_else(|error| fail(&error));
    if install_provider_clis {
        install_missing_provider_clis().unwrap_or_else(|error| fail(&error));
    }
    {
        // Link the enabled cores into this process. Does not start a browser,
        // provider CLI, or tunnel, and does not read secrets.
        let _linked = hatchery_node::force_link();
        tracing::info!(cores = ?hatchery_node::linked_core_names(), "node kit");
    }
    // Automatic mailbox: same daemon as the standalone mail4agent binary,
    // unless a courier already holds the port. A missing mailbox never
    // takes the node down.
    #[cfg(feature = "m4a")]
    let _mailbox = hatchery_node::kit::start_or_attach_mailbox();
    #[cfg(feature = "wireguard")]
    if let Some(wg) = hatchery_node::node_wg_from_env().unwrap_or_else(|error| fail(&error)) {
        tracing::info!(
            interface = %wg.interface_name,
            listen_port = wg.listen_port,
            dials_peer = wg.peer_endpoint.is_some(),
            "bringing up kernel wireguard node<->c2 peer"
        );
        hatchery_node::bring_up_node_wireguard(&wg)
            .unwrap_or_else(|error| fail(&error.to_string()));
    }
    let server = NodeServer::new(config).unwrap_or_else(|error| fail(&error.to_string()));
    // Signed plugins: load verified ones into provider homes so new sessions see them.
    match (plugin_root.clone(), plugin_keys.clone()) {
        (Some(root), Some(keys)) => {
            let trusted = hatchery_node::plugins::load_trusted_keys(&keys).unwrap_or_else(|e| fail(&e));
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| fail("--plugin-root needs HOME"));
            tokio::spawn(async move {
                let mut last = None;
                loop {
                    let (root, trusted, home) = (root.clone(), trusted.clone(), home.clone());
                    if let Ok((placed, skipped)) = tokio::task::spawn_blocking(move || hatchery_node::plugins::sync_into_home(&root, &trusted, &home)).await {
                        let now = (placed, skipped.clone());
                        if last.as_ref() != Some(&now) {
                            tracing::info!(placed, skipped = skipped.len(), "plugins synced into provider homes");
                            for why in &skipped {
                                tracing::warn!(reason = %why, "plugin file skipped");
                            }
                            last = Some(now);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            });
        }
        (None, None) => {}
        _ => fail("--plugin-root and --plugin-trusted-keys need each other"),
    }
    // Node calls (mail.*, task.*): over the restream uplink when there is one (the restream
    // bridge publishes it), else the legacy signed HTTP call to C2.
    let uplink_keys = restream_uplink_early.map(|_| resolve_upstream_keys(enroll_state.as_deref()).unwrap_or_else(|e| fail(&e)));
    let node_calls = node_id_for_door.clone().map(|id| {
        // Legacy signed HTTP node calls only for nodes WITHOUT a restream uplink:
        // C2 answers 410 "use restreamer uplink" to an uplinked node, so falling
        // back there just spammed the log every wake tick while the m4a channel
        // was (re)opening.
        let legacy = match (uplink_keys, door_c2, call_home_identity_for_door.clone()) {
            (None, Some(c2), Some(identity)) => Some((c2, identity)),
            _ => None,
        };
        hatchery_node::uplink_calls::NodeCalls::new(id, uplink_keys, legacy)
    });
    if let (Some(listen), Some(_c2), Some(_identity), Some(_id), Some(calls)) = (door_listen, door_c2, call_home_identity_for_door.clone(), node_id_for_door.clone(), node_calls.clone()) {
        if !listen.ip().is_loopback() {
            fail("--door-listen must be a loopback address");
        }
        let listener = tokio::net::TcpListener::bind(listen).await.unwrap_or_else(|e| fail(&format!("--door-listen: {e}")));
        let policy = std::sync::Arc::new(match &wake_policy_path {
            Some(path) => hatchery_node::wake_pipeline::WakePolicy::load(path).unwrap_or_else(|e| fail(&e)),
            None => hatchery_node::wake_pipeline::WakePolicy::default(),
        });
        let wake = hatchery_node::m4a_accounts::M4aSessionAccounts::from_env()
            .and_then(|a| a.wake_token().map_err(|e| tracing::warn!(error = %e, "letter wake off")).ok())
            .and_then(|token| server.wake_handle(token, std::sync::Arc::clone(&policy)));
        // Letters reach sessions only through the real m4a (the node's m4a client and its letter
        // hook); core keeps no mailbox to poll.
        // Short session signals (limit / login / ready) to core over the node-call channel.
        let (sig_tx, mut sig_rx) = tokio::sync::mpsc::unbounded_channel();
        hatchery_node::session_signals::set_sink(sig_tx);
        let sig_calls = calls.clone();
        tokio::spawn(async move {
            while let Some(out) = sig_rx.recv().await {
                let hint = match &out.signal {
                    hatchery_node::session_signals::Signal::Limit { hint } => Some(hint.clone()),
                    _ => None,
                };
                let payload = serde_json::json!({
                    "workspace_id": out.workspace,
                    "instance_id": out.instance,
                    "provider": out.provider,
                    "kind": out.signal.kind(),
                    "hint": hint,
                });
                match sig_calls.call("task.session_signal", &payload).await {
                    Ok(_) => tracing::info!(provider = %out.provider, kind = out.signal.kind(), "session signal sent"),
                    Err(e) => tracing::warn!(error = ?e, kind = out.signal.kind(), "session signal not delivered"),
                }
            }
        });
        // Provider inventory (enabled CLIs and the models they expose) to core: at start, then
        // every 30 min when it changed.
        let inv_calls = calls.clone();
        tokio::spawn(async move {
            let mut last: Option<serde_json::Value> = None;
            loop {
                let providers = tokio::task::spawn_blocking(hatchery_node::provider_inventory::detect_all).await.unwrap_or_default();
                let payload = serde_json::json!({ "providers": providers });
                if last.as_ref() != Some(&payload) {
                    match inv_calls.call("task.provider_inventory", &payload).await {
                        Ok(_) => {
                            let models = providers.iter().map(|p| format!("{}:{}", p.provider, p.models.len())).collect::<Vec<_>>().join(",");
                            tracing::info!(providers = providers.len(), %models, "provider inventory sent");
                            last = Some(payload);
                        }
                        Err(e) => tracing::warn!(error = ?e, "provider inventory not delivered"),
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(if last.is_some() { 1800 } else { 60 })).await;
            }
        });
        // m4a signer proxy (option A): sessions get a process-bound handle, core signs.
        #[cfg(unix)]
        if let Some(sock) = std::env::var_os(hatchery_node::keystore_proxy::SOCKET_ENV).filter(|v| !v.is_empty()) {
            let sock = PathBuf::from(sock);
            let _ = std::fs::remove_file(&sock);
            match tokio::net::UnixListener::bind(&sock) {
                Ok(l) => {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600));
                    let core = hatchery_node::keystore_proxy::UplinkCoreSigner::new(calls.clone(), tokio::runtime::Handle::current());
                    let proxy = std::sync::Arc::new(hatchery_node::keystore_proxy::SignerProxy::new(std::sync::Arc::new(core)));
                    if server.set_signer_proxy(proxy.clone()) {
                        tracing::info!("m4a signer proxy on (sessions get handles, no capabilities)");
                        tokio::spawn(hatchery_node::keystore_proxy::serve(proxy, l));
                    }
                }
                Err(e) => tracing::warn!(error = %e, "m4a signer proxy socket bind failed; proxy off"),
            }
        }
        tokio::spawn(hatchery_node::door::serve(listener, calls, wake));
    } else if door_listen.is_some() || door_c2.is_some() {
        fail("--door-listen and --door-c2 need each other and --enrolled");
    }
    // A2: opaque restreamer uplink (g4a bridged to core over Noise). Optional.
    let restream_uplink = restream_uplink_early;
    if let Some(uplink) = restream_uplink {
        let keys = uplink_keys.unwrap_or_else(|| fail("upstream keys"));
        let bridge_calls = node_calls.clone();
        let endpoint = endpoint_for_bridge;
        let node_id = node_id_for_bridge;
        let bridge_token = token_for_bridge;
        let journal = enroll_state.as_ref().map(|dir| {
            let path = dir.join("uplink.journal");
            std::sync::Arc::new(
                hatchery_node::uplink_journal::UplinkJournal::open(node_id.clone(), &path)
                    .unwrap_or_else(|e| fail(&format!("uplink journal: {e}"))),
            )
        });
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let browser_root = enroll_state.as_ref().map(|d| d.join("browser-profiles"));
        tokio::spawn(async move {
            loop {
                match hatchery_node::restream_g4a_bridge::run(
                    endpoint.clone(),
                    node_id.clone(),
                    bridge_token.clone(),
                    uplink,
                    keys,
                    journal.clone(),
                    browser_root.clone(),
                    bridge_calls.clone(),
                    stop_rx.clone(),
                )
                .await
                {
                    Ok(()) => break,
                    Err(err) => {
                        tracing::warn!(error = %err, %uplink, "restream g4a bridge ended; retry in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        if *stop_rx.borrow() { break; }
                    }
                }
            }
        });
        // Keep stop_tx alive for process lifetime.
        std::mem::forget(stop_tx);
        tracing::info!(%uplink, "restream g4a bridge starting");
    }
    let result = server.run_until_ctrl_signal().await;
    hatchery_node::node_control::cleanup();
    if let Err(error) = result {
        fail(&error.to_string());
    }
}

fn resolve_upstream_keys(enroll_dir: Option<&std::path::Path>) -> Result<hatchery_node::restream_session::UpstreamKeys, String> {
    use hatchery_node::restream_session::UpstreamKeys;
    use hatchery_core::nested::PublicKeyBytes;
    let from_b64 = |name: &str, v: &str| -> Result<PublicKeyBytes, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(v.trim())
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(v.trim()))
            .map_err(|e| format!("{name}: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!("{name}: expected 32 bytes, got {}", bytes.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(PublicKeyBytes(out))
    };
    let mut c2 = std::env::var("HATCHERY_UPSTREAM_C2_PUBLIC_B64").ok().filter(|s| !s.is_empty());
    let mut recip = std::env::var("HATCHERY_UPSTREAM_RECIPIENT_PUBLIC_B64").ok().filter(|s| !s.is_empty());
    if c2.is_none() || recip.is_none() {
        if let Some(dir) = enroll_dir {
            let path = dir.join("enrollment.json");
            if path.is_file() {
                let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let resp = v.get("response").unwrap_or(&v);
                if c2.is_none() {
                    c2 = resp.get("upstream_c2_public_key").and_then(|x| x.as_str()).map(str::to_owned);
                }
                if recip.is_none() {
                    recip = resp.get("upstream_recipient_public_key").and_then(|x| x.as_str()).map(str::to_owned);
                }
            }
        }
    }
    let c2 = c2.ok_or("upstream C2 public key missing (HATCHERY_UPSTREAM_C2_PUBLIC_B64 or enrollment.json)")?;
    let recip = recip.ok_or("upstream recipient public key missing (HATCHERY_UPSTREAM_RECIPIENT_PUBLIC_B64 or enrollment.json)")?;
    Ok(UpstreamKeys {
        c2_public: from_b64("c2", &c2)?,
        recipient_public: from_b64("recipient", &recip)?,
    })
}

/// Stock CLI secret resolver. There is no secret product here: any
/// reference is unavailable, so a profile that needs a secret fails closed
/// inside materialize instead of inventing a value.
struct CliUnavailableSecretResolver;

impl NodeSecretResolver for CliUnavailableSecretResolver {
    fn resolve(
        &self,
        _reference: &NodeSecretReference,
    ) -> Result<NodeSecretValue, NodeSecretResolveError> {
        Err(NodeSecretResolveError::Unavailable)
    }
}

/// Sibling of the state file, same shape as the delivery store root
/// (`<file>.delivery-store` → `<file>.session-environments`).
fn session_environment_root_for_state_path(
    state_path: &std::path::Path,
) -> Result<PathBuf, String> {
    let parent = state_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "state path has no parent for the session-environment root".to_owned())?;
    let file_name = state_path
        .file_name()
        .ok_or_else(|| "state path has no file name for the session-environment root".to_owned())?;
    let mut name = file_name.to_os_string();
    name.push(".session-environments");
    Ok(parent.join(name))
}

fn with_cli_session_environment_materialization(
    config: NodeServerConfig,
    state_path: &std::path::Path,
) -> Result<NodeServerConfig, String> {
    let root = session_environment_root_for_state_path(state_path)?;
    config
        .with_session_environment_materialization(root, Arc::new(CliUnavailableSecretResolver))
        .map_err(|error| error.to_string())
}

fn explicit_history_config(
    roots: Vec<NativeHistoryRoot>,
) -> Result<Option<NativeHistoryConfig>, String> {
    if roots.is_empty() {
        Ok(None)
    } else {
        NativeHistoryConfig::new(roots)
            .map(Some)
            .map_err(|error| error.to_string())
    }
}

fn parse_history_root(value: &str) -> NativeHistoryRoot {
    let mut fields = value.splitn(3, '|');
    let adapter = fields.next().unwrap_or_default();
    let layout = fields.next().unwrap_or_default();
    let root = fields.next().unwrap_or_default();
    if adapter.is_empty() || layout.is_empty() || root.is_empty() {
        fail("--history-root requires ADAPTER|LAYOUT|ABS_ROOT");
    }
    let adapter = AdapterId::new(adapter).unwrap_or_else(|error| fail(&error.to_string()));
    let layout = parse_history_layout(layout);
    NativeHistoryRoot::new(adapter, layout, root).unwrap_or_else(|error| fail(&error.to_string()))
}

fn parse_history_layout(value: &str) -> HistorySourceLayout {
    match value {
        "single-ndjson" => HistorySourceLayout::SingleNdjson,
        "single-json" => HistorySourceLayout::SingleJson,
        "json-or-ndjson" => HistorySourceLayout::JsonOrNdjson,
        "ndjson-with-optional-index" => HistorySourceLayout::NdjsonWithOptionalIndex,
        "summary-json-with-sibling-ndjson" => HistorySourceLayout::SummaryJsonWithSiblingNdjson,
        "metadata-json-with-sibling-json" => HistorySourceLayout::MetadataJsonWithSiblingJson,
        "session-json-with-sibling-message-json" => {
            HistorySourceLayout::SessionJsonWithSiblingMessageJson
        }
        "readonly-sqlite-projection" => HistorySourceLayout::ReadOnlySqliteProjection,
        "state-json-with-index-and-sibling-ndjson" => {
            HistorySourceLayout::StateJsonWithIndexAndSiblingNdjson
        }
        _ => fail("--history-root contains an unsupported layout"),
    }
}

fn parse_session_record_retention_age_ms(value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|error| format!("--session-record-retention-age-ms is invalid: {error}"))
}

fn parse_session_record_retention_keep(value: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|error| format!("--session-record-retention-keep is invalid: {error}"))
}


/// Check PATH for claude/codex/kimi/grok. Download only missing packages.
/// Never runs provider login or auth flows. Off unless `--install-provider-clis`.
fn install_missing_provider_clis() -> Result<(), String> {
    use std::process::Command;
    // Prefer the deploy script when present (Docker image copies it to sbin).
    let script_candidates = [
        PathBuf::from("/usr/local/sbin/install-provider-clis"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("deploy/node/install-provider-clis.sh"),
    ];
    for script in &script_candidates {
        if script.is_file() {
            let status = Command::new(script)
                .env("INSTALL_PROVIDER_CLIS", "1")
                .status()
                .map_err(|error| format!("install-provider-clis: {error}"))?;
            if status.success() {
                return Ok(());
            }
            return Err(format!(
                "install-provider-clis exited with {status}"
            ));
        }
    }
    // Fallback: inline check + npm for claude/codex. kimi/grok are optional:
    // warn when missing rather than failing node startup (owner can provision later).
    for (name, aliases, npm_pkg) in [
        ("claude", &["claude"][..], Some("@anthropic-ai/claude-code")),
        ("codex", &["codex"][..], Some("@openai/codex")),
        ("kimi", &["kimi", "kimi-cli"][..], None),
        ("grok", &["grok"][..], None),
    ] {
        let present = aliases.iter().any(|alias| {
            Command::new("sh")
                .args(["-c", &format!("command -v {alias}")])
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        });
        if present {
            tracing::info!(provider = name, "provider CLI already present");
            continue;
        }
        let Some(pkg) = npm_pkg else {
            tracing::warn!(
                provider = name,
                "provider CLI missing; continuing without it (no default package)"
            );
            continue;
        };
        tracing::info!(provider = name, package = pkg, "downloading missing provider CLI");
        match Command::new("npm").args(["install", "-g", pkg]).status() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::warn!(provider = name, %status, "npm install -g failed; continuing"),
            Err(error) => tracing::warn!(provider = name, %error, "npm install -g failed; continuing"),
        }
    }
    Ok(())
}

fn required_value(flag: &str, value: Option<String>) -> String {
    value.unwrap_or_else(|| fail(&format!("{flag} requires a value")))
}

fn parse_underlay_key_hex(value: &str) -> Result<[u8; 32], String> {
    let value = value.trim();
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("GATE4AGENT_MESH_UNDERLAY_KEY must be exactly 64 hex digits (32 bytes)".into());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in value.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex digit in GATE4AGENT_MESH_UNDERLAY_KEY".into()),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("gate4agent-node: {message}");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_node::NodeServerError;

    #[test]
    fn node_history_is_disabled_without_explicit_history_root() {
        assert_eq!(explicit_history_config(Vec::new()).unwrap(), None);
    }

    // The sample root is a Windows drive path; parsing it on unix exits the process.
    #[cfg(windows)]
    #[test]
    fn explicit_history_root_remains_the_only_history_authority() {
        let root =
            parse_history_root(r"codex|ndjson-with-optional-index|C:\operator-approved-history");
        let config = explicit_history_config(vec![root]).unwrap().unwrap();
        assert_eq!(config.roots().len(), 1);
    }

    #[test]
    fn session_record_retention_age_ms_parses_a_valid_flag_value() {
        assert_eq!(
            parse_session_record_retention_age_ms("604800000").unwrap(),
            604_800_000,
        );
    }

    #[test]
    fn session_record_retention_age_ms_refuses_a_non_numeric_value_by_name() {
        let error = parse_session_record_retention_age_ms("not-a-number").unwrap_err();
        assert!(error.starts_with("--session-record-retention-age-ms is invalid: "));
    }

    #[test]
    fn session_record_retention_keep_parses_a_valid_flag_value() {
        assert_eq!(parse_session_record_retention_keep("32").unwrap(), 32);
    }

    #[test]
    fn session_record_retention_keep_refuses_a_non_numeric_value_by_name() {
        let error = parse_session_record_retention_keep("not-a-number").unwrap_err();
        assert!(error.starts_with("--session-record-retention-keep is invalid: "));
    }

    #[test]
    fn cli_session_environment_root_sits_beside_the_state_file() {
        let root = session_environment_root_for_state_path(std::path::Path::new(
            "/var/lib/gate4agent/nodes/n1/state-v1.json",
        ))
        .unwrap();
        assert_eq!(
            root,
            PathBuf::from("/var/lib/gate4agent/nodes/n1/state-v1.json.session-environments"),
        );
    }

    #[test]
    fn cli_secret_resolver_fails_closed() {
        let reference = NodeSecretReference::new("fixture-secret").unwrap();
        match CliUnavailableSecretResolver.resolve(&reference) {
            Err(NodeSecretResolveError::Unavailable) => {}
            Ok(_) => panic!("cli secret resolver must not return a value"),
            Err(other) => panic!("expected unavailable, got {other}"),
        }
    }

    #[test]
    fn cli_materializer_initializes_from_the_state_path() {
        let scratch = std::env::temp_dir().join(format!(
            "g4a-cli-mat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let workspace = scratch.join("workspace");
        let state_parent = scratch.join("node");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&state_parent).unwrap();
        let state_path = state_parent.join("state-v1.json");
        let endpoint = scratch.join("node.sock");
        let config = NodeServerConfig::new(
            endpoint.to_string_lossy().as_ref(),
            "fixture-token",
            NodeId::new("cli-mat").unwrap(),
            [WorkspaceConfig::new(WorkspaceId::new("primary").unwrap(), &workspace).unwrap()],
        )
        .unwrap()
        .with_state_path(&state_path)
        .unwrap();
        let bare = config.clone();
        let config = with_cli_session_environment_materialization(config, &state_path).unwrap();
        let server = NodeServer::new(config).unwrap();
        drop(server);
        let marker = session_environment_root_for_state_path(&state_path)
            .unwrap()
            .join(".gate4agent-materialization-root");
        assert!(
            marker.is_file(),
            "materializer did not initialize beside the state file"
        );
        match bare.with_session_environment_materialization(
            PathBuf::from("relative-root"),
            Arc::new(CliUnavailableSecretResolver),
        ) {
            Err(NodeServerError::InvalidSessionEnvironmentRoot) => {}
            Ok(_) => panic!("relative session-environment root must be refused"),
            Err(other) => panic!("expected invalid root, got {other}"),
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| format!("cannot create a local token: {error}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `enroll --c2-enroll https://HOST|ADDR --node-id ID [--kind local|docker|qemu|remote] [--code CODE]
/// [--state-dir DIR]`: generate keys locally and send the signed request. The one-time code
/// may also come from `HATCHERY_ENROLL_CODE` (kept out of argv and shell history).
async fn run_enroll(args: &[String]) {
    let mut addr: Option<hatchery_node::enroll_client::EnrollTarget> = None;
    let mut id: Option<String> = None;
    let mut kind = hatchery_enroll::NodeKind::Local;
    let mut code = std::env::var("HATCHERY_ENROLL_CODE").ok();
    let mut dir = hatchery_node::enroll_client::default_enroll_dir();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().unwrap_or_else(|| fail(&format!("{name} requires a value")));
        match arg.as_str() {
            "--c2-enroll" => addr = Some(hatchery_node::enroll_client::EnrollTarget::parse(&value("--c2-enroll")).unwrap_or_else(|e| fail(&format!("--c2-enroll is invalid: {e}")))),
            "--node-id" => id = Some(value("--node-id")),
            "--code" => code = Some(value("--code")),
            "--state-dir" => dir = Some(value("--state-dir").into()),
            "--kind" => {
                kind = match value("--kind").as_str() {
                    "local" => hatchery_enroll::NodeKind::Local,
                    "docker" => hatchery_enroll::NodeKind::Docker,
                    "qemu" => hatchery_enroll::NodeKind::Qemu,
                    "remote" => hatchery_enroll::NodeKind::Remote,
                    _ => fail("--kind must be local, docker, qemu or remote"),
                }
            }
            other => fail(&format!("unknown enroll argument: {other}")),
        }
    }
    let addr = addr
        .or_else(|| default_enroll_addr().map(|a| hatchery_node::enroll_client::EnrollTarget::parse(&a).unwrap_or_else(|e| fail(&format!("HATCHERY_ENROLL_ADDR is invalid: {e}")))))
        .unwrap_or_else(|| fail("--c2-enroll is required (or HATCHERY_ENROLL_ADDR)"));
    let id = id.unwrap_or_else(|| fail("--node-id is required"));
    let dir = dir.unwrap_or_else(|| fail("no state directory (set --state-dir)"));
    if let hatchery_node::enroll_client::EnrollTarget::Plain(a) = &addr {
        if code.is_some() && !a.ip().is_loopback() {
            eprintln!("warning: the one-time code goes to {a} in the clear; use --c2-enroll https://HOST (a C2 with --enroll-tls-only burns codes sent this way)");
        }
    }
    match hatchery_node::enroll_client::enroll_to(&addr, &id, kind, code.as_deref(), &dir).await {
        Ok(state) => {
            let r = &state.response;
            println!(
                "enrolled node_id={} status={} tunnel_ip={} (state in {}; keys never leave this host)",
                r.node_id,
                r.status,
                r.tunnel_ip.as_deref().unwrap_or("-"),
                dir.display()
            );
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn run_kill_switch(args: &[String]) {
    let (mut wg_if, mut endpoint, mut apply) = (None, None, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--wg-if" => wg_if = it.next().cloned(),
            "--endpoint" => endpoint = it.next().cloned(),
            "--apply" => apply = true,
            other => fail(&format!("unknown kill-switch argument: {other}")),
        }
    }
    let ks = hatchery_enroll::KillSwitch::new(
        &wg_if.unwrap_or_else(|| fail("--wg-if is required")),
        &endpoint.unwrap_or_else(|| fail("--endpoint IP:PORT is required")),
    )
    .unwrap_or_else(|e| fail(&e.to_string()));
    if apply {
        #[cfg(unix)]
        match hatchery_node::isolation::apply_kill_switch(&ks) {
            Ok(tool) => println!("kill-switch installed with {tool}"),
            Err(error) => fail(&error.to_string()),
        }
        #[cfg(not(unix))]
        fail("kill-switch --apply is Linux only");
    } else {
        print!("{}", ks.nft_ruleset());
    }
}

/// Remote build: ask C2 (signed with this node's identity) to build and sign a recipe on the
/// build host; write the verified artifact. No signing key or build credential is involved here.
async fn run_build(args: &[String]) {
    let (mut addr, mut dir, mut recipe, mut signer, mut out, mut source_ref) = (None, hatchery_node::enroll_client::default_enroll_dir(), None, None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().cloned().unwrap_or_else(|| fail(&format!("{n} requires a value")));
        match arg.as_str() {
            "--c2-enroll" => addr = Some(value("--c2-enroll").pipe_resolve("--c2-enroll")),
            "--state-dir" => dir = Some(value("--state-dir").into()),
            "--recipe" => recipe = Some(value("--recipe")),
            "--trusted-signer" => signer = Some(value("--trusted-signer")),
            "--out" => out = Some(value("--out")),
            "--source-ref" => source_ref = Some(value("--source-ref")),
            other => fail(&format!("unknown build argument: {other}")),
        }
    }
    let dir: PathBuf = dir.unwrap_or_else(|| fail("no state directory (set --state-dir)"));
    let (identity, state) = hatchery_node::enroll_client::load_enrolled_signing_only(&dir).unwrap_or_else(|e| fail(&e.to_string()));
    let result = hatchery_node::enroll_client::request_build(
        addr.or_else(|| default_enroll_addr().map(|a| a.pipe_resolve("HATCHERY_ENROLL_ADDR"))).unwrap_or_else(|| fail("--c2-enroll is required")),
        &identity,
        &state.node_id,
        &recipe.unwrap_or_else(|| fail("--recipe is required")),
        source_ref.as_deref(),
        &signer.unwrap_or_else(|| fail("--trusted-signer is required")),
    )
    .await;
    match result {
        Ok((receipt, artifact)) => {
            std::fs::write(out.unwrap_or_else(|| fail("--out is required")), &artifact).unwrap_or_else(|e| fail(&e.to_string()));
            println!("built recipe={} sha256={} size={} signature_ok=true", receipt.recipe, receipt.artifact_sha256, receipt.artifact_size);
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// Resolve `HOST:PORT` or `IP:PORT` (enroll.example.org:18953) to one address, once at startup.
trait PipeResolve { fn pipe_resolve(&self, flag: &str) -> std::net::SocketAddr; }
impl PipeResolve for String {
    fn pipe_resolve(&self, flag: &str) -> std::net::SocketAddr {
        use std::net::ToSocketAddrs;
        match self.to_socket_addrs() {
            Ok(mut it) => it.find(|a| a.is_ipv4()).or_else(|| self.to_socket_addrs().ok().and_then(|mut i| i.next())).unwrap_or_else(|| fail(&format!("{flag}: {self} resolves to no address"))),
            Err(e) => fail(&format!("{flag} is invalid: {e}")),
        }
    }
}


/// Enroll address when `--c2-enroll` is absent: runtime `HATCHERY_ENROLL_ADDR`, then a value
/// baked in by the operator's own build via `option_env!`. Published builds bake nothing.
fn default_enroll_addr() -> Option<String> {
    std::env::var("HATCHERY_ENROLL_ADDR")
        .ok()
        .or_else(|| option_env!("HATCHERY_ENROLL_ADDR").map(str::to_owned))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}
