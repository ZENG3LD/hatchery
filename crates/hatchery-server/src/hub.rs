//! P6: the hub behind the server control API: node calls forwarded by C2 and operator (HQ)
//! calls for mail and tasks. All state lives under the data dir; admission is always read from
//! the P2 ledger, never from the request.

/// The operator's own principal in node/ledger checks (never a node id).
pub const OPERATOR: &str = "hatchery.operator";
use crate::provision::{read_ledger, LedgerEntry, NodeStatus};
use crate::tasks::{Refused, Session, TaskStore};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug)]
pub struct HubError {
    pub status: u16,
    pub message: String,
}

impl HubError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

pub struct Hub {
    data_dir: PathBuf,
    /// m4a signer: per-human key trees, unlocked by each human's passkey PRF secret.
    signer: std::sync::Arc<Mutex<hatchery_keystore::keyring::Keyring>>,
    tasks: Mutex<TaskStore>,
    advice: Mutex<crate::routing::AdviceGate>,
    /// The real m4a: where session letters go (no separate messenger for sessions).
    link: Option<crate::m4a_link::M4aLink>,
}

fn refused(r: Refused) -> HubError {
    match r {
        Refused::NotFound => HubError::new(404, "not found"),
        Refused::Conflict(m) => HubError::new(409, m),
        Refused::Invalid(m) => HubError::new(400, m),
    }
}

fn int(e: crate::error::ServerError) -> HubError {
    HubError::new(500, e.to_string())
}

fn opt_str<'a>(v: &'a Value, name: &str) -> Option<&'a str> {
    v.get(name).and_then(Value::as_str)
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(name).and_then(|r| r.strip_prefix('='))).filter(|v| !v.is_empty())
}

fn str_field<'a>(v: &'a Value, name: &str) -> Result<&'a str, HubError> {
    v.get(name).and_then(Value::as_str).ok_or_else(|| HubError::new(400, format!("missing field: {name}")))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

impl Hub {
    fn routing(&self) -> Result<(crate::routing::RoutingConfig, std::collections::BTreeMap<(String, String), crate::routing::SeatState>), HubError> {
        let tasks = self.tasks.lock().expect("tasks");
        let cfg = crate::routing::RoutingConfig::from_store(&tasks.inventory().map_err(int)?, tasks.roles().map_err(int)?);
        // Fold the recent signal window (bounded table).
        let mut all = Vec::new();
        let mut after = None;
        loop {
            let page = tasks.signals(None, after, 500).map_err(int)?;
            let done = page.len() < 500;
            after = page.last().map(|s| s.id).or(after);
            all.extend(page);
            if done {
                break;
            }
        }
        let state = crate::routing::fold_signals(&all, &cfg);
        Ok((cfg, state))
    }

    /// Routing recommendation for `role` (operator API and node door).
    pub fn recommend(&self, role: &str) -> Result<Value, HubError> {
        let (cfg, state) = self.routing()?;
        let inventory = self.tasks.lock().expect("tasks").inventory().map_err(int)?;
        let rec = crate::routing::recommend_in(&cfg, Some(&inventory), &state, role, now_ms()).map_err(|e| HubError::new(400, e))?;
        Ok(json!({ "recommendation": rec }))
    }

    pub fn open(data_dir: &Path) -> Result<Self, crate::error::ServerError> {
        Ok(Self {
            data_dir: data_dir.to_owned(),
            signer: std::sync::Arc::new(Mutex::new(hatchery_keystore::keyring::Keyring::new(signer_dir(data_dir)?))),
            tasks: Mutex::new(TaskStore::open(data_dir)?),
            advice: Mutex::new(Default::default()),
            link: crate::m4a_link::M4aLink::from_env(data_dir),
        })
    }

    fn entry(&self, node_id: &str) -> Option<LedgerEntry> {
        read_ledger(&self.data_dir).ok()?.into_iter().find(|e| e.node_id == node_id)
    }

