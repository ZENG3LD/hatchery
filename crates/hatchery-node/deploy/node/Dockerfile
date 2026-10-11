# Mode 2 — node as a container. Builds the standard kit (default features).
# Does not provision secrets, tokens, or WireGuard private keys.
#
# Build context: a directory holding the hatchery checkout as `hatchery/`; every
# dependency resolves from crates.io. `deploy/node/docker-build.sh` stages it
# (it excludes target/ and .git).
#
#   ./crates/hatchery-node/deploy/node/docker-build.sh
#
# Provider CLIs are not installed unless you pass
#   --build-arg INSTALL_PROVIDER_CLIS=1 --build-arg PROVIDER_CLI_INSTALL='...'
# The service unit (mode 1) does not install them.

FROM rust:bookworm AS build
WORKDIR /src
COPY hatchery /src/hatchery
WORKDIR /src/hatchery
RUN cargo build --locked --release -p hatchery-node

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        iproute2 \
        wireguard-tools \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/hatchery/target/release/gate4agent-node /usr/local/bin/gate4agent-node
COPY hatchery/crates/hatchery-node/deploy/node/install-provider-clis.sh /usr/local/sbin/install-provider-clis
COPY hatchery/crates/hatchery-node/deploy/node/gate4agent-node.service /etc/systemd/system/gate4agent-node.service
ARG INSTALL_PROVIDER_CLIS=0
ARG PROVIDER_CLI_INSTALL=
RUN chmod 0755 /usr/local/sbin/install-provider-clis \
    && INSTALL_PROVIDER_CLIS="$INSTALL_PROVIDER_CLIS" PROVIDER_CLI_INSTALL="$PROVIDER_CLI_INSTALL" /usr/local/sbin/install-provider-clis
# No ENV token, no COPY of a key. The process reads those from the operator at runtime.
ENTRYPOINT ["gate4agent-node"]
