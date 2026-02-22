# Technical Progress Report - Simple P2P Docstore

**Date**: December 12, 2025  
**Project**: Simple P2P Docstore  
**Status**: Implementation Phase — Core P2P features implemented; Kademlia & messaging operational; next priorities: persistence & relay support

---

## Completed Milestones

### 1. Core Infrastructure (Phase 1)
- ✅ **Browser-to-Server WebRTC Connectivity**: Successfully implemented using libp2p WebRTC transport (native server uses libp2p-webrtc; browser uses libp2p-webrtc-websys).
- ✅ **WASM Client**: Rust WASM module implemented in `src/lib.rs` with WebRTC transport, Kademlia client mode, event/command channels, and JS interop via `#[wasm_bindgen]`.
- ✅ **Native Server**: Rust server (`src/bin/server.rs`) supporting both TCP and WebRTC transports, identify and GossipSub behaviours, and Kademlia in server mode.
- ✅ **GossipSub Messaging**: Pub/sub messaging functional; messages can be published from the browser, logged on both client and server, and seen in the web UI.
- ✅ **Kademlia DHT (basic)**: Kademlia implemented (server & client modes) with identify-driven address discovery and `get_closest_peers` support. Server bootstrap via BOOTSTRAP_PEERS is available.
- ✅ **Find Peer UI**: `find_peer` implementation available in WASM and exposed in the web UI (Kademlia `get_closest_peers` queries produce PeerDiscovery events).
- ✅ **Build Pipeline**: `wasm-pack` build configured; `pkg/` output is usable with the web UI (manual copy from `pkg/` to `www/pkg/` currently required).
- ✅ **Headless Browser Test**: `test-wasm-browser.js` (puppeteer) script added for basic headless testing of the WASM client against a local server.

### 2. Architecture & Planning
- ✅ **Initial Plan**: Basic implementation plan documented and sample workflows defined.
- ✅ **Comparative Analysis**: Comparison notes with OrbitDB, Nostr, Holochain and others are documented for architectural decisions.
- ✅ **Technical Documentation**: Build conventions and AI-agent playbooks (`.github/copilot-instructions.md`) are in place for contributors.

---

## Current Architecture

### Components Implemented
1. **WASM Client** (`src/lib.rs`)
   - WebRTC transport for browser using `libp2p-webrtc-websys`.
   - Global state management with `Arc<futures::lock::Mutex<SharedState>>`, unbounded channels for commands/events, and `next_event()`/`get_network_status()` exported to JavaScript.
   - GossipSub configuration and Kademlia client mode (MemoryStore);
   - `publish_update`, `find_peer`, `next_event`, and `get_network_status` are exported via `#[wasm_bindgen]` for JS integration.

2. **Native Server** (`src/bin/server.rs`)
   - Dual transport: TCP (native↔native with Noise & Yamux) + WebRTC (browser↔native via `libp2p-webrtc`); the server listens on TCP and a WebRTC-direct UDP port (default 9090) for browser clients.
   - GossipSub pub/sub with message logging and subscribe/unsubscribe events printed to STDOUT, making it easy to retrieve multiaddr / certhash required for dialing.
   - Kademlia server mode (MemoryStore by default), with bootstrapping support via `BOOTSTRAP_PEERS` env var and identify-driven Kademlia address population.
   - Event-driven architecture built on `Swarm` with `SwarmEvent` processing loop.

3. **Web Interface** (`www/`)
   - Simple HTML/JS frontend for testing (`index.html` + `main.js`).
   - Connect form (dial server `multiaddr`) and message publishing UI; Find Peer functionality and real-time network status view.
   - Event polling loop via `next_event()`, and network status polling calling `get_network_status()`; UI shows listen addresses, connected/discovered peers, subscriptions and relays.

