# Build stage
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Create dummy source for dependency caching
# Pin home crate to version compatible with Rust 1.85
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo update home@0.5.12 --precise 0.5.9 && \
    cargo build --release --features postgres && \
    rm -rf src

# Copy actual source
COPY src ./src
COPY migrations ./migrations

# Build with postgres feature for production
RUN touch src/main.rs && \
    cargo build --release --features postgres

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache ca-certificates libgcc

# Create non-root user
RUN addgroup -g 1000 app && \
    adduser -u 1000 -G app -s /bin/sh -D app

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/skip-select-simulator /app/skip-select-simulator

# Copy migrations for runtime migration
COPY --from=builder /app/migrations /app/migrations

# Set ownership
RUN chown -R app:app /app

USER app

EXPOSE 8080 9090

ENV RUST_LOG=skip_select_simulator=info,tower_http=info

CMD ["/app/skip-select-simulator"]
