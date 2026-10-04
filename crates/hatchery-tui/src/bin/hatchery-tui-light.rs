use std::str::FromStr;

use hatchery_tui::{HarnessOperatorEndpoint, PtyColorMode, RunOptions};

const C2_TOKEN_ENV: &str = "GATE4AGENT_C2_TOKEN";

/// Parsed CLI surface: unchanged from the direct-C2 era (c2 endpoint arg +
/// `GATE4AGENT_C2_TOKEN` env, plus the mode-agnostic `--style` override) even
/// though what happens with it changed completely -- `main` now feeds
/// `c2_endpoint`/`c2_token` to `hatchery_harness_light::start_harness_light`
/// instead of dialing the app's own (now-dead) direct-C2 worker.
enum LightAttach {
    Unix(String),
    #[cfg(unix)]
    WireGuard(gate4agent_c2_client::HqWgClientConfig),
}

struct LightStartup {
    attach: LightAttach,
    c2_token: String,
    color_mode_override: Option<PtyColorMode>,
}

fn value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value"))
}

const USAGE: &str = "usage: hatchery-tui-light --c2-control PATH [--style inherit|gate]\n       \
     hatchery-tui-light --wg-client --c2-endpoint HOST:PORT --c2-public-key KEY \\\n       \
     --wg-private-key PATH --hq-tunnel-address IP --c2-tunnel-address IP \\\n       \
     --control-port PORT [--wg-listen-port 0] [--wg-allowed-ip IP/PREFIX] \\\n       \
     [--wg-keepalive SECS] [--wg-interface NAME] [--style inherit|gate]\n\
     credential env: GATE4AGENT_C2_TOKEN\n\
     unix --c2-control and --wg-client are alternatives; --node is rejected";

struct WgCli {
    enabled: bool,
    endpoint: Option<String>,
    public_key: Option<String>,
    private_key: Option<String>,
    hq_tunnel_address: Option<String>,
    c2_tunnel_address: Option<String>,
    control_port: Option<u16>,
    listen_port: u16,
    allowed_ips: Vec<String>,
    keepalive: u16,
    interface_name: String,
}

impl WgCli {
    fn new() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            public_key: None,
            private_key: None,
            hq_tunnel_address: None,
            c2_tunnel_address: None,
            control_port: None,
            listen_port: 0,
            allowed_ips: Vec::new(),
            keepalive: 25,
            interface_name: "wg-hq".to_owned(),
        }
    }

    fn any_detail(&self) -> bool {
        self.endpoint.is_some()
            || self.public_key.is_some()
            || self.private_key.is_some()
            || self.hq_tunnel_address.is_some()
            || self.c2_tunnel_address.is_some()
            || self.control_port.is_some()
            || self.listen_port != 0
            || !self.allowed_ips.is_empty()
            || self.interface_name != "wg-hq"
            || self.keepalive != 25
    }
}

fn parse_port(text: &str, flag: &str) -> Result<u16, String> {
    text.parse::<u16>().map_err(|_| format!("{flag} is invalid: {text}"))
}

