FROM rust:1.86-slim AS builder

ARG IP_ADDR=0.0.0.0

WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY esp32-firmware ./esp32-firmware
COPY telemetry-server ./telemetry-server

RUN IP_ADDR=${IP_ADDR} cargo build-telemetry --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/main /usr/local/bin/telemetry-server

EXPOSE 8080

CMD ["telemetry-server"]
