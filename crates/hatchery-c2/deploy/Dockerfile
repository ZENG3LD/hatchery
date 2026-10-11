# C2 as a container. Build context: a directory with these siblings
#   hatchery/   gate4agent/
# (gate4agent is resolved through the workspace [patch.crates-io] until the
# publish step). `deploy/docker-build.sh` stages the context.
# No token or key is baked into the image; pass them at run time.
FROM rust:bookworm AS build
WORKDIR /src
COPY hatchery /src/hatchery
COPY gate4agent /src/gate4agent
WORKDIR /src/hatchery
RUN cargo build --locked --release -p hatchery-c2 -p hatchery-c2-client

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 wireguard-tools \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/hatchery/target/release/hatchery-c2 /usr/local/bin/hatchery-c2
COPY --from=build /src/hatchery/target/release/hatchery-c2ctl /usr/local/bin/hatchery-c2ctl
ENTRYPOINT ["hatchery-c2"]
