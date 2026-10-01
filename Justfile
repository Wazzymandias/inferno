# Run rustfmt and Clippy with warnings denied.
lint:
    cargo +nightly fmt --all
    cargo +nightly clippy --all-targets -- -D warnings

# Build the Infergate binary.
build:
    cargo build --bin infergate

test:
    cargo nextest run --all-targets

# Build the local image.
docker:
    docker build --tag infergate:local .

