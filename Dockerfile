FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin slimcontroller

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/slimcontroller /usr/local/bin

# I'm passing in the extra arguments as secrets with fly.io.
# The corresponding environment variables are REDIS_URL and PASSWORD.
ENTRYPOINT ["/usr/local/bin/slimcontroller"]
CMD ["8080"]
