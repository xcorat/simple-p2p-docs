#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::collections::HashMap;

use futures::{channel::mpsc, stream::StreamExt, task::AtomicWaker};
use js_sys::{Object, Reflect};
use libp2p::{
    gossipsub::{self},
    identify, identity, ping,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, StreamProtocol, Swarm,
    multiaddr::Protocol,
};
use libp2p_kad::{Behaviour as KademliaBehaviour, store::MemoryStore, Event as KademliaEvent, QueryResult};
use libp2p_webrtc_websys::browser::Behaviour as WebRTCBehaviour;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::node::{NodeBuilder, NodeRole};
use crate::wasm_transport::{TransportConfig, build_composite_transport};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Initialize panic hook for better error messages in browser console
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Extract peer ID from a multiaddr if present
fn extract_peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    for protocol in addr.iter() {
        if let libp2p::multiaddr::Protocol::P2p(peer_id) = protocol {
            return Some(peer_id);
        }
    }
    None
}

/// Get current timestamp in milliseconds
fn get_timestamp_ms() -> f64 {
    js_sys::Date::now()
}

// Direct message request/response types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DirectMessage {
    data: Vec<u8>,
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    relay: libp2p_relay::client::Behaviour,
    webrtc: WebRTCBehaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
    kademlia: KademliaBehaviour<MemoryStore>,
    request_response: request_response::cbor::Behaviour<DirectMessage, DirectMessage>,
}

enum Command {
    Publish(Vec<u8>),
    FindPeer(libp2p::PeerId),
    SendDirect { peer_id: libp2p::PeerId, data: Vec<u8> },
    ListenOnRelay { relay_addr: Multiaddr },
    ListenForWebRTC,
    DialPeer { addr: Multiaddr },
}

#[derive(Debug, Clone)]
enum Event {
    Connected { peer_id: String },
    Disconnected { peer_id: String },
    MessageReceived { peer_id: String, data: String },
    MessagePublished { msg_id: String },
    PeerDiscovery { peer_id: String, addrs: Vec<String> },
    DirectMessageReceived { peer_id: String, data: String },
    DirectMessageSent { peer_id: String },
    ListenStarted { addr: String },
    RelayReservationCreated { addr: String },
    RelayConnectionEstablished { peer_id: String },
    WebRTCConnectionEstablished { peer_id: String },
    Error { msg: String },
}

// Relay information with connection tracking
#[derive(Debug, Clone)]
struct RelayInfo {
    peer_id: String,
    full_addr: String,
    connected_at: f64, // timestamp in milliseconds
    supports_relay: bool,
}

// Shared state for network status
#[derive(Debug, Clone, Default)]
struct SharedState {
    listen_addrs: Vec<String>,
    connected_peers: HashMap<String, Vec<String>>,
    discovered_peers: HashMap<String, Vec<String>>,
    subscriptions: Vec<String>,
    relays: Vec<RelayInfo>,
}

#[wasm_bindgen]
pub struct WasmNode {
    cmd_sender: mpsc::UnboundedSender<Command>,
    event_receiver: Arc<futures::lock::Mutex<mpsc::UnboundedReceiver<Event>>>,
    peer_id: String,
    shared_state: Arc<futures::lock::Mutex<SharedState>>,
}

