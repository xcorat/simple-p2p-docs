#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::collections::HashMap;

use futures::{channel::mpsc, prelude::*, stream::StreamExt};
use js_sys::{Object, Reflect};
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify, identity, ping,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_kad::{Behaviour as KademliaBehaviour, Config as KademliaConfig, store::MemoryStore, Mode, Event as KademliaEvent, QueryResult};
use libp2p_webrtc_websys;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::behaviour::{make_docstore_gossipsub, make_peer_dht};
use crate::node::{NodeBuilder, NodeRole};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
    kademlia: KademliaBehaviour<MemoryStore>,
}

enum Command {
    Publish(Vec<u8>),
    FindPeer(libp2p::PeerId),
}

#[derive(Debug, Clone)]
enum Event {
    Connected { peer_id: String },
    Disconnected { peer_id: String },
    MessageReceived { peer_id: String, data: String },
    MessagePublished { msg_id: String },
    PeerDiscovery { peer_id: String, addrs: Vec<String> },
    Error { msg: String },
}

// Shared state for network status
#[derive(Debug, Clone, Default)]
struct SharedState {
    listen_addrs: Vec<String>,
    connected_peers: HashMap<String, Vec<String>>,
    discovered_peers: HashMap<String, Vec<String>>,
    subscriptions: Vec<String>,
    relays: Vec<String>,
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

        // Build swarm using the new builder API - use with_other_transport for webrtc
        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
            .with_wasm_bindgen()
            .with_other_transport(|key| {
                libp2p_webrtc_websys::Transport::new(libp2p_webrtc_websys::Config::new(&key))
            })
            .map_err(|e| JsValue::from_str(&format!("transport build error: {e:?}")))?
            .with_behaviour(|key| {
                let (ping_beh, gossipsub_beh, identify_beh, kademlia_beh) = NodeBuilder::new(NodeRole::Client).build_behaviours(key);

                MyBehaviour {
                    ping: ping_beh,
                    gossipsub: gossipsub_beh,
                    identify: identify_beh,
                    kademlia: kademlia_beh,
                }
            })
            .map_err(|e| JsValue::from_str(&format!("behaviour build error: {e:?}")))?
            .build();

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
        log(&format!("dialing {}", addr));
        swarm.dial(addr.clone())
            .map_err(|e| JsValue::from_str(&format!("dial error: {e}")))?;

        // Create command and event channels
        #[allow(clippy::disallowed_methods)]
        let (cmd_sender, mut cmd_receiver) = mpsc::unbounded();
        #[allow(clippy::disallowed_methods)]
        let (event_sender, event_receiver) = mpsc::unbounded();

        // Spawn the event loop - swarm is moved in and owned by this task
        spawn_local(async move {
            let topic = crate::behaviour::docstore::docstore_topic();
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
                        }
                    }
                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(beh_event) => {
                                log(&format!("Behaviour event: {:?}", beh_event));
                                
                                // Handle gossipsub messages
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
                            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                                log(&format!("Connected to {peer_id}"));
                                let _ = event_sender.unbounded_send(Event::Connected {
                                    peer_id: peer_id.to_string()
                                });
                                // Update shared state
                                let mut state = shared_state_clone.lock().await;
                                let addrs = vec![endpoint.get_remote_address().to_string()];
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
}
