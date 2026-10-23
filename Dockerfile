FROM rust:1-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev perl make && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY server ./server
# Keep the desktop workspace member available for Cargo manifest resolution.
COPY src-tauri/Cargo.toml src-tauri/build.rs ./src-tauri/
COPY src-tauri/src ./src-tauri/src
RUN cargo build --locked --release -p noryn-sync

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 noryn && useradd --uid 10001 --gid noryn --no-create-home noryn \
    && mkdir -p /var/lib/noryn && chown noryn:noryn /var/lib/noryn
COPY --from=builder /build/target/release/noryn-sync /usr/local/bin/noryn-sync
ENV NORYN_SYNC_BIND=0.0.0.0:8787 NORYN_SYNC_DATA=/var/lib/noryn
USER 10001:10001
EXPOSE 8787
VOLUME ["/var/lib/noryn"]
ENTRYPOINT ["/usr/local/bin/noryn-sync"]