    /// Active (member/trusted) node holding `capability`, per the ledger.
    fn admitted(&self, node_id: &str, capability: &str) -> bool {
        node_id != OPERATOR
            && self.entry(node_id).is_some_and(|e| {
                matches!(e.status, NodeStatus::Member | NodeStatus::Trusted) && e.grants.capabilities.iter().any(|c| c == capability)
            })
    }

    /// A call made on behalf of a node. The caller (C2) has authenticated the node and checked its
    /// policy; the hub checks the ledger again, independently.
    pub fn node_call(&self, node_id: &str, op: &str, payload: &Value) -> Result<Value, HubError> {
        let capability = match op.split_once('.').map(|p| p.0) {
            Some("task") | Some("run") => "tasks",
            Some("signer") => "mail",
            _ => return Err(HubError::new(404, "unknown operation")),
        };
        if !self.admitted(node_id, capability) {
            return Err(HubError::new(403, "refused"));
        }
        match op {
            "task.list" => {
                let tasks = self.tasks.lock().expect("tasks").list(Some(node_id), true).map_err(|e| HubError::new(500, e.to_string()))?;
                Ok(json!({ "tasks": tasks }))
            }
            "task.update" => {
                let id = payload.get("id").and_then(Value::as_i64).ok_or_else(|| HubError::new(400, "missing field: id"))?;
                let status = str_field(payload, "status")?;
                if !matches!(status, "in_progress" | "done") {
                    return Err(HubError::new(400, "status must be in_progress or done"));
                }
                let changed = self.tasks.lock().expect("tasks").set_status(id, status, Some(node_id)).map_err(|e| HubError::new(500, e.to_string()))?;
                if changed { Ok(json!({ "ok": true })) } else { Err(HubError::new(403, "refused")) }
            }
            "task.route" => self.recommend(str_field(payload, "role")?),
            // What this node's provider CLIs expose (replaces the node's previous report).
            "task.provider_inventory" => {
                let providers: Vec<crate::routing::ProviderInventory> =
                    serde_json::from_value(payload.get("providers").cloned().unwrap_or(Value::Null)).map_err(|_| HubError::new(400, "bad providers"))?;
                let providers = crate::routing::clean_report(providers).map_err(|e| HubError::new(400, e))?;
                self.tasks.lock().expect("tasks").set_inventory(node_id, &providers).map_err(int)?;
                tracing::info!(node = node_id, providers = providers.len(), "provider inventory");
                Ok(json!({ "ok": true }))
            }
            // Advisor anti-noise gate: an advisor session asks before it sends advice via m4a.
            "task.advice_gate" => {
                let (cfg, _) = self.routing()?;
                let advisor = format!("{node_id}/{}", opt_str(payload, "advisor").unwrap_or("advisor"));
                match self.advice.lock().expect("advice").check(&cfg.advisor, &advisor, str_field(payload, "topic")?, now_ms()) {
                    Ok(()) => Ok(json!({ "allow": true })),
                    Err(why) => Ok(json!({ "allow": false, "reason": why })),
                }
            }
            // Short limit / login / ready signal read off a session screen by the node.
            "task.session_signal" => {
                let instance = payload.get("instance_id").and_then(Value::as_i64).ok_or_else(|| HubError::new(400, "missing field: instance_id"))?;
                let sig = self
                    .tasks
                    .lock()
                    .expect("tasks")
                    .add_signal(node_id, str_field(payload, "workspace_id")?, instance, str_field(payload, "provider")?, str_field(payload, "kind")?, opt_str(payload, "hint"))
                    .map_err(int)?
                    .map_err(refused)?;
                tracing::info!(node = node_id, provider = %sig.provider, kind = %sig.kind, "session signal");
                Ok(json!({ "signal_id": sig.id }))
            }
            // The run's own session closes it (decision 8); the run must live on this node.
            // `task.close_run` is the door-facing name; `run.close` is kept as an alias.
            "task.close_run" | "run.close" => {
                let run_id = payload.get("run_id").and_then(Value::as_i64).ok_or_else(|| HubError::new(400, "missing field: run_id"))?;
                let tasks = self.tasks.lock().expect("tasks");
                let run = tasks.run(run_id).map_err(int)?.ok_or_else(|| HubError::new(404, "not found"))?;
                if run.node_id != node_id {
                    return Err(HubError::new(403, "refused"));
                }
                let run = tasks
                    .close_run(run_id, str_field(payload, "status")?, "session", opt_str(payload, "session_id"), opt_str(payload, "branch"), opt_str(payload, "commit"), opt_str(payload, "result_message_id"))
                    .map_err(int)?
                    .map_err(refused)?;
                Ok(json!({ "run": run }))
            }
            // A fresh session on this node gets its identity from the account tree that owns
            // the node. The token goes back to the node's proxy, never into the PTY.
            "signer.provision" | "keystore.provision" => {
                let mut k = self.signer.lock().expect("signer");
                let account = k.account_for_node(node_id).ok_or_else(|| HubError::new(423, "signer locked for this node's account"))?;
                let p = k
                    .provision(&account, node_id, str_field(payload, "provider")?, str_field(payload, "session")?, hatchery_keystore::default_rights(), None)
                    .map_err(signer_err)?;
                Ok(json!({ "nick": p.nick, "token": p.token, "public_key": hex::encode(p.public_key) }))
            }
            "signer.sign" | "keystore.sign" => {
                let payload_bytes = hex::decode(str_field(payload, "payload_hex")?).map_err(|_| HubError::new(400, "bad payload"))?;
                let sig = self.signer.lock().expect("signer").sign_from_node(node_id, str_field(payload, "token")?, str_field(payload, "op")?, &payload_bytes).map_err(signer_err)?;
                Ok(json!({ "nick": sig.nick, "public_key_hex": hex::encode(sig.public_key), "signature_hex": hex::encode(sig.signature) }))
            }
            // Signer-signed letter: core signs as the token's own nick and delivers from that nick.
            "signer.send" | "keystore.send" => {
                let (to, subject, body) = (str_field(payload, "to")?, str_field(payload, "subject")?, str_field(payload, "body")?);
                let canonical = serde_json::to_vec(&json!({ "to": to, "subject": subject, "body": body })).map_err(|_| HubError::new(500, "encode"))?;
                let sig = self.signer.lock().expect("signer").sign_from_node(node_id, str_field(payload, "token")?, hatchery_keystore::RIGHT_SEND, &canonical).map_err(signer_err)?;
                if !hatchery_keystore::verify_signature(&sig, hatchery_keystore::RIGHT_SEND, &canonical) {
                    return Err(HubError::new(500, "signature check failed"));
                }
                // Into the real m4a, end-to-end encrypted, as the session's own m4a account
                // (identity key and vault master key rooted in the keystore).
                let link = self.link.as_ref().ok_or_else(|| HubError::new(503, "m4a link not configured"))?;
                let token = str_field(payload, "token")?.to_owned();
                let vault_key = self.signer.lock().expect("signer").m4a_vault_key(node_id, &token).map_err(signer_err)?;
                let (signer, node) = (self.signer.clone(), node_id.to_owned());
                let sign: crate::m4a_link::ExternalSign = Box::new(move |m: &[u8]| {
                    signer.lock().expect("signer").sign_m4a_keyproof(&node, &token, m).map(|s| s.signature).map_err(|e| e.to_string())
                });
                // E2E setup (login, key upload, peer join, Megolm share) can take longer than a
                // node call may block, so delivery runs on its own thread; the outcome is logged.
                // One delivery at a time: a session's store is opened by one client only.
                static DELIVERY: Mutex<()> = Mutex::new(());
                let (link, nick, node, to, body) = (link.clone(), sig.nick.clone(), node_id.to_owned(), to.to_owned(), body.to_owned());
                let public_key = sig.public_key;
                std::thread::spawn(move || {
                    let _one = DELIVERY.lock().unwrap_or_else(|e| e.into_inner());
                    match link.deliver_e2e(&nick, public_key, vault_key, sign, &to, &body) {
                        Ok((m4a, room, event)) => tracing::info!(node = %node, nick = %nick, m4a = %m4a, room = %room, event = %event, "signed e2e letter delivered to m4a"),
                        Err(e) => tracing::warn!(node = %node, nick = %nick, status = e.0, error = %e.1, "m4a delivery failed"),
                    }
                });
                Ok(json!({ "queued": true, "from": sig.nick, "signature_hex": hex::encode(sig.signature), "public_key_hex": hex::encode(sig.public_key) }))
            }
            "signer.revoke" | "keystore.revoke" => {
                let session = str_field(payload, "session")?;
                let mut k = self.signer.lock().expect("signer");
                let account = k.account_for_node(node_id).ok_or_else(|| HubError::new(423, "signer locked"))?;
                let b = k.branch(&account).map_err(signer_err)?;
                b.session_on_node(session, node_id).map_err(signer_err)?;
                b.revoke_session(session).map_err(signer_err)?;
                Ok(json!({ "ok": true }))
            }
            _ => Err(HubError::new(404, "unknown operation")),
        }
    }

