use libp2p::{identify, ping, identity::PublicKey, PeerId};
use libp2p_kad::{Behaviour as KademliaBehaviour, store::MemoryStore, Mode};

/// Construct basic PeerDHT behaviours (ping, identify, kademlia) for a node.
///
/// Returns (ping_behaviour, identify_behaviour, kademlia_behaviour)
pub fn make_peer_dht(
    local_pub: &PublicKey,
    local_peer_id: PeerId,
    mode: Mode,
) -> (ping::Behaviour, identify::Behaviour, KademliaBehaviour<MemoryStore>) {
    let ping_behaviour = ping::Behaviour::default();

    let identify_cfg = identify::Config::new("simple-p2p-docstore/0.1".to_string(), local_pub.clone());
    let identify_behaviour = identify::Behaviour::new(identify_cfg);

    let store = MemoryStore::new(local_peer_id);
    let mut kademlia = KademliaBehaviour::new(local_peer_id, store);
    kademlia.set_mode(Some(mode));

    (ping_behaviour, identify_behaviour, kademlia)
}