### Technology Stack
- **Rust**: Core language for native & WASM implementations
- **libp2p (custom fork)**: WebRTC support via a fork (PR #5978 branch) for browser-to-browser and browser-to-server transport
- **libp2p-webrtc-websys**: Browser transport for the WASM client
- **wasm-pack**: WASM compilation and packaging
- **GossipSub**: Messaging protocol for pub/sub
- **Kademlia**: DHT-based peer discovery (MemoryStore used as initial implementation)

---

## Planned Components (Not Yet Implemented / Ongoing)

### Phase 2: Signaling & Peer Discovery (Ongoing)
- ✅ **Kademlia DHT (basic)**: Implemented — client & server modes are functional for address discovery and queries; server-side bootstrap option available.
- ⏳ **Bootstrap Relays & Peer List Management**: Robust bootstrap tooling, persistent peer lists, and improved peer management still to be implemented and validated across networks.

### Phase 3: User Profile System
- ⏳ **User Profile DHT Relay**: Lightweight, optional storage for user metadata
- ⏳ **Identity Management**: Public-facing identifiers, namespaces, and basic profile attributes
- ⏳ **Profile Data Separation**: Static profile fields vs. dynamic data (design & API)

### Phase 4: Content-Addressed Storage
- ⏳ **Docstore DHT**: Full content-addressed document storage backed by the DHT
- ⏳ **Namespace Model**: Mutable/immutable field separation and namespace ID derivation from immutable fields

### Phase 5: Relay & Service Infrastructure
- ⏳ **Circuit Relay v2**: Relay support for browser↔browser comms (server relay behaviour)
- ⏳ **Relay Optimization**: Performance improvements at scale and more robust relay discovery
- ⏳ **Custom network support**: Ability to spin up isolated networks (e.g., dev/test)

---

## Technical Decisions

### 1. Custom libp2p Fork
- Using branch: `feat(webrtc)-implement-webrtc-protocol-for-browser-to-browser-communication` (custom fork).
- Reason: Browser-to-browser WebRTC support not yet in upstream; we rely on the fork to get `libp2p-webrtc-websys` & `libp2p-webrtc` features in place.
- Risk: Dependency on an unmerged upstream PR (PR #5978). Mitigation: Plan to migrate to upstream once PR merges, and implement library-agnostic abstraction where practical.

### 2. DHT-Based Architecture
- Kademlia DHT for distributed key-value storage and peer discovery; MemoryStore used initially for simplicity and to demonstrate discovery features.
- Aligns with IPFS/OrbitDB patterns but makes network creation independent by default.

### 3. Modular Relay Design
- Relays are planned as optional modular services that provide signaling, storage, and discovery.
- Design fits a lightweight Nostr-like relay model but augmented with persistent storage for data retention where necessary.

---

## Known Issues & Limitations

### Current Limitations
1. **No Persistence**: State is primarily memory-backed; the server persists only identity key files when `IDENTITY_KEY_PATH` is set. Kademlia uses MemoryStore and does not persist entries.
2. **Single Topic**: The system hardcodes a single GossipSub topic (`docstore/v1/updates`) and lacks a schema for message types and subtopics.
3. **No Relay Support**: Circuit Relay v2 and dedicated relay behaviours are not yet implemented, so browser→browser connectivity is not yet fully supported.
4. **No Error Recovery**: Limited reconnection logic; browser client requires a page reload for recovery from network failures or some error conditions.
5. **Manual WASM Packaging**: `wasm-pack` is in use, but artifacts must be copied from `pkg/` to `www/pkg/` manually; build automation is required.
6. **Limited Automated Tests / CI**: There is a headless test script (puppeteer) available, but tests are not integrated into a CI pipeline.

### Technical Debt
- Automate `wasm-pack` build, copy artifacts into `www/pkg/`, and integrate into CI for quick test/dev cycles.
- Add persistent docstore storage (DHT-backed content addressed store) and migrate server MemoryStore to a persistent approach for nodes that require durability.
- Implement Circuit Relay v2 server behaviour and browser client support (with relay reservation & discovery mechanisms).
- Create automated connection testing (CI) using headless browser tests and add unit/integration coverage for critical code paths.
- Improve error handling and reconnection/backoff strategies for the browser client & server.

---

## Comparative Analysis Summary

### vs. OrbitDB
- **Advantage**: Not tied to IPFS infrastructure; allows independent networks and modular relay services.
- **Advantage**: Lightweight, easier to iterate and deploy for application-specific networks.
- **Trade-off**: Less mature than OrbitDB’s union with IPFS; fewer out-of-the-box storage abstractions.

### vs. Nostr
- **Compatible**: The architecture can host Nostr-style protocols on top of the DHT and relays.
- **Advantage**: Adds potential persistent storage beyond ephemeral relay models.

### vs. Holochain
- **Different Model**: Shared DHT-based storage vs. agent-centric chain models; this implementation prioritizes simplicity and independent networks.
- **Trade-off**: Less emphasis on per-agent chain data integrity and validation.

---

## Next Steps

### Immediate Priorities (Next 1-2 weeks)
1. Implement Circuit Relay v2 support (server behaviour + browser client support) to enable browser↔browser connectivity and fallbacks for NAT traversal.
2. Design & implement persistent docstore (content-addressed storage) and integrate with the DHT (Phase 4 work).
3. Automate wasm build & packaging and integrate headless test script into CI (e.g., GitHub Actions or similar).
4. Add improved reconnect & error handling for the browser client (exponential backoff, retry/queueing behavior).
5. Add example end-to-end browser-to-browser demo and documentation for testing across NATs.

### Short-term Goals (Next 2-4 weeks)
1. Harden Kademlia bootstrap & peer management; add persistent bootstrap address lists and better query behavior.
2. Begin Phase 3 (User Profile System) with lightweight metadata relay and identity management design.
3. Implement and validate relay functionality using the server as relay for a small set of browser clients.

### Long-term Goals (1-3 months)
1. Complete content-addressed docstore implementation with namespaced storage & replication strategies.
2. Implement a namespace model (mutable/immutable fields & namespace ID derivation).
3. Performance optimization for relay nodes and large DHT partitions.
4. Expand documentation, tutorials, and example applications demonstrating cross-browser sync and multi-user workflows.

---

## Metrics

- **Lines of Code**: ~665 (Rust: `src/lib.rs` ~407 lines WASM, `src/bin/server.rs` ~258 lines), ~341 (JS: `www/main.js` ~188 lines, `www/index.html` ~69, `test-wasm-browser.js` ~84)
- **Build Time**: ~30 seconds (WASM on local machine), ~2 minutes (native release build locally; Docker image build time varies)
- **Test Coverage**: Manual testing with a headless puppeteer script; no automated CI coverage yet
- **Documentation**: README, copilot instructions, and inline code comments provide basic build/run instructions; more developer/architecture docs needed

---

## Resources & References

- Custom libp2p fork: https://github.com/elijahhampton/rust-libp2p (branch: feat(webrtc)-implement-webrtc-protocol-for-browser-to-browser-communication)
- libp2p WebRTC guide: https://blog.libp2p.io/rust-libp2p-browser-to-server/
- wasm-pack documentation: https://rustwasm.github.io/wasm-pack/
- Test scripts: `test-wasm-browser.js` (puppeteer headless browser test)
- Docker & docker-compose configurations in repo (server Dockerfile and top-level `docker-compose.yml`)

---

## Team Notes
- Current working branch: feat/peerkdht
- Project is in a proof-of-concept stage with an emphasis on incremental development and demos.
- Focused next on relay support and durable docstore storage.
- Iterative, documentation-first approach to enable future contributors.

