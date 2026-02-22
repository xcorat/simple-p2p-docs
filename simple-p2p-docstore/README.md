# Simple P2P Docstore (libp2p WebRTC)

Minimal example showing a native libp2p server (WebRTC) and a browser WASM client (WebRTC) using GossipSub for messaging. This is a starter for a docstore; storage and sync protocols can be added incrementally.

**Note**: Uses libp2p PR #5978 (browser-to-browser WebRTC) from https://github.com/elijahhampton/rust-libp2p

## Browser-to-Browser Communication

This project includes **Circuit Relay v2** support for browser-to-browser communication:

- **Server**: Acts as relay with HOP capability (forwards connections between browsers)
- **Browser**: Auto-detects relay servers and can listen for incoming connections via relay circuit
- **WebRTC**: Direct browser-to-browser WebRTC connections via relay signaling

### How It Works

1. Browser connects to relay server via WebRTC-direct
2. Server is auto-detected as relay (supports `/libp2p/circuit/relay/0.2.0/hop`)
3. Browser can click "Start Listening" to listen on relay circuit address
4. Other browsers can connect through the relay (potential DCUTR upgrade to direct connection)

**Note**: The custom libp2p fork (PR #5978) integrates relay circuit support directly into the WebRTC transport for browser clients.

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

### Build Configurations

The project supports feature flags for different WASM builds:

**Default build** (relay support enabled):
```bash
wasm-pack build --target web --out-dir pkg
```

**Minimal build** (relay support disabled):
```bash
wasm-pack build --target web --out-dir pkg --no-default-features
```

## Run

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

## Usage

Open http://127.0.0.1:8080 and enter the server multiaddr. Example (replace certhash and peer id printed by the server):

```
/ip4/127.0.0.1/udp/9090/webrtc-direct/certhash/<hash>/p2p/<server-peer-id>
```

**Note**: The `/p2p/<server-peer-id>` component is **optional** - the browser will auto-detect the relay peer ID via the Identify protocol.

### Testing Browser-to-Browser

1. **Open two browser tabs** (Tab A and Tab B)
2. **Connect both to the relay server**
   - Enter server address (with or without peer ID)
   - Verify "✓ Auto-detected and added relay" in console
3. **Tab A: Start listening**
   - Click "Start Listening" button
   - Check console for "✓ Started listening on /p2p/.../p2p-circuit/webrtc"
4. **Tab B: Send message to Tab A**
   - Copy Tab A's peer ID from the UI
   - Enter peer ID in "Send Direct Message" field
   - Message should be routed through relay circuit

## Notes

- This Docker image is configured for WebRTC-only (UDP/9090) so it does not expose (or rely on) a TCP port out-of-the-box. If you need TCP connections, update the Rust server to listen on a fixed TCP port and add the mapping to `docker-compose.yml`.
- Browser-to-browser connections use Circuit Relay v2 for signaling and connection establishment
- Ensure UDP 9090 is reachable if testing across machines. For Podman and Docker NAT networking, you may prefer `--net=host` during development.
- For SharedArrayBuffer or WASM threads, the browser content must be served with Cross-Origin-Opener-Policy and Cross-Origin-Embedder-Policy headers (COOP and COEP). See below for example headers.

### Troubleshooting

**"Multiaddr is not supported" when clicking "Start Listening":**
- This error indicates the WebRTC transport in this fork handles relay circuits differently than expected
- The browser successfully detects the relay and stores it in state
- The limitation may be in how the custom fork implements `/p2p-circuit` address support
- Workaround: Use direct peer-to-peer messaging for now (both browsers connected to same relay)

**Relay not detected:**
- Ensure server is running with Circuit Relay v2 HOP support (already configured)
- Check browser console for "✓ Peer supports Circuit Relay" message
- Verify relay shown in Network Status → Relays section

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
