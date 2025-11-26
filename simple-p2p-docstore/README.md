# Simple P2P Docstore (libp2p WebRTC)

Minimal example showing a native libp2p server (WebRTC) and a browser WASM client (WebRTC) using GossipSub for messaging. This is a starter for a docstore; storage and sync protocols can be added incrementally.

**Note**: Uses libp2p PR #5978 (browser-to-browser WebRTC) from https://github.com/elijahhampton/rust-libp2p

## Build

Install tools:

```bash
cargo install wasm-pack
cargo install basic-http-server
```

Build WASM package (outputs to `pkg/`):

```bash
wasm-pack build --target web --out-dir pkg
```

Run native server (the Docker image shipped with this repo exposes only WebRTC via UDP 9090; TCP is not exposed):

```bash
# Run locally using cargo (default signaling port 9090):
SIGNALING_PORT=9090 cargo run --release --bin server
```

Docker (recommended for browser testing):

```bash
docker compose up --build
```

Running with Docker Compose (recommended):

```bash
# Build and run
docker compose up --build

```

Running with Podman (dev — host networking helps with UDP):

```bash
# Build image then run with host networking (less NAT issues for UDP)
podman build -f simple-p2p-docstore/Dockerfile -t simple-p2p-server:latest simple-p2p-docstore
podman run --rm -it --net host -e SIGNALING_PORT=9090 simple-p2p-server:latest
```

Serve the web app:

```bash
basic-http-server www -a 127.0.0.1:8080
```

Open http://127.0.0.1:8080 and enter the server multiaddr. Example (replace certhash and peer id printed by the server):

```
/ip4/127.0.0.1/udp/9090/webrtc-direct/certhash/<hash>/p2p/<server-peer-id>
```

Notes:
- This Docker image is configured for WebRTC-only (UDP/9090) so it does not expose (or rely on) a TCP port out-of-the-box. If you need TCP connections, update the Rust server to listen on a fixed TCP port and add the mapping to `docker-compose.yml`.
- Browser-to-browser requires a Circuit Relay v2; this example focuses on browser→server first.
- Ensure UDP 9090 is reachable if testing across machines. For Podman and Docker NAT networking, you may prefer `--net=host` during development.
- For SharedArrayBuffer or WASM threads, the browser content must be served with Cross-Origin-Opener-Policy and Cross-Origin-Embedder-Policy headers (COOP and COEP). See below for example headers.

Required/Recommended HTTP headers for testing (examples):

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
Access-Control-Allow-Origin: http://localhost:8080  # or your actual dev host
Access-Control-Allow-Methods: GET, OPTIONS
Access-Control-Allow-Headers: Content-Type
```

Persistent keyfiles and certs:
- By default the server generates identities at startup. To persist identity/certs across restarts, mount a host directory to `/app/.p2p` and set `IDENTITY_KEY_PATH`/`CERT_PATH` env variables.

Podman note: if you use Podman on Linux and need UDP connectivity to map directly with less NAT complexity, prefer `--net=host` for dev testing. Example:

```bash
podman run --net=host -e SIGNALING_PORT=9090 <image>
```
