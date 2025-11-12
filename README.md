# Simple P2P Docs

Documentation and examples for building peer-to-peer applications with libp2p and WebRTC.

## Contents

- **`simple-p2p-docstore/`** - Working example demonstrating browser-to-server P2P communication using Rust libp2p with WebRTC transport

## Quick Start

```bash
cd simple-p2p-docstore
wasm-pack build --target web --out-dir pkg
cp -r pkg/* www/pkg/
SIGNALING_PORT=9090 cargo run --release --bin server
# In another terminal:
basic-http-server www -a 127.0.0.1:8080
```

Open http://127.0.0.1:8080 and connect using the server's multiaddr.

See [`simple-p2p-docstore/README.md`](simple-p2p-docstore/README.md) for detailed instructions.

## Features

- Native Rust server with TCP and WebRTC transports
- WASM browser client using WebRTC
- GossipSub messaging protocol
- Circuit Relay v2 support (planned)

## Documentation

Technical guides and implementation details are in [`docs/tech_guides/`](docs/tech_guides/).