    /// Operator signer calls. `prf_hex` is the account passkey's WebAuthn PRF output, evaluated
    /// in the human's browser; the passkey itself never reaches core.
    fn signer_operator(&self, op: &str, body: &Value) -> Result<Value, HubError> {
        let account = str_field(body, "account")?;
        let mut k = self.signer.lock().expect("signer");
        let human = || -> Result<hatchery_keystore::HumanKey, HubError> {
            let prf = hex::decode(str_field(body, "prf_hex")?).map_err(|_| HubError::new(400, "bad prf"))?;
            if prf.len() < 32 {
                return Err(HubError::new(400, "prf output too short"));
            }
            Ok(hatchery_keystore::HumanKey::from_passkey_prf(&prf))
        };
        match op {
            "enroll" => k.enroll(account, &human()?).map_err(signer_err)?,
            "unlock" => k.unlock(account, &human()?).map_err(signer_err)?,
            "lock" => k.lock(account),
            "register_node" => {
                let h = human()?;
                k.branch(account).map_err(signer_err)?.register_node(str_field(body, "node")?, &h).map_err(signer_err)?
            }
            "status" => return Ok(json!({ "account": account, "unlocked": k.is_unlocked(account), "exists": k.exists(account) })),
            // Browser passkey login (PRF evaluated in the human's browser): first time enrolls the
            // account's branch, then every active node is put under it, and other accounts
            // holding those nodes are locked (the passkey account replaces any bootstrap one).
            "passkey" => {
                let h = human()?;
                let enrolled = if k.exists(account) { k.unlock(account, &h).map_err(signer_err)?; false } else { k.enroll(account, &h).map_err(signer_err)?; true };
                let mut nodes: Vec<String> = read_ledger(&self.data_dir)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| matches!(e.status, NodeStatus::Member | NodeStatus::Trusted))
                    .map(|e| e.node_id)
                    .collect();
                for n in std::env::var("HATCHERY_CORE_NODE_IDS").unwrap_or_default().split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    if !nodes.iter().any(|x| x == n) {
                        nodes.push(n.to_owned());
                    }
                }
                let b = k.branch(account).map_err(signer_err)?;
                for n in &nodes {
                    if !b.has_node(n) {
                        b.register_node(n, &h).map_err(signer_err)?;
                    }
                }
                let locked = k.take_nodes(account, &nodes);
                tracing::info!(account, enrolled, nodes = nodes.len(), locked = locked.len(), "keystore unlocked by passkey");
                return Ok(json!({ "ok": true, "unlocked": true, "enrolled": enrolled, "nodes": nodes, "locked_others": locked }));
            }
            _ => return Err(HubError::new(404, "unknown signer operation")),
        }
        tracing::info!(account, op, "signer operator call");
        Ok(json!({ "ok": true, "unlocked": k.is_unlocked(account) }))
    }

    /// Operator (HQ) calls, authenticated by the control bearer in the HTTP layer.
    pub fn operator_call(&self, method: &str, path: &str, body: &Value) -> Result<Value, HubError> {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "signer", op]) => self.signer_operator(op, body),
            ("POST", ["v1", "tasks"]) => {
                let assignee = opt_str(body, "assignee");
                if let Some(node) = assignee {
                    if !self.admitted(node, "tasks") {
                        return Err(HubError::new(403, "assignee does not hold the tasks capability"));
                    }
                }
                let tasks = self.tasks.lock().expect("tasks");
                let op = opt_str(body, "op_id");
                if let Some(done) = tasks.replay(op).map_err(int)? {
                    return Ok(done);
                }
                let status = opt_str(body, "status");
                if let Some(st) = status {
                    if !matches!(st, "backlog" | "open" | "assigned") {
                        return Err(HubError::new(400, "initial status must be backlog, open or assigned"));
                    }
                }
                let task = tasks
                    .create_full(str_field(body, "title")?, opt_str(body, "body").unwrap_or(""), assignee, opt_str(body, "role"), body.get("parent_id").and_then(Value::as_i64), status)
                    .map_err(int)?;
                let out = json!({ "task": task });
                tasks.record(op, &out).map_err(int)?;
                Ok(out)
            }
            ("GET", ["v1", "tasks"]) => {
                let roles: Option<Vec<String>> = query_param(query, "role").map(|r| r.split(',').map(str::to_owned).collect());
                let tasks = self.tasks.lock().expect("tasks").list_filtered(query_param(query, "assignee"), roles.as_deref(), false).map_err(int)?;
                Ok(json!({ "tasks": tasks }))
            }
            ("GET", ["v1", "tasks", id]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let tasks = self.tasks.lock().expect("tasks");
                let task = tasks.get(id).map_err(int)?.ok_or_else(|| HubError::new(404, "not found"))?;
                Ok(json!({ "task": task, "runs": tasks.runs(id).map_err(int)? }))
            }
            ("POST", ["v1", "tasks", id, "move"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let tasks = self.tasks.lock().expect("tasks");
                let op = opt_str(body, "op_id");
                if let Some(done) = tasks.replay(op).map_err(int)? {
                    return Ok(done);
                }
                let task = tasks
                    .move_to(id, str_field(body, "status")?, body.get("expected_revision").and_then(Value::as_i64))
                    .map_err(int)?
                    .map_err(refused)?;
                let out = json!({ "task": task });
                tasks.record(op, &out).map_err(int)?;
                Ok(out)
            }
            ("POST", ["v1", "tasks", id, "runs"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let run = self.tasks.lock().expect("tasks").start_run(id, str_field(body, "session_id")?).map_err(int)?.map_err(refused)?;
                Ok(json!({ "run": run }))
            }
            ("POST", ["v1", "runs", id, "close"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let run = self
                    .tasks
                    .lock()
                    .expect("tasks")
                    .close_run(id, str_field(body, "status")?, "operator", None, opt_str(body, "branch"), opt_str(body, "commit"), opt_str(body, "result_message_id"))
                    .map_err(int)?
                    .map_err(refused)?;
                Ok(json!({ "run": run }))
            }
            ("GET", ["v1", "routing"]) => {
                let (cfg, state) = self.routing()?;
                let seats: Vec<Value> = cfg
                    .seats
                    .iter()
                    .map(|seat| {
                        let st = state.get(&(seat.node.clone(), seat.provider.clone())).cloned().unwrap_or_default();
                        json!({ "seat": seat, "state": st })
                    })
                    .collect();
                let inventory = self.tasks.lock().expect("tasks").inventory().map_err(int)?;
                Ok(json!({ "roles": cfg.roles, "seats": seats, "inventory": inventory, "advisor": cfg.advisor }))
            }
            // The user's role choice: preset entries must be models some node reported.
            ("POST", ["v1", "routing", "roles"]) => {
                let role = str_field(body, "role")?;
                if role.is_empty() || role.len() > 64 || !role.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)) {
                    return Err(HubError::new(400, "bad role name"));
                }
                let preset: Vec<String> = body
                    .get("preset")
                    .and_then(Value::as_array)
                    .ok_or_else(|| HubError::new(400, "missing field: preset"))?
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| HubError::new(400, "preset entries are strings")))
                    .collect::<Result<_, _>>()?;
                if preset.len() > 16 {
                    return Err(HubError::new(400, "preset too long"));
                }
                let tasks = self.tasks.lock().expect("tasks");
                let inventory = tasks.inventory().map_err(int)?;
                for entry in &preset {
                    let m = crate::routing::parse_model_ref(entry).ok_or_else(|| HubError::new(400, "bad preset entry"))?;
                    if !crate::routing::offered(&inventory, &m) {
                        return Err(HubError::new(409, format!("not offered by any node: {entry}")));
                    }
                }
                let r = crate::routing::Role { preset, auto: body.get("auto").and_then(Value::as_bool).unwrap_or(false) };
                tasks.set_role(role, &r).map_err(int)?;
                Ok(json!({ "role": role, "preset": r.preset, "auto": r.auto }))
            }
            ("POST", ["v1", "routing", "roles", role, "delete"]) => {
                let gone = self.tasks.lock().expect("tasks").delete_role(role).map_err(int)?;
                Ok(json!({ "deleted": gone }))
            }
            ("POST", ["v1", "routing", "recommend"]) => self.recommend(str_field(body, "role")?),
            ("GET", ["v1", "signals"]) => {
                let after = query_param(query, "after").and_then(|v| v.parse().ok());
                let limit = query_param(query, "limit").and_then(|v| v.parse().ok()).unwrap_or(100);
                let signals = self.tasks.lock().expect("tasks").signals(query_param(query, "node"), after, limit).map_err(int)?;
                Ok(json!({ "signals": signals }))
            }
            ("GET", ["v1", "sessions"]) => {
                let sessions = self.tasks.lock().expect("tasks").sessions(query_param(query, "node")).map_err(int)?;
                Ok(json!({ "sessions": sessions }))
            }
            // Record a real provider session (a provider conversation id); drafts are not recorded.
            ("POST", ["v1", "sessions"]) => {
                let session = Session {
                    session_id: str_field(body, "session_id")?.to_owned(),
                    node_id: str_field(body, "node_id")?.to_owned(),
                    provider: str_field(body, "provider")?.to_owned(),
                    workspace_id: opt_str(body, "workspace_id").map(str::to_owned),
                    role: opt_str(body, "role").map(str::to_owned),
                    state: opt_str(body, "state").unwrap_or("live").to_owned(),
                    created_at_ms: 0,
                    updated_at_ms: 0,
                };
                let tasks = self.tasks.lock().expect("tasks");
                let session = tasks.upsert_session(&session).map_err(int)?;
                if let Some(nick) = opt_str(body, "nick").filter(|n| !n.is_empty() && n.len() <= 128) {
                    tasks.set_session_nick(&session.session_id, nick).map_err(int)?;
                }
                Ok(json!({ "session": session }))
            }
            ("POST", ["v1", "tasks", id, "assign"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let node = str_field(body, "assignee")?;
                if !self.admitted(node, "tasks") {
                    return Err(HubError::new(403, "assignee does not hold the tasks capability"));
                }
                if self.tasks.lock().expect("tasks").assign(id, node).map_err(|e| HubError::new(500, e.to_string()))? { Ok(json!({ "ok": true })) } else { Err(HubError::new(409, "task cannot be assigned")) }
            }
            // Operator archive: finished tasks and idle session records leave the lists.
            ("POST", ["v1", "tasks", id, "archive"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let task = self.tasks.lock().expect("tasks").archive_task(id).map_err(int)?.map_err(refused)?;
                Ok(json!({ "task": task }))
            }
            ("POST", ["v1", "sessions", id, "archive"]) => {
                self.tasks.lock().expect("tasks").archive_session(id).map_err(int)?.map_err(refused)?;
                Ok(json!({ "archived": id }))
            }
            ("POST", ["v1", "tasks", id, "cancel"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                if self.tasks.lock().expect("tasks").set_status(id, "cancelled", None).map_err(|e| HubError::new(500, e.to_string()))? { Ok(json!({ "ok": true })) } else { Err(HubError::new(409, "task is final")) }
            }
            _ => Err(HubError::new(404, "not found")),
        }
    }
}