fn parse_args_from(
    args: &[String],
    mut read_secret: impl FnMut(&str) -> Result<String, String>,
) -> Result<LightStartup, String> {
    let mut c2_control = None;
    let mut color_mode_override = None;
    let mut wg = WgCli::new();
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--node" => {
                return Err("direct Node mode is disabled; use --c2-control".to_owned());
            }
            "--c2-control" => {
                if c2_control.is_some() {
                    return Err("--c2-control can be specified only once".to_owned());
                }
                c2_control = Some(value(args, &mut index, "--c2-control")?);
            }
            "--wg-client" => {
                if wg.enabled {
                    return Err("--wg-client can be specified only once".to_owned());
                }
                wg.enabled = true;
            }
            "--c2-endpoint" => {
                if wg.endpoint.is_some() {
                    return Err("--c2-endpoint can be specified only once".to_owned());
                }
                wg.endpoint = Some(value(args, &mut index, "--c2-endpoint")?);
            }
            "--c2-public-key" => {
                if wg.public_key.is_some() {
                    return Err("--c2-public-key can be specified only once".to_owned());
                }
                wg.public_key = Some(value(args, &mut index, "--c2-public-key")?);
            }
            "--wg-private-key" => {
                if wg.private_key.is_some() {
                    return Err("--wg-private-key can be specified only once".to_owned());
                }
                wg.private_key = Some(value(args, &mut index, "--wg-private-key")?);
            }
            "--hq-tunnel-address" => {
                if wg.hq_tunnel_address.is_some() {
                    return Err("--hq-tunnel-address can be specified only once".to_owned());
                }
                wg.hq_tunnel_address = Some(value(args, &mut index, "--hq-tunnel-address")?);
            }
            "--c2-tunnel-address" => {
                if wg.c2_tunnel_address.is_some() {
                    return Err("--c2-tunnel-address can be specified only once".to_owned());
                }
                wg.c2_tunnel_address = Some(value(args, &mut index, "--c2-tunnel-address")?);
            }
            "--control-port" => {
                if wg.control_port.is_some() {
                    return Err("--control-port can be specified only once".to_owned());
                }
                wg.control_port = Some(parse_port(&value(args, &mut index, "--control-port")?, "--control-port")?);
            }
            "--wg-listen-port" => {
                wg.listen_port = parse_port(&value(args, &mut index, "--wg-listen-port")?, "--wg-listen-port")?;
            }
            "--wg-allowed-ip" => {
                wg.allowed_ips.push(value(args, &mut index, "--wg-allowed-ip")?);
            }
            "--wg-keepalive" => {
                wg.keepalive = parse_port(&value(args, &mut index, "--wg-keepalive")?, "--wg-keepalive")?;
            }
            "--wg-interface" => {
                wg.interface_name = value(args, &mut index, "--wg-interface")?;
            }
            "--style" => {
                color_mode_override = Some(PtyColorMode::from_str(&value(args, &mut index, "--style")?)?)
            }
            "--help" | "-h" => return Err(USAGE.to_owned()),
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
        index += 1;
    }

    if wg.enabled && c2_control.is_some() {
        return Err("unix --c2-control and --wg-client are alternatives".to_owned());
    }
    if !wg.enabled {
        if wg.any_detail() {
            return Err("WG client flags require --wg-client".to_owned());
        }
        let c2_endpoint = c2_control.ok_or_else(|| "configure --c2-control PIPE".to_owned())?;
        let c2_token = read_token(&mut read_secret)?;
        return Ok(LightStartup {
            attach: LightAttach::Unix(c2_endpoint),
            c2_token,
            color_mode_override,
        });
    }

    #[cfg(not(unix))]
    {
        let _ = wg;
        return Err("HQ WireGuard client dial is not available on this platform".to_owned());
    }
    #[cfg(unix)]
    {
        let config = wg_config(wg)?;
        let c2_token = read_token(&mut read_secret)?;
        Ok(LightStartup {
            attach: LightAttach::WireGuard(config),
            c2_token,
            color_mode_override,
        })
    }
}

fn read_token(read_secret: &mut impl FnMut(&str) -> Result<String, String>) -> Result<String, String> {
    let c2_token = read_secret(C2_TOKEN_ENV)?;
    if c2_token.is_empty() {
        return Err(format!("{C2_TOKEN_ENV} must not be empty"));
    }
    Ok(c2_token)
}

