# syntax=docker/dockerfile:1
# Builds for the platform buildx targets; production is linux/arm64 (t4g, docs/DEPLOYMENT.md).
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY manifests ./manifests
RUN cargo build --release --locked && cp target/release/feedwatch-poller /app

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /app /app
USER nonroot
ENTRYPOINT ["/app"]
