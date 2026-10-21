# syntax=docker/dockerfile:1
#
# nuck-server image (ADR 0012). Multi-stage: compile in the Rust image, ship only the
# binary on a slim runtime. SQLx migrations are embedded at compile time.

FROM rust:1.99-bookworm AS build
WORKDIR /src

# The workspace manifest and lockfile first, so dependency builds can be cached.
# (No rust-toolchain.toml: the image's stable toolchain satisfies ruma's >= 1.89
# requirement, and the official image no longer publishes a `nightly` tag.)
COPY Cargo.toml Cargo.lock ./
COPY nuck-protocol ./nuck-protocol
COPY nuck-server ./nuck-server

RUN cargo build --release --locked -p nuck-server

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