#[cfg(unix)]
fn wg_config(wg: WgCli) -> Result<gate4agent_c2_client::HqWgClientConfig, String> {
    use gate4agent_c2_client::{HqWgAllowedIp, HqWgClientConfig, HqWgPeer};
    use std::net::IpAddr;
    use std::path::PathBuf;

    let c2_tunnel_address: IpAddr = wg
        .c2_tunnel_address
        .ok_or_else(|| "--wg-client requires --c2-tunnel-address".to_owned())?
        .parse()
        .map_err(|_| "--c2-tunnel-address is invalid".to_owned())?;
    let hq_tunnel_address: IpAddr = wg
        .hq_tunnel_address
        .ok_or_else(|| "--wg-client requires --hq-tunnel-address".to_owned())?
        .parse()
        .map_err(|_| "--hq-tunnel-address is invalid".to_owned())?;
    let control_port = wg.control_port.ok_or_else(|| "--wg-client requires --control-port".to_owned())?;
    let private_key_path = PathBuf::from(
        wg.private_key.ok_or_else(|| "--wg-client requires --wg-private-key".to_owned())?,
    );
    let public_key = wg.public_key.ok_or_else(|| "--wg-client requires --c2-public-key".to_owned())?;
    let endpoint = match wg.endpoint {
        Some(text) => Some(text.parse().map_err(|_| format!("--c2-endpoint is invalid: {text}"))?),
        None => None,
    };
    let allowed_ips = if wg.allowed_ips.is_empty() {
        vec![HqWgAllowedIp::host(c2_tunnel_address)]
    } else {
        let mut parsed = Vec::with_capacity(wg.allowed_ips.len());
        for text in &wg.allowed_ips {
            parsed.push(
                HqWgAllowedIp::parse(text).ok_or_else(|| format!("--wg-allowed-ip is invalid: {text}"))?,
            );
        }
        parsed
    };
    let config = HqWgClientConfig {
        interface_name: wg.interface_name,
        private_key_path,
        listen_port: wg.listen_port,
        hq_tunnel_address,
        c2_tunnel_address,
        control_port,
        peers: vec![HqWgPeer {
            public_key,
            endpoint,
            allowed_ips,
            persistent_keepalive_secs: wg.keepalive,
        }],
    };
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

async fn start_attached(
    startup: &LightStartup,
) -> Result<hatchery_harness_light::HarnessLightRunning, hatchery_harness_light::HarnessLightError> {
    match &startup.attach {
        LightAttach::Unix(endpoint) => {
            hatchery_harness_light::start_harness_light(endpoint, &startup.c2_token).await
        }
        #[cfg(unix)]
        LightAttach::WireGuard(config) => {
            hatchery_harness_light::start_harness_light_wg(config.clone(), &startup.c2_token).await
        }
    }
}

fn parse_args() -> Result<LightStartup, String> {
    let args = std::env::args().collect::<Vec<_>>();
    parse_args_from(&args, |name| {
        let value = std::env::var(name)
            .map_err(|_| format!("{name} is required and must be valid Unicode"))?;
        std::env::remove_var(name);
        Ok(value)
    })
}

#[tokio::main]
async fn main() {
    hatchery_tui::diagnostics::install_panic_hook();
    let startup = match parse_args() {
        Ok(startup) => startup,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(if message.starts_with("usage:") { 0 } else { 2 });
        }
    };
    // The light binary hosts `hatchery-harness-light` in-process (see that
    // crate's own doc comment) instead of speaking the c2 dialect itself: the
    // minted operator credential/endpoint hand-off is an in-process return
    // value, never an env round trip.
    let running = match start_attached(&startup).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("hatchery-tui-light: harness-light startup failed: {error}");
            std::process::exit(1);
        }
    };
    let options = RunOptions {
        operator: HarnessOperatorEndpoint {
            endpoint: running.operator_endpoint(),
            credential: running.operator_credential(),
            launch_plan_id: None,
        },
        // The kanban (task/run board) stays disabled: light-harness tasks/
        // runs are always empty (no task kernel, by canon), so the session
        // board is the default view -- matching light's pre-cutover UX.
        kanban_default: false,
        color_mode_override: startup.color_mode_override,
        // Light has no `--control-plane` flag of its own yet -- this stays
        // `None` unconditionally, so `client::run` never binds a socket
        // for this binary. See `control_plane`'s own module doc comment.
        control_plane: None,
    };
    let run_result = hatchery_tui::run(options).await;
    if let Err(error) = running.shutdown().await {
        eprintln!("hatchery-tui-light: harness-light shutdown failed: {error}");
    }
    if let Err(error) = run_result {
        hatchery_tui::diagnostics::record_runtime(
            hatchery_tui::diagnostics::RuntimeDiagnostic::Fatal,
        );
        eprintln!("hatchery-tui-light: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parse(args: &[&str], secrets: &[(&str, &str)]) -> Result<LightStartup, String> {
        let args = args.iter().map(|value| value.to_string()).collect::<Vec<_>>();
        let secrets = secrets
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        parse_args_from(&args, |name| {
            secrets.get(name).cloned().ok_or_else(|| format!("missing {name}"))
        })
    }

    #[test]
    fn c2_endpoint_is_required_before_secret_access() {
        let args = vec!["hatchery-tui-light".to_owned()];
        let mut requested_secrets = Vec::new();
        let error = parse_args_from(&args, |name| {
            requested_secrets.push(name.to_owned());
            Err("unexpected secret read".to_owned())
        }).err().unwrap();
        assert_eq!(error, "configure --c2-control PIPE");
        assert!(requested_secrets.is_empty());
    }

    #[test]
    fn direct_node_and_harness_operator_arguments_are_not_available() {
        let direct = parse(
            &["hatchery-tui-light", "--node", r"desk-a=\\.\pipe\desk-a"],
            &[],
        ).err().unwrap();
        assert_eq!(direct, "direct Node mode is disabled; use --c2-control");

        let harness = parse(
            &["hatchery-tui-light", "--harness-operator", "127.0.0.1:18080"],
            &[],
        ).err().unwrap();
        assert_eq!(harness, "unknown argument: --harness-operator");
    }

    #[test]
    fn startup_arguments_are_not_accepted() {
        for argument in ["--startup-node", "--workspace", "--agent"] {
            let error = parse(
                &["hatchery-tui-light", argument, "value"],
                &[(C2_TOKEN_ENV, "token")],
            ).err().unwrap();
            assert_eq!(error, format!("unknown argument: {argument}"));
        }
    }

    #[test]
    fn c2_control_and_style_options_parse() {
        let startup = parse(
            &[
                "hatchery-tui-light",
                "--c2-control",
                r"\\.\pipe\gate4agent-c2",
                "--style",
                "gate",
            ],
            &[(C2_TOKEN_ENV, "c2-token")],
        ).unwrap();
        match &startup.attach {
            LightAttach::Unix(endpoint) => assert_eq!(endpoint, r"\\.\pipe\gate4agent-c2"),
            #[cfg(unix)]
            LightAttach::WireGuard(_) => panic!("unix mode expected"),
        }
        assert_eq!(startup.c2_token, "c2-token");
        assert_eq!(startup.color_mode_override, Some(PtyColorMode::GateOverride));
    }

    #[test]
    fn node_dial_flag_is_still_rejected_with_wg_client() {
        let error = parse(
            &["hatchery-tui-light", "--wg-client", "--node", "10.88.1.1:9"],
            &[],
        ).err().unwrap();
        assert_eq!(error, "direct Node mode is disabled; use --c2-control");
    }

    #[cfg(unix)]
    #[test]
    fn wg_client_rejects_missing_endpoint_listen_port_and_foreign_allowed_ips() {
        let base = [
            "hatchery-tui-light",
            "--wg-client",
            "--c2-public-key",
            "pyjbO05QwAyerdhvLEogM9JRv8P8Im52v2b3P1KHfwM=",
            "--wg-private-key",
            "/tmp/hq.priv",
            "--hq-tunnel-address",
            "10.88.1.3",
            "--c2-tunnel-address",
            "10.88.1.2",
            "--control-port",
            "27441",
        ];
        let missing = parse(&base, &[(C2_TOKEN_ENV, "token")]).err().unwrap();
        assert_eq!(missing, "HQ WireGuard client is missing the C2 public endpoint");

        let mut listen = base.to_vec();
        listen.extend(["--c2-endpoint", "192.168.78.2:51820", "--wg-listen-port", "51820"]);
        let listen = parse(&listen, &[(C2_TOKEN_ENV, "token")]).err().unwrap();
        assert_eq!(listen, "HQ WireGuard listen port must be 0, got 51820");

        let mut allowed = base.to_vec();
        allowed.extend([
            "--c2-endpoint",
            "192.168.78.2:51820",
            "--wg-allowed-ip",
            "10.88.1.0/24",
        ]);
        let allowed = parse(&allowed, &[(C2_TOKEN_ENV, "token")]).err().unwrap();
        assert_eq!(allowed, "HQ WireGuard allowed IPs must be only the single C2 tunnel address");

        let both = ["hatchery-tui-light", "--c2-control", "/tmp/gate4agent-c2.sock", "--wg-client"];
        let both = parse(&both, &[]).err().unwrap();
        assert_eq!(both, "unix --c2-control and --wg-client are alternatives");
    }

    #[test]
    fn unsupported_workspace_path_is_rejected() {
        let error = parse(
            &[
                "hatchery-tui-light",
                "--c2-control",
                r"\\.\pipe\gate4agent-c2",
                "--cwd",
                r"C:\work",
            ],
            &[(C2_TOKEN_ENV, "token")],
        ).err().unwrap();
        assert_eq!(error, "unknown argument: --cwd");
    }
}
