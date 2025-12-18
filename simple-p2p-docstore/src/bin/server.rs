use futures::prelude::*;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::gossipsub::{self};
use libp2p::identify;
use libp2p_kad::{Behaviour as KademliaBehaviour, store::MemoryStore, Event as KademliaEvent, QueryResult};
use libp2p::identity;
use std::path::{Path, PathBuf};
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use libp2p::noise;
use anyhow::Context;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
use libp2p_yamux as yamux;

use simple_p2p_docstore::behaviour::{make_docstore_gossipsub, make_peer_dht};
use simple_p2p_docstore::node::{NodeBuilder, NodeRole};

#[cfg(not(target_arch = "wasm32"))]
use libp2p::{tcp, Transport};
#[cfg(not(target_arch = "wasm32"))]
use libp2p_webrtc as webrtc;

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    ping: libp2p::ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
    kademlia: KademliaBehaviour<MemoryStore>,

    #[cfg(not(target_arch = "wasm32"))]
    relay: libp2p::relay::Behaviour,
}

// PeerDHT and DocStore behaviour are provided by `src/behaviour`

fn load_or_create_identity(path: &Path) -> anyhow::Result<identity::Keypair> {
    // Try to load existing keypair from disk (protobuf encoding). If not present, generate and persist.
    if path.exists() {
        let mut f = OpenOptions::new().read(true).open(path)
            .with_context(|| format!("failed to open identity key file: {}", path.display()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).with_context(|| format!("failed to read identity key file: {}", path.display()))?;
        if let Ok(kp) = identity::Keypair::from_protobuf_encoding(&buf) {
            tracing::info!("Loaded identity key from {}", path.display());
            return Ok(kp);
        } else {
            tracing::warn!("Failed to parse identity key from {} — generating new one", path.display());
        }
    }

    // Generate a new keypair and write it to disk.
    let kp = identity::Keypair::generate_ed25519();
    let bytes = kp.to_protobuf_encoding().context("failed to serialize identity key pair")?;
    // Ensure parent directory exists if the path has a parent
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("failed to create identity parent directory: {}", parent.display()))?;
    }
    // Create the file atomically and set permissions to 0o600 on unix.
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)] { opts.mode(0o600); }
    let mut f = opts.open(path).with_context(|| format!("failed to create identity key file: {}", path.display()))?;
    f.write_all(&bytes).with_context(|| format!("failed to write identity key file: {}", path.display()))?;
    tracing::info!("Generated new identity key and saved to {}", path.display());
    Ok(kp)
}

/// Returns the identity key path to use, giving precedence to the `IDENTITY_KEY_PATH` environment
/// variable. Otherwise default to ./ .p2p/identity.key in the process working directory.
fn get_identity_key_path() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("IDENTITY_KEY_PATH") {
        return Ok(PathBuf::from(p));
    }
    let cwd = std::env::current_dir().context("failed to determine current working directory")?;
    Ok(cwd.join(".p2p").join("identity.key"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let key_path_buf = get_identity_key_path()?;
    println!("Using identity key path: {}", key_path_buf.display());
    let local_key = load_or_create_identity(&key_path_buf)?;
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
            #[cfg(target_arch = "wasm32")]
            {
                let (ping_beh, gossipsub_beh, identify_beh, kademlia_beh) = NodeBuilder::new(NodeRole::Relay).build_behaviours(key);
                Ok(MyBehaviour {
                    ping: ping_beh,
                    gossipsub: gossipsub_beh,
                    identify: identify_beh,
                    kademlia: kademlia_beh,
                })
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                let (ping_beh, gossipsub_beh, identify_beh, kademlia_beh, relay_beh) = NodeBuilder::new(NodeRole::Relay).build_behaviours(key);
                Ok(MyBehaviour {
                    ping: ping_beh,
                    gossipsub: gossipsub_beh,
                    identify: identify_beh,
                    kademlia: kademlia_beh,
                    relay: relay_beh.expect("relay behaviour expected for relay role"),
                })
            }
        })?
        .build();

    // Listen on TCP random port
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Listen on WebRTC-direct UDP port (9090 by default)
    let udp_port: u16 = std::env::var("SIGNALING_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(9090);
    let webrtc_addr: Multiaddr = format!("/ip4/0.0.0.0/udp/{}/webrtc-direct", udp_port).parse()?;
    swarm.listen_on(webrtc_addr.clone())?;

    // Subscribe to the public docstore topic via behaviour helper
    simple_p2p_docstore::behaviour::docstore::subscribe(&mut swarm.behaviour_mut().gossipsub)?;
    println!("✓ Subscribed to topic: docstore/v1/updates");

    println!("Listening on TCP & WebRTC port {}", udp_port);

    // Bootstrap peers (if provided) - environment variable: BOOTSTRAP_PEERS (comma-separated multiaddrs)
    if let Ok(peers) = std::env::var("BOOTSTRAP_PEERS") {
        for p in peers.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            match p.parse::<Multiaddr>() {
                Ok(addr) => {
                    // Try to extract a PeerId from the multiaddr. If found, add it into Kademlia store; otherwise dial.
                    let mut peer_id_opt: Option<PeerId> = None;
                    for protocol in addr.iter() {
                        use libp2p::multiaddr::Protocol;
                        if let Protocol::P2p(pid) = protocol {
                            peer_id_opt = Some(pid);
                        }
                    }
                    if let Some(peer_id) = peer_id_opt {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                        println!("Added bootstrap address for {}: {}", peer_id, addr);
                    } else {
                        // Dial the address; this will eventually learn addresses from the peer via Identify
                        if let Err(e) = swarm.dial(addr.clone()) {
                            println!("Failed to dial bootstrap addr {}: {}", addr, e);
                        } else {
                            println!("Dialed bootstrap address: {}", addr);
                        }
                    }
                }
                Err(e) => {
                    println!("Invalid bootstrap multiaddr {}: {}", p, e);
                }
            }
        }
        // Kick off bootstrap query
        if let Err(e) = swarm.behaviour_mut().kademlia.bootstrap() {
            println!("Failed to bootstrap Kademlia: {}", e);
        } else {
            println!("Kademlia bootstrap started");
        }
    }

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
                        MyBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. }) => {
                            tracing::debug!("Identify Received for peer {}: addresses: {:?}", peer_id, info.listen_addrs);
                            for addr in info.listen_addrs {
                                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                                println!("Added address {} for peer {} to Kademlia", addr, peer_id);
                            }
                        }
                        MyBehaviourEvent::Kademlia(evt) => {
                            // Log some Kademlia events for now
                            tracing::debug!("Kademlia event: {:?}", evt);
                            match evt {
                                KademliaEvent::OutboundQueryProgressed { id, result, .. } => {
                                    match result {
                                        QueryResult::GetClosestPeers(Ok(get_closest)) => {
                                            println!("Kademlia GetClosestPeers result for query {:?}: peers={:?}", id, get_closest.peers);
                                        }
                                        QueryResult::GetClosestPeers(Err(err)) => {
                                            println!("Kademlia GetClosestPeers query {:?} failed: {:?}", id, err);
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            }
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
