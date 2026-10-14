# C2 as a container. Build context: a directory holding the hatchery checkout as
# `hatchery/`; dependencies resolve from crates.io. `deploy/docker-build.sh` stages it.
# No token or key is baked into the image; pass them at run time.
FROM rust:alpine AS build
RUN apk add --no-cache musl-dev pkgconf openssl-dev openssl-libs-static sqlite-dev sqlite-static perl make cmake clang git linux-headers
ENV OPENSSL_STATIC=1 PKG_CONFIG_ALL_STATIC=1
WORKDIR /src
COPY hatchery /src/hatchery
WORKDIR /src/hatchery
RUN cargo build --locked --release -p hatchery-c2 -p hatchery-c2-client

FROM alpine:3
RUN apk add --no-cache ca-certificates iproute2 wireguard-tools
COPY --from=build /src/hatchery/target/release/hatchery-c2 /usr/local/bin/hatchery-c2
COPY --from=build /src/hatchery/target/release/hatchery-c2ctl /usr/local/bin/hatchery-c2ctl
ENTRYPOINT ["hatchery-c2"]
