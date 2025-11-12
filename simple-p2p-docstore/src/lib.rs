#![allow(unused_imports)]
#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use futures::{channel::mpsc, prelude::*, stream::StreamExt};
use js_sys::{Object, Reflect};
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify, identity, ping,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_webrtc_websys;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

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
}

enum Command {
    Publish(Vec<u8>),
}

#[derive(Debug, Clone)]
enum Event {
    Connected { peer_id: String },
    Disconnected { peer_id: String },
    MessageReceived { peer_id: String, data: String },
    MessagePublished { msg_id: String },
    Error { msg: String },
}

#[wasm_bindgen]
pub struct WasmNode {
    cmd_sender: mpsc::UnboundedSender<Command>,
    event_receiver: Arc<futures::lock::Mutex<mpsc::UnboundedReceiver<Event>>>,
    peer_id: String,
}

#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
    let _ = tracing_wasm::set_as_global_default();
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
                // Gossipsub
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .validation_mode(ValidationMode::Strict)
                    .heartbeat_interval(std::time::Duration::from_secs(1))
                    .build()
                    .expect("valid gossipsub config");
                let gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                ).expect("gossipsub");

                // Identify
                let identify_config = identify::Config::new("simple-p2p-docstore/0.1".to_string(), key.public());
                let identify = identify::Behaviour::new(identify_config);

                MyBehaviour {
                    ping: ping::Behaviour::default(),
                    gossipsub,
                    identify,
                }
            })
            .map_err(|e| JsValue::from_str(&format!("behaviour build error: {e:?}")))?
            .build();

        // Subscribe to topic
        let topic = IdentTopic::new("docstore/v1/updates");
        swarm.behaviour_mut().gossipsub.subscribe(&topic)
            .map_err(|e| JsValue::from_str(&format!("subscribe error: {e}")))?;
        log("✓ Subscribed to topic: docstore/v1/updates");

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
                                    _ => {}
                                }
                            }
                            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                                log(&format!("Connected to {peer_id}"));
                                let _ = event_sender.unbounded_send(Event::Connected {
                                    peer_id: peer_id.to_string()
                                });
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                log(&format!("Disconnected from {peer_id}"));
                                let _ = event_sender.unbounded_send(Event::Disconnected {
                                    peer_id: peer_id.to_string()
                                });
                            }
                            SwarmEvent::NewListenAddr { address, .. } => {
                                log(&format!("Listening on {address}"));
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
