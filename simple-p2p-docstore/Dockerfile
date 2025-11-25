# Build stage
FROM rust:1.90-slim-bullseye AS builder

WORKDIR /usr/src/app
# Copy crate contents into the working directory
COPY . .

# Build release version of the server binary
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential ca-certificates \
  && rm -rf /var/lib/apt/lists/*

RUN  cargo build --release --bin server

# Production stage
FROM debian:bullseye-slim

# Create app directory
WORKDIR /app

# Create persistent data dir for identity and certs (use app working dir for tests)
ENV P2P_DATA_DIR=/app/.p2p
RUN mkdir -p ${P2P_DATA_DIR} && chown -R 1000:1000 ${P2P_DATA_DIR}

# Copy server binary
# Copy server binary and ensure the final user (UID 1000) owns it so it can run without root
COPY --from=builder --chown=1000:1000 /usr/src/app/target/release/server /app/server

# Identity files should not be baked into the image. To persist identity across runs,
# mount a host volume to ${P2P_DATA_DIR} at runtime instead.
# Copy entrypoint script
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

# Default signaling port
ENV SIGNALING_PORT=9090
ENV IDENTITY_KEY_PATH=${P2P_DATA_DIR}/identity.key
ENV CERT_PATH=${P2P_DATA_DIR}/webrtc_cert.der

# Expose UDP 9090 for WebRTC
EXPOSE 9090/udp

# Optionally expose a TCP port if you plan to enable TCP later
#EXPOSE 8080/tcp


# Switch to the non-root test user (UID 1000) to ensure permissions match mounted host data that is
# typically owned by the builder/developer user. This avoids chown failures during startup.
USER 1000:1000

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["/app/server"]
