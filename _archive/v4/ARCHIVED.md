# Archived: the original hatchery (tag `legacy-v4`)

The first hatchery: a Claude-only orchestrator (Nydus scheduler, Queen workers
over `claude -p` pipes, Overlord merge gate, SwarmPool heuristics) and a V4
modular pipeline that was built but never executed. Kept here detached from
cargo, never deleted. The repository now hosts the control plane, harness and
TUI that used to live in gate4agent.

Mechanisms judged worth a read-through for the new dispatcher are listed in
the workspace docs: `docs/hatchery/research/hatchery-inventory-2026-09-24.md`.
