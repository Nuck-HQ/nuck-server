# syntax=docker/dockerfile:1
#
# nuck-server image (ADR 0012). Multi-stage: compile in the Rust image, ship only the
# binary on a slim runtime. SQLx migrations are embedded at compile time.

FROM rust:1.99-bookworm AS build
WORKDIR /src

# Manifests and lockfile first, then fetch, so dependency builds — including
# nuck-protocol pulled from its pinned git tag — are cached across source changes.
# rust-toolchain.toml is not copied: the image's own stable 1.99 toolchain is what
# we build with.
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch --locked

COPY src ./src
COPY migrations ./migrations

RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/nuck-server /usr/local/bin/nuck-server

# Run unprivileged. nuck-server needs no filesystem writes (the projection is in
# Postgres and the AppService registration lives in Tuwunel's config).
USER 65534:65534
EXPOSE 8008
ENTRYPOINT ["/usr/local/bin/nuck-server"]