#[wasm_bindgen]
impl WasmNode {
    #[wasm_bindgen(constructor)]
    pub fn new(server_multiaddr: String) -> Result<WasmNode, JsValue> {
        // Create local identity
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        log(&format!("local peer id: {}", local_peer_id));

        // Create transport waker for WebRTC transport
        let transport_waker = Arc::new(AtomicWaker::new());

        // Build composite transport using our modular builder
        let transport_config = TransportConfig::new(local_key.clone());
        let (final_transport, webrtc_behaviour, relay_behaviour) = 
            build_composite_transport(transport_config, transport_waker)
                .map_err(|e| JsValue::from_str(&format!("transport build error: {e:?}")))?;

        // Build behaviours using NodeBuilder (for ping, gossipsub, identify, kademlia)
        let (ping_beh, gossipsub_beh, identify_beh, kademlia_beh) = 
            NodeBuilder::new(NodeRole::Client).build_behaviours(&local_key);
        
        // Create request-response behaviour for direct messaging
        let req_resp_beh = request_response::cbor::Behaviour::<DirectMessage, DirectMessage>::new(
            [(StreamProtocol::new("/docstore/direct-message/1.0.0"), ProtocolSupport::Full)],
            request_response::Config::default(),
        );

        // Compose all behaviours including relay and WebRTC
        let behaviour = MyBehaviour {
            relay: relay_behaviour,
            webrtc: webrtc_behaviour,
            ping: ping_beh,
            gossipsub: gossipsub_beh,
            identify: identify_beh,
            kademlia: kademlia_beh,
            request_response: req_resp_beh,
        };

        // Build swarm manually (not via SwarmBuilder) because we have custom composite transport
        let mut swarm = Swarm::new(
            final_transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_executor(Box::new(|fut| {
                wasm_bindgen_futures::spawn_local(fut);
            }))
            .with_idle_connection_timeout(std::time::Duration::from_secs(300)),
        );

        // Subscribe to docstore topic using behaviour helper
        crate::behaviour::docstore::subscribe(&mut swarm.behaviour_mut().gossipsub)
            .map_err(|e| JsValue::from_str(&format!("subscribe error: {e}")))?;
        log("✓ Subscribed to topic: docstore/v1/updates");
        
        // Initialize shared state
        let shared_state = Arc::new(futures::lock::Mutex::new(SharedState {
            subscriptions: vec!["docstore/v1/updates".to_string()],
            ..Default::default()
        }));
        let shared_state_clone = shared_state.clone();

        // Dial the server (webrtc-direct or websocket multiaddr)
        let addr: Multiaddr = server_multiaddr
            .parse()
            .map_err(|e| JsValue::from_str(&format!("invalid multiaddr: {e}")))?;
        
        // Extract potential relay peer ID from the server address
        let relay_peer_id_opt = extract_peer_id_from_multiaddr(&addr);
        if let Some(relay_peer_id) = relay_peer_id_opt {
            log(&format!("Detected relay peer: {}", relay_peer_id));
            // Store relay info immediately (will be validated on connection)
            let mut state = shared_state.try_lock().expect("lock shared state");
            state.relays.push(RelayInfo {
                peer_id: relay_peer_id.to_string(),
                full_addr: addr.to_string(),
                connected_at: get_timestamp_ms(),
                supports_relay: false, // Will be validated on Identify event
            });
        } else {
            log("Warning: Server address does not contain peer ID - relay functionality may be limited");
        }
        
        log(&format!("dialing {}", addr));
        swarm.dial(addr.clone())
            .map_err(|e| JsValue::from_str(&format!("dial error: {e}")))?;

        // Create command and event channels
        #[allow(clippy::disallowed_methods)]
        let (cmd_sender, mut cmd_receiver) = mpsc::unbounded();
        #[allow(clippy::disallowed_methods)]
        let (event_sender, event_receiver) = mpsc::unbounded();

        // Store local_peer_id for later use in event loop
        let local_peer_id_for_events = local_peer_id;

        // Spawn the event loop - swarm is moved in and owned by this task
        spawn_local(async move {
            let topic = crate::behaviour::docstore::docstore_topic();
            let mut relay_address: Option<Multiaddr> = None;
            let mut webrtc_listening = false;
            
            loop {
                futures::select! {
                    cmd = cmd_receiver.select_next_some() => {
                        match cmd {
                            Command::Publish(data) => {
                                match swarm.behaviour_mut()
                                    .gossipsub
                                    .publish(topic.clone(), data) {
                                    Ok(msg_id) => {
                                        log(&format!("Published message: {:?}", msg_id));
                                        let _ = event_sender.unbounded_send(Event::MessagePublished {
                                            msg_id: format!("{:?}", msg_id)
                                        });
                                    }
                                    Err(e) => {
                                        log(&format!("Publish error: {}", e));
                                        let _ = event_sender.unbounded_send(Event::Error {
                                            msg: format!("Publish error: {}", e)
                                        });
                                    }
                                }
                            }
                            Command::FindPeer(pid) => {
                                let qid = swarm.behaviour_mut().kademlia.get_closest_peers(pid.clone());
                                log(&format!("Started find_peer query {:?} for {}", qid, pid.to_string()));
                            }
                            Command::SendDirect { peer_id, data } => {
                                let msg = DirectMessage { data };
                                let req_id = swarm.behaviour_mut().request_response.send_request(&peer_id, msg);
                                log(&format!("Sent direct message request {:?} to {}", req_id, peer_id));
                            }
                            Command::ListenOnRelay { relay_addr } => {
                                relay_address = Some(relay_addr.clone());
                                // Build the circuit address for reservation
                                let circuit_addr = relay_addr.with(Protocol::P2pCircuit);
                                
                                log(&format!("Attempting to listen on relay circuit: {}", circuit_addr));
                                match swarm.listen_on(circuit_addr.clone()) {
                                    Ok(listener_id) => {
                                        log(&format!("✓ Relay circuit listener created: {:?}", listener_id));
                                    }
                                    Err(e) => {
                                        log(&format!("❌ Failed to listen on relay circuit: {}", e));
                                        let _ = event_sender.unbounded_send(Event::Error {
                                            msg: format!("Listen on relay failed: {}", e)
                                        });
                                    }
                                }
                            }
                            Command::ListenForWebRTC => {
                                if !webrtc_listening {
                                    let webrtc_listen_addr = "/webrtc".parse::<Multiaddr>().unwrap();
                                    
                                    log("Attempting to listen for incoming WebRTC connections...");
                                    match swarm.listen_on(webrtc_listen_addr.clone()) {
                                        Ok(listener_id) => {
                                            log(&format!("✓ WebRTC listener created: {:?}", listener_id));
                                            webrtc_listening = true;
                                        }
                                        Err(e) => {
                                            log(&format!("❌ Failed to create WebRTC listener: {}", e));
                                            let _ = event_sender.unbounded_send(Event::Error {
                                                msg: format!("WebRTC listen failed: {}", e)
                                            });
                                        }
                                    }
                                } else {
                                    log("⚠ WebRTC listener already active");
                                }
                            }
                            Command::DialPeer { addr } => {
                                let addr_str = addr.to_string();
                                
                                // Check if this is a browser-to-browser dial (contains /p2p-circuit and /webrtc)
                                if addr_str.contains("/p2p-circuit") && addr_str.contains("/webrtc") {
                                    log(&format!("🔗 Browser-to-browser dial: {}", addr));
                                    
                                    // Step 1: Dial relay circuit (for signaling channel)
                                    let relay_circuit_addr_str = addr_str.replace("/webrtc", "");
                                    log(&format!("  → Dialing relay circuit: {}", relay_circuit_addr_str));
                                    
                                    match relay_circuit_addr_str.parse::<Multiaddr>() {
                                        Ok(relay_circuit_addr) => {
                                            if let Err(e) = swarm.dial(relay_circuit_addr.clone()) {
                                                log(&format!("❌ Failed to dial relay circuit: {:?}", e));
                                                let _ = event_sender.unbounded_send(Event::Error {
                                                    msg: format!("Relay dial failed: {}", e)
                                                });
                                                continue;
                                            }
                                            
                                            // Step 2: Dial WebRTC address (triggers signaling)
                                            let peer_id_str = addr_str.split("/p2p/").last().unwrap_or("");
                                            let webrtc_addr_str = format!("/webrtc/p2p/{}", peer_id_str);
                                            log(&format!("  → Dialing WebRTC: {}", webrtc_addr_str));
                                            
                                            match webrtc_addr_str.parse::<Multiaddr>() {
                                                Ok(webrtc_addr) => {
                                                    if let Err(e) = swarm.dial(webrtc_addr) {
                                                        log(&format!("❌ Failed to dial WebRTC: {:?}", e));
                                                    }
                                                }
                                                Err(e) => {
                                                    log(&format!("❌ Invalid WebRTC multiaddr: {:?}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log(&format!("❌ Invalid relay circuit multiaddr: {:?}", e));
                                        }
                                    }
                                } else {
                                    // Simple direct dial (e.g., relay server via webrtc-direct)
                                    log(&format!("📞 Direct dial: {}", addr));
                                    if let Err(e) = swarm.dial(addr.clone()) {
                                        log(&format!("❌ Dial failed: {:?}", e));
                                        let _ = event_sender.unbounded_send(Event::Error {
                                            msg: format!("Dial failed: {}", e)
                                        });
                                    }
                                }
                            }
                        }
                    }
                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(beh_event) => {
                                log(&format!("Behaviour event: {:?}", beh_event));
                                
                                // Handle request-response separately to consume the channel
                                if let MyBehaviourEvent::RequestResponse(req_resp_evt) = beh_event {
                                    use request_response::Event as ReqRespEvent;
                                    match req_resp_evt {
                                        ReqRespEvent::Message { peer, message, .. } => {
                                            match message {
                                                request_response::Message::Request { request, channel, .. } => {
                                                    let data = String::from_utf8_lossy(&request.data).to_string();
                                                    log(&format!("Received direct message from {}: {}", peer, data));
                                                    let _ = event_sender.unbounded_send(Event::DirectMessageReceived {
                                                        peer_id: peer.to_string(),
                                                        data: data.clone(),
                                                    });
                                                    // Send acknowledgment response
                                                    let response = DirectMessage { data: b"ack".to_vec() };
                                                    if let Err(_resp) = swarm.behaviour_mut().request_response.send_response(channel, response) {
                                                        log(&format!("Failed to send response: response data lost"));
                                                    }
                                                }
                                                request_response::Message::Response { .. } => {
                                                    log(&format!("Received direct message response from {}", peer));
                                                    let _ = event_sender.unbounded_send(Event::DirectMessageSent {
                                                        peer_id: peer.to_string(),
                                                    });
                                                }
                                            }
                                        }
                                        ReqRespEvent::OutboundFailure { peer, error, .. } => {
                                            log(&format!("Direct message outbound failure to {:?}: {:?}", peer, error));
                                            let _ = event_sender.unbounded_send(Event::Error {
                                                msg: format!("Direct message failed: {:?}", error)
                                            });
                                        }
                                        ReqRespEvent::InboundFailure { peer, error, .. } => {
                                            log(&format!("Direct message inbound failure from {}: {:?}", peer, error));
                                        }
                                        _ => {}
                                    }
                                } else {
                                    // Handle other events by reference
                                    use gossipsub::Event as GossipsubEvent;
                                    match &beh_event {
                                        MyBehaviourEvent::Gossipsub(GossipsubEvent::Message { 
                                            propagation_source, 
                                            message, 
                                            .. 
                                        }) => {
                                            let data = String::from_utf8_lossy(&message.data).to_string();
                                            log(&format!("Received message from {}: {}", propagation_source, data));
                                            let _ = event_sender.unbounded_send(Event::MessageReceived {
                                                peer_id: propagation_source.to_string(),
                                                data,
                                            });
                                        }
                                        MyBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. }) => {
                                            log(&format!("Identify Received for peer {}: addresses: {:?}", peer_id, info.listen_addrs));
                                            
                                            // Check if this peer supports relay protocol
                                            let supports_relay = info.protocols.iter().any(|p| {
                                                p.as_ref().starts_with("/libp2p/circuit/relay")
                                            });
                                            
                                            if supports_relay {
                                                log(&format!("✓ Peer {} supports Circuit Relay", peer_id));
                                            } else {
                                                log(&format!("⚠ Peer {} does NOT support Circuit Relay", peer_id));
                                            }
                                            
                                            // Update relay info if this is a known relay, or add it if it supports relay
                                            let mut state = shared_state_clone.lock().await;
                                            if let Some(relay_info) = state.relays.iter_mut().find(|r| r.peer_id == peer_id.to_string()) {
                                                relay_info.supports_relay = supports_relay;
                                                relay_info.connected_at = get_timestamp_ms();
                                                
                                                if !supports_relay {
                                                    log(&format!("❌ ERROR: Server {} does not support relay functionality!", peer_id));
                                                    let _ = event_sender.unbounded_send(Event::Error {
                                                        msg: format!("Server does not support Circuit Relay protocol - browser-to-browser communication will not work")
                                                    });
                                                }
                                            } else if supports_relay {
                                                // Auto-detect and add new relay
                                                let peer_id_str = peer_id.to_string();
                                                
                                                // Choose best address from info.listen_addrs (prefer webrtc-direct)
                                                let full_addr = info.listen_addrs.iter()
                                                    .find(|addr| addr.to_string().contains("webrtc-direct"))
                                                    .or_else(|| info.listen_addrs.first())
                                                    .map(|addr| {
                                                        // Add peer ID to address if not present
                                                        let addr_str = addr.to_string();
                                                        if addr_str.contains(&peer_id_str) {
                                                            addr_str
                                                        } else {
                                                            format!("{}/p2p/{}", addr_str, peer_id_str)
                                                        }
                                                    })
                                                    .unwrap_or_else(|| peer_id_str.clone());
                                                
                                                state.relays.push(RelayInfo {
                                                    peer_id: peer_id_str.clone(),
                                                    full_addr: full_addr.clone(),
                                                    connected_at: get_timestamp_ms(),
                                                    supports_relay: true,
                                                });
                                                
                                                log(&format!("✓ Auto-detected and added relay: {} ({})", peer_id_str, full_addr));
                                            }
                                            
                                            // Add addresses to Kademlia
                                            for addr in &info.listen_addrs {
                                                swarm.behaviour_mut().kademlia.add_address(peer_id, addr.clone());
                                                log(&format!("Added address {} for peer {} to Kademlia", addr, peer_id));
                                            }
                                        }
                                        MyBehaviourEvent::Kademlia(evt) => {
                                            match evt {
                                                KademliaEvent::OutboundQueryProgressed { id, result, .. } => {
                                                    match result {
                                                        QueryResult::GetClosestPeers(Ok(ok)) => {
                                                            log(&format!("Kademlia get_closest_peers {:?} => {:?}", id, ok.peers));
                                                            let mut state = shared_state_clone.lock().await;
                                                            for p in ok.peers.iter() {
                                                                let addrs: Vec<String> = p.addrs.iter().map(|a| a.to_string()).collect();
                                                                let _ = event_sender.unbounded_send(Event::PeerDiscovery {
                                                                    peer_id: p.peer_id.to_string(),
                                                                    addrs: addrs.clone(),
                                                                });
                                                                // Update shared state
                                                                state.discovered_peers.insert(p.peer_id.to_string(), addrs);
                                                            }
                                                        }
                                                        QueryResult::GetClosestPeers(Err(err)) => {
                                                            log(&format!("Kademlia get_closest_peers {:?} error: {:?}", id, err));
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                _ => {
                                                    log(&format!("Kademlia event: {:?}", evt));
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                                let remote_addr = endpoint.get_remote_address().to_string();
                                
                                // Distinguish between different connection types
                                if remote_addr.contains("/webrtc") && !remote_addr.contains("/p2p-circuit") {
                                    log(&format!("✅ Direct WebRTC connection established with {}", peer_id));
                                    let _ = event_sender.unbounded_send(Event::WebRTCConnectionEstablished {
                                        peer_id: peer_id.to_string()
                                    });
                                } else if remote_addr.contains("/p2p-circuit") {
                                    log(&format!("🔗 Relay connection established with {} via {}", peer_id, remote_addr));
                                    let _ = event_sender.unbounded_send(Event::RelayConnectionEstablished {
                                        peer_id: peer_id.to_string()
                                    });
                                } else {
                                    log(&format!("Connected to {peer_id}"));
                                    let _ = event_sender.unbounded_send(Event::Connected {
                                        peer_id: peer_id.to_string()
                                    });
                                }
                                
                                // Update shared state
                                let mut state = shared_state_clone.lock().await;
                                let addrs = vec![remote_addr];
                                state.connected_peers.insert(peer_id.to_string(), addrs);
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                log(&format!("Disconnected from {peer_id}"));
                                let _ = event_sender.unbounded_send(Event::Disconnected {
                                    peer_id: peer_id.to_string()
                                });
                                // Update shared state
                                let mut state = shared_state_clone.lock().await;
                                state.connected_peers.remove(&peer_id.to_string());
                            }
                            SwarmEvent::NewListenAddr { address, .. } => {
                                log(&format!("Listening on {address}"));
                                
                                // Check if this is a relay reservation (contains P2pCircuit)
                                if address.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
                                    // This is our relay reservation! Construct the full WebRTC address
                                    if let Some(ref relay_addr) = relay_address {
                                        let webrtc_reservation_addr = format!(
                                            "{}/p2p-circuit/webrtc/p2p/{}",
                                            relay_addr,
                                            local_peer_id_for_events
                                        );
                                        log(&format!("🎉 Relay reservation created: {}", webrtc_reservation_addr));
                                        let _ = event_sender.unbounded_send(Event::RelayReservationCreated {
                                            addr: webrtc_reservation_addr
                                        });
                                    }
                                }
                                
                                // Update shared state
                                let mut state = shared_state_clone.lock().await;
                                let addr_str = address.to_string();
                                if !state.listen_addrs.contains(&addr_str) {
                                    state.listen_addrs.push(addr_str);
                                }
                            }
                            SwarmEvent::Dialing { peer_id, .. } => {
                                log(&format!("Dialing {:?}", peer_id));
                            }
                            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                                log(&format!("Connection error to {:?}: {}", peer_id, error));
                                let _ = event_sender.unbounded_send(Event::Error {
                                    msg: format!("Connection error: {}", error)
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(WasmNode {
            cmd_sender,
            event_receiver: Arc::new(futures::lock::Mutex::new(event_receiver)),
            peer_id: local_peer_id.to_string(),
            shared_state,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn peer_id(&self) -> String {
        self.peer_id.clone()
    }

    #[wasm_bindgen]
    pub fn publish_update(&self, data: String) -> Result<(), JsValue> {
        let bytes = data.into_bytes();
        self.cmd_sender
            .unbounded_send(Command::Publish(bytes))
            .map_err(|e| JsValue::from_str(&format!("Failed to send command: {}", e)))
    }

    #[wasm_bindgen]
    pub fn find_peer(&self, peer_id: String) -> Result<(), JsValue> {
        let pid: PeerId = peer_id
            .parse()
            .map_err(|e| JsValue::from_str(&format!("invalid peer id: {e}")))?;
        // Run the query via kademlia
        match self.cmd_sender.unbounded_send(Command::FindPeer(pid)) {
            Ok(_) => Ok(()),
            Err(e) => Err(JsValue::from_str(&format!("Failed to send find peer command: {}", e))),
        }
    }

    /// Listen on relay circuit (for incoming browser-to-browser connections)
    /// relay_multiaddr: e.g., "/ip4/127.0.0.1/udp/9090/webrtc-direct/certhash/<hash>/p2p/<relay-id>"
    #[wasm_bindgen]
    pub fn listen_on_relay(&self, relay_multiaddr: String) -> Result<(), JsValue> {
        let addr: Multiaddr = relay_multiaddr
            .parse()
            .map_err(|e| JsValue::from_str(&format!("invalid relay multiaddr: {e}")))?;
        
        self.cmd_sender
            .unbounded_send(Command::ListenOnRelay { relay_addr: addr })
            .map_err(|e| JsValue::from_str(&format!("Failed to send listen on relay command: {}", e)))
    }

    /// Start listening for incoming WebRTC connections (call after listen_on_relay)
    #[wasm_bindgen]
    pub fn listen_for_webrtc(&self) -> Result<(), JsValue> {
        self.cmd_sender
            .unbounded_send(Command::ListenForWebRTC)
            .map_err(|e| JsValue::from_str(&format!("Failed to send listen for webrtc command: {}", e)))
    }

    /// Dial a peer using browser-to-browser WebRTC via relay
    /// peer_addr: e.g., "/ip4/.../p2p/<relay-id>/p2p-circuit/webrtc/p2p/<peer-id>"
    #[wasm_bindgen]
    pub fn dial_peer(&self, peer_addr: String) -> Result<(), JsValue> {
        let addr: Multiaddr = peer_addr
            .parse()
            .map_err(|e| JsValue::from_str(&format!("invalid peer multiaddr: {e}")))?;
        
        self.cmd_sender
            .unbounded_send(Command::DialPeer { addr })
            .map_err(|e| JsValue::from_str(&format!("Failed to send dial peer command: {}", e)))
    }

    /// Legacy method for backward compatibility - automatically detects relay from connected peers
    #[wasm_bindgen]
    pub fn start_listen(&self) -> Result<(), JsValue> {
        // For backward compatibility, we'll try to auto-detect relay
        // In the new implementation, users should call listen_on_relay() followed by listen_for_webrtc()
        log("⚠ start_listen() is deprecated. Please use listen_on_relay() and listen_for_webrtc()");
        log("ℹ For now, please manually specify the relay address using listen_on_relay()");
        Err(JsValue::from_str("Please use listen_on_relay(relay_addr) instead"))
    }

    #[wasm_bindgen]
    pub fn send_direct(&self, peer_id: String, data: String) -> Result<(), JsValue> {
        let pid: PeerId = peer_id
            .parse()
            .map_err(|e| JsValue::from_str(&format!("invalid peer id: {e}")))?;
        let bytes = data.into_bytes();
        self.cmd_sender
            .unbounded_send(Command::SendDirect { peer_id: pid, data: bytes })
            .map_err(|e| JsValue::from_str(&format!("Failed to send direct message command: {}", e)))
    }

    #[wasm_bindgen]
    pub async fn next_event(&self) -> Result<JsValue, JsValue> {
        let mut receiver = self.event_receiver.lock().await;
        if let Some(event) = receiver.next().await {
            let obj = Object::new();
            match event {
                Event::Connected { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"connected".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                }
                Event::Disconnected { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"disconnected".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                }
                Event::MessageReceived { peer_id, data } => {
                    Reflect::set(&obj, &"type".into(), &"messageReceived".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                    Reflect::set(&obj, &"data".into(), &data.into())?;
                }
                Event::MessagePublished { msg_id } => {
                    Reflect::set(&obj, &"type".into(), &"messagePublished".into())?;
                    Reflect::set(&obj, &"msg_id".into(), &msg_id.into())?;
                }
                Event::PeerDiscovery { peer_id, addrs } => {
                    Reflect::set(&obj, &"type".into(), &"peerDiscovery".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                    let js_arr = js_sys::Array::new();
                    for a in addrs.iter() {
                        js_arr.push(&JsValue::from_str(a));
                    }
                    Reflect::set(&obj, &"addrs".into(), &js_arr.into())?;
                }
                Event::DirectMessageReceived { peer_id, data } => {
                    Reflect::set(&obj, &"type".into(), &"directMessageReceived".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                    Reflect::set(&obj, &"data".into(), &data.into())?;
                }
                Event::DirectMessageSent { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"directMessageSent".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                }
                Event::ListenStarted { addr } => {
                    Reflect::set(&obj, &"type".into(), &"listenStarted".into())?;
                    Reflect::set(&obj, &"addr".into(), &addr.into())?;
                }
                Event::RelayReservationCreated { addr } => {
                    Reflect::set(&obj, &"type".into(), &"relayReservationCreated".into())?;
                    Reflect::set(&obj, &"addr".into(), &addr.into())?;
                }
                Event::RelayConnectionEstablished { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"relayConnectionEstablished".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                }
                Event::WebRTCConnectionEstablished { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"webrtcConnectionEstablished".into())?;
                    Reflect::set(&obj, &"peer_id".into(), &peer_id.into())?;
                }
                Event::Error { msg } => {
                    Reflect::set(&obj, &"type".into(), &"error".into())?;
                    Reflect::set(&obj, &"msg".into(), &msg.into())?;
                }
            }
            Ok(obj.into())
        } else {
            Err(JsValue::from_str("No events available"))
        }
    }

    #[wasm_bindgen]
    pub async fn get_network_status(&self) -> Result<JsValue, JsValue> {
        let state = self.shared_state.lock().await;
        let obj = Object::new();
        
        // Convert listen_addrs
        let listen_addrs = js_sys::Array::new();
        for addr in &state.listen_addrs {
            listen_addrs.push(&JsValue::from_str(addr));
        }
        Reflect::set(&obj, &"listen_addrs".into(), &listen_addrs.into())?;
        
        // Convert connected_peers (HashMap<String, Vec<String>>)
        let connected_peers = Object::new();
        for (peer_id, addrs) in &state.connected_peers {
            let addrs_arr = js_sys::Array::new();
            for addr in addrs {
                addrs_arr.push(&JsValue::from_str(addr));
            }
            Reflect::set(&connected_peers, &peer_id.as_str().into(), &addrs_arr.into())?;
        }
        Reflect::set(&obj, &"connected_peers".into(), &connected_peers.into())?;
        
        // Convert discovered_peers (HashMap<String, Vec<String>>)
        let discovered_peers = Object::new();
        for (peer_id, addrs) in &state.discovered_peers {
            let addrs_arr = js_sys::Array::new();
            for addr in addrs {
                addrs_arr.push(&JsValue::from_str(addr));
            }
            Reflect::set(&discovered_peers, &peer_id.as_str().into(), &addrs_arr.into())?;
        }
        Reflect::set(&obj, &"discovered_peers".into(), &discovered_peers.into())?;
        
        // Convert subscriptions
        let subscriptions = js_sys::Array::new();
        for sub in &state.subscriptions {
            subscriptions.push(&JsValue::from_str(sub));
        }
        Reflect::set(&obj, &"subscriptions".into(), &subscriptions.into())?;
        
        // Convert relays (Vec<RelayInfo>)
        let relays = js_sys::Array::new();
        for relay in &state.relays {
            let relay_obj = Object::new();
            Reflect::set(&relay_obj, &"peer_id".into(), &JsValue::from_str(&relay.peer_id))?;
            Reflect::set(&relay_obj, &"full_addr".into(), &JsValue::from_str(&relay.full_addr))?;
            Reflect::set(&relay_obj, &"connected_at".into(), &JsValue::from_f64(relay.connected_at))?;
            Reflect::set(&relay_obj, &"supports_relay".into(), &JsValue::from_bool(relay.supports_relay))?;
            relays.push(&relay_obj.into());
        }
        Reflect::set(&obj, &"relays".into(), &relays.into())?;
        
        Ok(obj.into())
    }
}
