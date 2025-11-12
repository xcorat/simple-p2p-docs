# AI Agent Instructions - Simple P2P Docstore

## Project Overview
This is a **libp2p WebRTC example** demonstrating browser-to-server P2P communication. It consists of:
- **Rust WASM client** (`src/lib.rs`) that runs in the browser using WebRTC transport
- **Native Rust server** (`src/bin/server.rs`) that bridges TCP and WebRTC transports
- **Simple HTML/JS frontend** (`www/`) for testing the connection

The codebase uses a **custom fork of rust-libp2p** (PR #5978) for browser-to-browser WebRTC support.

## Critical Build Commands

```bash
# Build WASM module (required before serving)
wasm-pack build --target web --out-dir pkg

# Copy WASM artifacts to www directory
cp -r pkg/* www/pkg/

# Run native server (listens on TCP + UDP 9090 for WebRTC)
SIGNALING_PORT=9090 cargo run --release --bin server

# Serve web app (use basic-http-server or any static server)
basic-http-server www -a 127.0.0.1:8080
```

**Important**: The WASM build outputs to `pkg/` but the web app expects files in `www/pkg/`. Always copy after building.

## Architecture Patterns

### Dual Transport Setup
The server supports **both TCP (native↔native) and WebRTC (browser↔native)**:
- TCP transport: `libp2p-tcp` with Noise encryption + Yamux multiplexing
- WebRTC transport: `libp2p-webrtc` for native, `libp2p-webrtc-websys` for WASM
- Uses `with_other_transport()` to combine transports in the SwarmBuilder

Browser clients **only use WebRTC** (no TCP available in WASM).

### Global State Management (WASM)
`src/lib.rs` uses `LazyLock<Mutex<Option<WasmState>>>` for thread-safe global state:
- Single `WasmState` holds the Swarm and topic subscription
- Event loop runs in `spawn_local()` and polls the global state
- JavaScript interacts via `#[wasm_bindgen]` functions like `start_node()` and `publish_update()`

**Pattern**: Always lock state briefly, do work, then release to avoid deadlocks in async context.

### GossipSub Configuration
Both client and server use identical GossipSub config:
- `ValidationMode::Strict` for message authenticity
- `MessageAuthenticity::Signed` using local keypair
- Topic: `"docstore/v1/updates"` (hardcoded)
- 1-second heartbeat interval

**Convention**: All P2P messages go through GossipSub on this topic. No custom protocols yet.

### WebRTC Connection Flow
1. Server prints multiaddr on startup (includes `/webrtc-direct/certhash/<hash>/p2p/<peer-id>`)
2. User pastes multiaddr into browser UI
3. Browser calls `wasm.start_node(addr)` → dials server
4. On `ConnectionEstablished`, both peers can publish/receive GossipSub messages

**Critical**: The multiaddr **must include the certhash** for WebRTC-direct to work.

## Code Conventions

### Conditional Compilation
Target-specific dependencies are split in `Cargo.toml`:
```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
libp2p-webrtc-websys = { ... }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
libp2p-webrtc = { ... }
libp2p-tcp = { ... }
```

**Pattern**: Use `#[cfg(target_arch = "wasm32")]` for WASM-specific code, `#[cfg(not(...))]` for native-only.

### Error Handling
WASM functions return `Result<JsValue, JsValue>` or `js_sys::Promise`:
- Use `.map_err(|e| JsValue::from_str(&format!("context: {e}")))` for descriptive errors
- Wrap async functions with `future_to_promise()` for JS interop
- Log errors to browser console before returning

Native server uses `anyhow::Result` for simpler error propagation.

### Swarm Event Loop Pattern
Both client and server use `swarm.select_next_some().await` in a loop:
- Match on `SwarmEvent` variants (ConnectionEstablished, Behaviour, etc.)
- WASM version uses `poll_fn()` because it can't await directly on the swarm (it's behind a mutex)
- Native version directly awaits on the swarm

**Key Events**:
- `NewListenAddr`: Print to stdout/console (needed to construct dial multiaddr)
- `ConnectionEstablished/Closed`: Log peer ID for debugging
- `Behaviour(...)`: Handle protocol events (currently unused but placeholder exists)

## Dependencies & Versions

All libp2p crates use **same git commit** from custom fork:
```toml
libp2p = { git = "https://github.com/elijahhampton/rust-libp2p", 
           branch = "feat(webrtc)-implement-webrtc-protocol-for-browser-to-browser-communication" }
```

**Critical**: This is a temporary solution until PR #5978 merges upstream. When upgrading libp2p:
1. Test browser→server connectivity first
2. Verify WebRTC multiaddr format hasn't changed
3. Check if `with_wasm_bindgen()` builder API is still correct

## Common Tasks

### Adding a New Protocol
1. Define protocol handler struct (e.g., `struct DocStoreProtocol { ... }`)
2. Add field to `MyBehaviour` (must derive `NetworkBehaviour`)
3. Initialize in `with_behaviour()` closure (both client and server)
4. Handle events in `SwarmEvent::Behaviour(MyBehaviourEvent::...)` match arm

### Debugging Connection Issues
1. Check server prints correct multiaddr with `/webrtc-direct/certhash/...`
2. Verify browser console shows "dialing ..." message
3. Look for "Connected to <peer_id>" in both logs
4. If no connection, check:
   - UDP port 9090 is not blocked
   - Server IP is correct (not 127.0.0.1 if testing remotely)
   - Multiaddr format is exact (copy-paste from server output)

### Testing GossipSub Messages
1. Connect browser to server
2. Open browser console
3. Call `wasm.publish_update("test message")` from console
4. Currently no subscriber logging - need to add in `SwarmEvent::Behaviour` handler

**TODO**: Implement message reception logging in both client and server.

## File Organization

```
simple-p2p-docstore/
├── src/
│   ├── lib.rs           # WASM client (browser-side)
│   └── bin/
│       └── server.rs    # Native server
├── www/
│   ├── index.html       # UI (connect form + publish input)
│   ├── main.js          # WASM loading & event handlers
│   └── pkg/             # WASM build output (copied from ../pkg/)
├── pkg/                 # wasm-pack build output
└── Cargo.toml           # Dependencies with custom libp2p fork
```

**Convention**: All P2P logic is in Rust. JavaScript is minimal glue code.

## Known Limitations

1. **No message reception display**: GossipSub messages are received but not logged/displayed
2. **No persistence**: All state is in-memory only
3. **Single topic**: Hardcoded to `"docstore/v1/updates"`
4. **No relay support**: Browser→Browser requires manual relay implementation
5. **No error recovery**: Connection failures require page reload

## Next Development Steps (from docs/tech_guides/)

**Phase 1** (current): Browser→Server messaging via GossipSub
- TODO: Add message reception logging/display in `SwarmEvent::Behaviour` handler for both client and server

**Phase 2**: Add document storage layer (key-value store)

**Phase 3**: Implement Circuit Relay v2 for browser↔browser
- Add relay protocol to server `MyBehaviour` struct
- Configure relay discovery and reservation in browser client
- Test direct browser-to-browser connections through relay
- Implement relay fallback for NAT traversal scenarios

See `docs/tech_guides/simple-example-proposal.md` for detailed architecture guide and implementation patterns.

## References

- libp2p WebRTC guide: https://blog.libp2p.io/rust-libp2p-browser-to-server/
- wasm-pack docs: https://rustwasm.github.io/wasm-pack/
- Custom fork: https://github.com/elijahhampton/rust-libp2p/tree/feat(webrtc)-implement-webrtc-protocol-for-browser-to-browser-communication
