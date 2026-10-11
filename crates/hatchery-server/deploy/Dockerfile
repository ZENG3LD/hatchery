# hatchery-server (+ harness and memory4agent binaries) as a container.
# Build context: a directory with sibling checkouts
#   hatchery/   gate4agent/
# (gate4agent resolves through the workspace [patch.crates-io] until the publish
# step). `deploy/docker-build.sh` stages the context.
# No token, key or master secret is baked in. Mount a persistent volume at
# /var/lib/hatchery (subservice data) and pass secrets through the environment
# or a secret file.
FROM rust:bookworm AS build
WORKDIR /src
COPY hatchery /src/hatchery
COPY gate4agent /src/gate4agent
WORKDIR /src/hatchery
RUN cargo build --locked --release -p hatchery-harness-service \
    && (cd crates/hatchery-server && cargo build --release)
# memory4agent 0.3 from crates.io (tree-sitter and bundled sqlite compile here, not in the workspace).
RUN cargo install memory4agent --version "^0.3" --locked --root /opt/mem

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/hatchery/crates/hatchery-server/target/release/hatchery-server /usr/local/bin/hatchery-server
COPY --from=build /src/hatchery/target/release/hatchery-harness /usr/local/bin/hatchery-harness
COPY --from=build /opt/mem/bin/memory4agent-server /usr/local/bin/memory4agent-server
VOLUME ["/var/lib/hatchery"]
ENV HATCHERY_SERVER_DATA_DIR=/var/lib/hatchery
ENTRYPOINT ["hatchery-server"]
