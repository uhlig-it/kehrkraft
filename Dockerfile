# syntax=docker/dockerfile:1

# ----- Build stage -----
FROM rust:1 AS builder
WORKDIR /app

# Cache dependencies
COPY Cargo.toml ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs
RUN cargo build --release

# Build actual binary
COPY . .
RUN cargo build --release

# Download the prebuilt Typst CLI (static musl binary, runs on glibc too).
# TARGETARCH is set by BuildKit: amd64 on x86_64 hosts, arm64 on aarch64 hosts.
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && case "$TARGETARCH" in \
         arm64) TYPT_ARCH=aarch64 ;; \
         *) TYPT_ARCH=x86_64 ;; \
       esac \
    && curl -L --fail -o /tmp/typst.tar.xz \
        https://github.com/typst/typst/releases/download/v0.15.1/typst-$TYPT_ARCH-unknown-linux-musl.tar.xz \
    && mkdir -p /tmp/typst \
    && tar -xJf /tmp/typst.tar.xz -C /tmp/typst --strip-components=1 \
    && install -m 0755 /tmp/typst/typst /usr/local/bin/typst \
    && typst --version

# ----- Runtime stage -----
FROM debian:trixie-slim
RUN useradd -m -u 10001 appuser \
    && mkdir -p /app \
    && chown appuser:appuser /app
# Database file (KEHRKRAFT_DATABASE_URL default) is created at runtime under /app
COPY --from=builder /app/target/release/kehrkraft /usr/local/bin/kehrkraft
COPY --from=builder /usr/local/bin/typst /usr/local/bin/typst
WORKDIR /app
ENV RUST_LOG=info
ENV KEHRKRAFT_PORT=3000
USER appuser
EXPOSE 3000
CMD ["/usr/local/bin/kehrkraft"]
