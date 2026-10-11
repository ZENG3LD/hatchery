# C2 as a container. Build context: a directory holding the hatchery checkout as
# `hatchery/`; dependencies resolve from crates.io. `deploy/docker-build.sh` stages it.
# No token or key is baked into the image; pass them at run time.
FROM rust:bookworm AS build
WORKDIR /src
COPY hatchery /src/hatchery
WORKDIR /src/hatchery
RUN cargo build --locked --release -p hatchery-c2 -p hatchery-c2-client

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 wireguard-tools \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/hatchery/target/release/hatchery-c2 /usr/local/bin/hatchery-c2
COPY --from=build /src/hatchery/target/release/hatchery-c2ctl /usr/local/bin/hatchery-c2ctl
ENTRYPOINT ["hatchery-c2"]
