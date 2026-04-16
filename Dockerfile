# ============================================================
# Stage 1 – Builder
# Uses the official Rust image to compile a fully static binary
# via musl so it runs on any Linux without glibc dependencies.
# ============================================================
FROM rust:1.78-alpine AS builder

# musl-dev supplies the C headers that openssl-sys / ring need when
# targeting musl.  pkg-config helps the linker locate system libs.
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static

WORKDIR /build

# Cache dependencies separately from source so a source-only change
# does not re-download / re-compile the whole crate graph.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl \
    && rm -rf src

# Now copy real source and do the final build.
COPY src ./src
# Touch main.rs so Cargo knows it changed (the dummy above was older).
RUN touch src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl

# ============================================================
# Stage 2 – Runtime
# Scratch image: nothing but the static binary.
# Total image size is typically ~5-8 MB.
# ============================================================
FROM scratch

# Copy CA certificates so HTTPS calls to the Telegram API work.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy the compiled binary.
COPY --from=builder \
    /build/target/x86_64-unknown-linux-musl/release/monad-monitoring \
    /usr/local/bin/monad-monitoring

# State files (.last_height, .last_status) are written to the working
# directory.  Mount a host volume here to persist state across restarts.
WORKDIR /data

ENTRYPOINT ["/usr/local/bin/monad-monitoring"]
