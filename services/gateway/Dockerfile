ARG RUST_IMAGE=rust:1.98-bookworm
ARG RUNTIME_IMAGE=gcr.io/distroless/cc-debian13:nonroot

FROM ${RUST_IMAGE} AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY services/gateway/Cargo.toml ./services/gateway/Cargo.toml
COPY services/gateway/src ./services/gateway/src
RUN rustup toolchain install nightly --profile minimal && cargo build --package infergate --bin infergate --release --locked

FROM ${RUNTIME_IMAGE}
COPY --from=build /app/target/release/infergate /usr/local/bin/infergate
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/infergate"]
