# hatchery-harness-mcp

MCP stdio **session helper** for the grant-filtered harness API.

Not an HQ-facing control door. Product path: node `harness_mcp_proxy` +
`--session-proxy` + `HATCHERY_HARNESS_SESSION_*`. Doctrine:
hatchery-websession-docs `plans/node-stdio-jsonrpc-entry-2026-10-02.md`.
Rename ledger: `plans/harness-mcp-bin-env-domain-rename-2026-10-02.md`.

## Alias map (runtime names still mixed — §11.4 deferred)

| Kind | Current (wire / process) | Alias / target |
|---|---|---|
| Crate | `hatchery-harness-mcp` | (final) |
| **Bin** | `gate4agent-harness-mcp` | → `hatchery-harness-mcp` (wave C) |
| MCP `serverInfo.name` | `gate4agent-harness-mcp` | with bin |
| Launch `server_name` | `hatchery` | provider-visible id (separate layer) |
| Session env | `HATCHERY_HARNESS_SESSION_ENDPOINT` / `_TOKEN` | (final) |
| Trace / program env | `HATCHERY_HARNESS_MCP_TRACE*` / `_PROGRAM` | (final) |
| Legacy direct env | `GATE4AGENT_HARNESS_READ_ENDPOINT` / `_CREDENTIAL` | scrubbed on product spawn; rename or retire in wave D |
| Capability (g4a/C2) | `harness-mcp-read-proxy-v1` | **do not rename** (C2 negotiate) |

Never put tokens on argv or in logs.
