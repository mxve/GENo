FROM rust:alpine AS builder

# musl-dev for static builds
RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:3.20

# ca-certificates for https requests
RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder \
    /build/target/x86_64-unknown-linux-musl/release/GENo \
    /app/GENo

# default config, user can mount their own
COPY config.toml /app/config.toml

# sqlite db storage volume
VOLUME ["/app/data"]

EXPOSE 8080

ENTRYPOINT ["/app/GENo"]
