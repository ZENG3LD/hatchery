# hatchery local HQ

Local edge for hatchery: the terminal client (`hatchery-tui`, with the Pet Bastion arcade) built on your own machine. It talks to hq-core through the hq-edge (hq-web) API: `hatchery-tui hq tasks|sessions|signals|routing ...`.

The server line (node, c2, hq-core, hq-web) lives on the `main` branch.

```
cargo build --release -p hatchery-tui
```
