use libp2p::{identity, Multiaddr, PeerId};
use libp2p_kad::Mode;
use crate::behaviour::{make_docstore_gossipsub, make_peer_dht};

/// Node roles that determine which behaviours are enabled and how Kademlia is configured.
#[derive(Debug, Clone, Copy)]
pub enum NodeRole {
    Client,
    Relay,
    FullNode,
}

pub struct NodeBuilder {
    role: NodeRole,
    bootstrap_peers: Vec<Multiaddr>,
}

impl NodeBuilder {
    pub fn new(role: NodeRole) -> Self {
        Self { role, bootstrap_peers: Vec::new() }
    }

    pub fn add_bootstrap(mut self, addr: Multiaddr) -> Self {
        self.bootstrap_peers.push(addr);
        self
    }

    /// Assemble the behaviour components for the given identity key.
    /// Returns (ping, gossipsub, identify, kademlia) which can be used to construct a NetworkBehaviour.
    #[cfg(target_arch = "wasm32")]
    pub fn build_behaviours(
        &self,
        key: &identity::Keypair,
    ) -> (
        libp2p::ping::Behaviour,
        libp2p::gossipsub::Behaviour,
        libp2p::identify::Behaviour,
        libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>,
    ) {
        let local_peer_id = PeerId::from(key.public());
        let mode = match self.role {
            NodeRole::Client => Mode::Client,
            NodeRole::Relay | NodeRole::FullNode => Mode::Server,
        };
        let (ping_beh, identify_beh, kademlia_beh) = make_peer_dht(&key.public(), local_peer_id, mode);
        let gossipsub = make_docstore_gossipsub(key);
        (ping_beh, gossipsub, identify_beh, kademlia_beh)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn build_behaviours(
        &self,
        key: &identity::Keypair,
    ) -> (
        libp2p::ping::Behaviour,
        libp2p::gossipsub::Behaviour,
        libp2p::identify::Behaviour,
        libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>,
        Option<libp2p::relay::Behaviour>,
    ) {
        let local_peer_id = PeerId::from(key.public());
        let mode = match self.role {
            NodeRole::Client => Mode::Client,
            NodeRole::Relay | NodeRole::FullNode => Mode::Server,
        };
        let (ping_beh, identify_beh, kademlia_beh) = make_peer_dht(&key.public(), local_peer_id, mode);
        let gossipsub = make_docstore_gossipsub(key);
        let relay_beh = match self.role {
            NodeRole::Relay | NodeRole::FullNode => Some(crate::behaviour::relay::make_relay_behaviour(local_peer_id)),
            _ => None,
        };
        (ping_beh, gossipsub, identify_beh, kademlia_beh, relay_beh)
    }
}
