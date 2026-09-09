FROM litestream/litestream:0.5.16@sha256:f085f8bce71a5ad4ce8e28b28ea522de1d9e0d7dd0af3ea5c1bd626d0f341954 AS litestream

FROM rust:1.97.1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --locked --release

FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime
RUN rm -f /etc/apt/sources.list.d/debian.sources && \
    printf 'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20260826T000000Z/ trixie main\ndeb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/20260826T000000Z/ trixie-security main\n' \
        > /etc/apt/sources.list.d/snapshot.list && \
    apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        tini \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir /data && chown 10001:10001 /data
COPY --from=builder /app/target/release/sqlite-web-starter /usr/local/bin/sqlite-web-starter
COPY --from=litestream /usr/local/bin/litestream /usr/local/bin/litestream
COPY litestream.yml /etc/litestream.yml
COPY --chmod=755 entrypoint.sh /usr/local/bin/entrypoint.sh
ENV SQLITE_PATH=/data/demo.sqlite PORT=3000 SHUTDOWN_TIMEOUT_SECONDS=25
USER 10001:10001
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
