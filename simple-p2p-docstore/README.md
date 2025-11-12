# Simple P2P Docstore (libp2p WebRTC)

Minimal example showing a native libp2p server (TCP + WebRTC) and a browser WASM client (WebRTC) using GossipSub for messaging. This is a starter for a docstore; storage and sync protocols can be added incrementally.

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

Run native server (listens on TCP random port and UDP 9090 for WebRTC-direct):

```bash
SIGNALING_PORT=9090 cargo run --release --bin server
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
- Browser-to-browser requires a Circuit Relay v2; this example focuses on browser→server first.
- Ensure UDP 9090 is reachable if testing across machines.
- For SharedArrayBuffer (if needed), serve with COOP/COEP headers.
