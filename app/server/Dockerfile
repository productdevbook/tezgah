# syntax=docker/dockerfile:1

# Not built here — "Nothing is built or tested on the machine this was
# written on" applies to `docker build` as much as it does to `cargo build`.
# CI builds this; the machine reading this comment does not.

FROM rust:1-slim-bookworm AS builder
WORKDIR /build

# The dependency layer, cached apart from the source that changes on every
# commit. A workspace manifest and lockfile plus a stand-in entry point for
# each member are enough for cargo to resolve and build every dependency
# before a single line of tezgah's own code is copied in.
COPY Cargo.toml Cargo.lock ./
COPY app/server/Cargo.toml app/server/Cargo.toml
# One stand-in per target the manifests declare — both libs and the binary.
# This list is a hand-kept copy of the crate layout: `app/server/src/lib.rs`
# arrived with the server's integration tests and this did not, and the image
# stopped building while CI stayed green, because nothing but this file reads
# these paths. `ci.yml` builds the image now, so the next one fails in the
# pull request instead.
RUN mkdir -p src app/server/src \
    && echo "" > src/lib.rs \
    && echo "" > app/server/src/lib.rs \
    && echo "fn main() {}" > app/server/src/main.rs \
    && cargo build --release --package tezgah-server \
    && rm -rf src app/server/src

COPY src ./src
COPY migrations ./migrations
COPY app/server/src ./app/server/src
# The stand-ins are older than the manifests that named them; cargo will not
# otherwise notice real source has replaced them.
RUN touch src/lib.rs app/server/src/lib.rs app/server/src/main.rs \
    && cargo build --release --package tezgah-server

# distroless/cc: glibc and libgcc for a binary built against them, CA roots
# for sqlx's TLS connection to Postgres, no shell, no package manager — and
# the `nonroot` tag's uid 65532 rather than a root default. No HEALTHCHECK:
# a k8s probe against GET /health is what decides whether this is up.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

COPY --from=builder /build/target/release/tezgah-server /usr/local/bin/tezgah-server

USER nonroot:nonroot
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/tezgah-server"]