#[cfg(test)]
mod task_api_tests {
    use super::*;

    #[test]
    fn signer_ops_need_unlock_and_letters_come_from_the_session_nick() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::open(dir.path()).unwrap();
        let prf = hex::encode([5u8; 32]);
        hub.operator_call("POST", "/v1/signer/enroll", &json!({"account": "owner", "prf_hex": prf})).unwrap();
        hub.operator_call("POST", "/v1/signer/register_node", &json!({"account": "owner", "node": "alpha", "prf_hex": prf})).unwrap();
        // Wrong passkey secret cannot register nodes.
        assert_eq!(hub.operator_call("POST", "/v1/signer/register_node", &json!({"account": "owner", "node": "x", "prf_hex": hex::encode([6u8; 32])})).unwrap_err().status, 401);
        let k = hub.signer.lock().unwrap();
        assert_eq!(k.account_for_node("alpha").as_deref(), Some("owner"));
        drop(k);
        hub.operator_call("POST", "/v1/signer/lock", &json!({"account": "owner"})).unwrap();
        assert!(hub.signer.lock().unwrap().account_for_node("alpha").is_none());
        hub.operator_call("POST", "/v1/signer/unlock", &json!({"account": "owner", "prf_hex": prf})).unwrap();
        let p = hub.signer.lock().unwrap().provision("owner", "alpha", "codex", "s1", hatchery_keystore::default_rights(), None).unwrap();
        assert_eq!(p.nick, hub.signer.lock().unwrap().sign(&p.token, hatchery_keystore::RIGHT_SEND, b"x").unwrap().nick);
    }

    #[test]
    fn roles_only_from_reported_models() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::open(dir.path()).unwrap();
        let inv: Vec<crate::routing::ProviderInventory> = serde_json::from_value(json!([
            {"provider": "codex", "source": "cli", "models": [{"id": "m1", "efforts": ["high"]}]},
            {"provider": "grok", "source": "cli", "models": [{"id": "g1", "default": true}]}
        ]))
        .unwrap();
        hub.tasks.lock().unwrap().set_inventory("alpha", &inv).unwrap();
        // Invented model or effort: refused.
        assert_eq!(hub.operator_call("POST", "/v1/routing/roles", &json!({"role": "coder", "preset": ["codex/m9"]})).unwrap_err().status, 409);
        assert_eq!(hub.operator_call("POST", "/v1/routing/roles", &json!({"role": "coder", "preset": ["codex/m1:low"]})).unwrap_err().status, 409);
        hub.operator_call("POST", "/v1/routing/roles", &json!({"role": "coder", "preset": ["codex/m1:high", "grok/g1"], "auto": true})).unwrap();
        let r = hub.operator_call("POST", "/v1/routing/recommend", &json!({"role": "coder"})).unwrap();
        assert_eq!(r["recommendation"]["pick"]["seat"]["id"], "alpha/codex");
        assert_eq!(r["recommendation"]["auto"], true);
        let g = hub.operator_call("GET", "/v1/routing", &json!({})).unwrap();
        assert_eq!(g["inventory"].as_array().unwrap().len(), 2);
        // A node that stops reporting codex: the role falls to grok.
        hub.tasks.lock().unwrap().set_inventory("alpha", &inv[1..]).unwrap();
        let r = hub.operator_call("POST", "/v1/routing/recommend", &json!({"role": "coder"})).unwrap();
        assert_eq!(r["recommendation"]["pick"]["seat"]["id"], "alpha/grok");
        hub.operator_call("POST", "/v1/routing/roles/coder/delete", &json!({})).unwrap();
        assert!(hub.operator_call("POST", "/v1/routing/recommend", &json!({"role": "coder"})).is_err());
    }

    #[test]
    fn operator_task_create_move_cancel_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::open(dir.path()).unwrap();
        let op = |m: &str, p: &str, b: Value| hub.operator_call(m, p, &b);
        let t = op("POST", "/v1/tasks", json!({"title": "port", "role": "dev", "status": "backlog", "op_id": "c1"})).unwrap();
        let id = t["task"]["id"].as_i64().unwrap();
        // Idempotent create.
        assert_eq!(op("POST", "/v1/tasks", json!({"title": "port", "op_id": "c1"})).unwrap(), t);
        assert_eq!(op("GET", "/v1/tasks?role=advisor", json!({})).unwrap()["tasks"].as_array().unwrap().len(), 0);
        assert_eq!(op("GET", "/v1/tasks?role=dev", json!({})).unwrap()["tasks"].as_array().unwrap().len(), 1);
        let moved = op("POST", &format!("/v1/tasks/{id}/move"), json!({"status": "open", "expected_revision": 1})).unwrap();
        assert_eq!(moved["task"]["revision"], 2);
        assert_eq!(op("POST", &format!("/v1/tasks/{id}/move"), json!({"status": "open", "expected_revision": 1})).unwrap_err().status, 409);
        // Runs need a recorded real session.
        assert_eq!(op("POST", &format!("/v1/tasks/{id}/runs"), json!({"session_id": "s1"})).unwrap_err().status, 400);
        op("POST", "/v1/sessions", json!({"session_id": "s1", "node_id": "alpha", "provider": "codex"})).unwrap();
        let run = op("POST", &format!("/v1/tasks/{id}/runs"), json!({"session_id": "s1"})).unwrap();
        let rid = run["run"]["id"].as_i64().unwrap();
        // Node path is gated by the ledger.
        assert_eq!(hub.node_call("alpha", "run.close", &json!({"run_id": rid, "status": "done", "session_id": "s1"})).unwrap_err().status, 403);
        let closed = op("POST", &format!("/v1/runs/{rid}/close"), json!({"status": "cancelled"})).unwrap();
        assert_eq!(closed["run"]["closed_by"], "operator");
        let got = op("GET", &format!("/v1/tasks/{id}"), json!({})).unwrap();
        assert_eq!(got["task"]["status"], "cancelled");
        assert_eq!(got["runs"].as_array().unwrap().len(), 1);
        assert_eq!(op("POST", &format!("/v1/tasks/{id}/move"), json!({"status": "open"})).unwrap_err().status, 409);
        assert_eq!(op("GET", "/v1/sessions?node=alpha", json!({})).unwrap()["sessions"].as_array().unwrap().len(), 1);
    }
}

fn signer_dir(data_dir: &Path) -> Result<PathBuf, crate::error::ServerError> {
    let d = data_dir.join("m4a-signer");
    std::fs::create_dir_all(&d).map_err(|e| crate::error::ServerError::sub("signer", format!("dir: {}", e.kind())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(d)
}

fn signer_err(e: hatchery_keystore::SignerError) -> HubError {
    use hatchery_keystore::SignerError::*;
    tracing::warn!(error = ?e, "m4a signer refused a node call");
    match e {
        Locked => HubError::new(423, "signer locked"),
        WrongHumanKey | HumanApprovalRequired => HubError::new(401, "human key required"),
        Denied(_) | Retired | BadToken => HubError::new(403, "refused"),
        UnknownSession => HubError::new(404, "unknown session"),
        Store(m) => HubError::new(500, format!("signer store: {m}")),
    }
}
