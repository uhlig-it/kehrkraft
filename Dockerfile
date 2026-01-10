# syntax=docker/dockerfile:1

# ----- Build stage -----
FROM rust:1 AS builder
WORKDIR /app

# Cache dependencies
COPY Cargo.toml .
RUN mkdir -p src && echo 'fn main() { println!("build placeholder"); }' > src/main.rs
RUN cargo build --release

# Build actual binary
COPY . .
RUN cargo build --release

# ----- Runtime stage -----
FROM debian:bookworm-slim
RUN useradd -m -u 10001 appuser
COPY --from=builder /app/target/release/kehrkraft /usr/local/bin/kehrkraft
ENV RUST_LOG=info
USER appuser
EXPOSE 3000
CMD ["/usr/local/bin/kehrkraft"]
