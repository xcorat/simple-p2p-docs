#!/usr/bin/env bash
set -euo pipefail

P2P_DATA_DIR=${P2P_DATA_DIR:-/app/.p2p}
IDENTITY_KEY_PATH=${IDENTITY_KEY_PATH:-$P2P_DATA_DIR/identity.key}
CERT_PATH=${CERT_PATH:-$P2P_DATA_DIR/webrtc_cert.der}
SIGNALING_PORT=${SIGNALING_PORT:-9090}

# Ensure the data dir exists with safe permissions
mkdir -p "$P2P_DATA_DIR"
# Only attempt to change ownership/permissions when running as root. This avoids permission errors
# when the container runs as non-root or when a volume mount prevents permission changes.
if [ "$(id -u)" = "0" ]; then
  chown -R 1000:1000 "$P2P_DATA_DIR" || true
  chmod 700 "$P2P_DATA_DIR" || true
fi

# Id files are ephemeral in this image and are created on server startup.
# If persistence is desired, mount a host volume to ${P2P_DATA_DIR} at runtime.

export SIGNALING_PORT
export IDENTITY_KEY_PATH
export CERT_PATH

# Execute the server as the current arguments
# If the image runs as root in container, it will still run; prefer running
# as non-root under container runtime if desired.
exec "$@"
