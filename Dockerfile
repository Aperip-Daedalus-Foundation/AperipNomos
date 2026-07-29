# syntax=docker/dockerfile:1.7

FROM rust:1.95.0-bookworm AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src src

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    cargo build --locked --release --bin aperip-nomos && \
    install -D -m 0755 target/release/aperip-nomos /out/aperip-nomos

FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="AperipNomos" \
      org.opencontainers.image.description="Dual-port open-source license archive backed by RNMDB" \
      org.opencontainers.image.rnmdb.revision="013ec2f48a1dab89997430d72c2b176be2c29d47"

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 aperip-nomos && \
    useradd --uid 10001 --gid 10001 --no-create-home \
      --home-dir /nonexistent --shell /usr/sbin/nologin aperip-nomos && \
    install -d -o 10001 -g 10001 -m 0750 /var/lib/aperip-nomos

COPY --from=builder --chown=0:0 /out/aperip-nomos /usr/local/bin/aperip-nomos

USER 10001:10001
WORKDIR /var/lib/aperip-nomos
EXPOSE 54871 54872

ENTRYPOINT ["/usr/local/bin/aperip-nomos"]
