use std::time::Duration;

use futures::prelude::*;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity};
use libp2p::identify;
use libp2p::identity;
use libp2p::noise;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
use libp2p_yamux as yamux;

#[cfg(not(target_arch = "wasm32"))]
use libp2p::{tcp, Transport};
#[cfg(not(target_arch = "wasm32"))]
use libp2p_webrtc as webrtc;

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    ping: libp2p::ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
}

fn mk_gossipsub(local_key: &identity::Keypair) -> gossipsub::Behaviour {
    let config = gossipsub::ConfigBuilder::default()
        .validation_mode(gossipsub::ValidationMode::Strict)
        .heartbeat_interval(Duration::from_secs(1))
        .build()
        .expect("valid gossipsub config");

    gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(local_key.clone()),
        config,
    )
    .expect("gossipsub")
}

fn mk_identify(local_pub: &identity::PublicKey) -> identify::Behaviour {
    let cfg = identify::Config::new("simple-p2p-docstore/0.1".to_string(), local_pub.clone());
    identify::Behaviour::new(cfg)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {}", local_peer_id);

    // Build swarm with the new builder API
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_other_transport(|local_key| {
            // WebRTC transport for browser connectivity
            Ok(webrtc::tokio::Transport::new(
                local_key.clone(),
                webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn))))
        })?
        .with_behaviour(|key| {
            Ok(MyBehaviour {
                ping: libp2p::ping::Behaviour::default(),
                gossipsub: mk_gossipsub(key),
                identify: mk_identify(&key.public()),
            })
        })?
        .build();

    // Listen on TCP random port
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Listen on WebRTC-direct UDP port (use WEBRTC_PORT env var, fallback to 9090)
    let webrtc_port: u16 = std::env::var("WEBRTC_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9090);
    let webrtc_addr: Multiaddr = format!("/ip4/0.0.0.0/udp/{}/webrtc-direct", webrtc_port).parse()?;
    swarm.listen_on(webrtc_addr.clone())?;

    let topic = IdentTopic::new("docstore/v1/updates");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
    println!("✓ Subscribed to topic: docstore/v1/updates");

    println!("Listening on TCP & WebRTC port {}", webrtc_port);

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("New listen addr: {}", address);
            }
            SwarmEvent::Behaviour(ev) => {
                match ev {
                    MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        propagation_source,
                        message_id,
                        message,
                    }) => {
                        let data = String::from_utf8_lossy(&message.data);
                        println!("📨 Received GossipSub message:");
                        println!("   From: {}", propagation_source);
                        println!("   ID: {:?}", message_id);
                        println!("   Topic: {:?}", message.topic);
                        println!("   Data: {}", data);
                    }
                    MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic }) => {
                        println!("✓ Peer {} subscribed to topic: {:?}", peer_id, topic);
                    }
                    MyBehaviourEvent::Gossipsub(gossipsub::Event::Unsubscribed { peer_id, topic }) => {
                        println!("✗ Peer {} unsubscribed from topic: {:?}", peer_id, topic);
                    }
                    _ => {
                        // Other events (ping, identify, etc.)
                        tracing::debug!("Behaviour event: {:?}", ev);
                    }
                }
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                println!("Connection established: {}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                println!("Connection closed: {}", peer_id);
            }
            _ => {}
        }
    }
}
