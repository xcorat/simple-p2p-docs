#![cfg(not(target_arch = "wasm32"))]

use libp2p::relay;

/// Create a basic relay behaviour (Circuit Relay v2) with default config.
///
/// This requires the `relay` feature to be available in `libp2p` (native builds).
use libp2p::PeerId;

pub fn make_relay_behaviour(local_peer_id: PeerId) -> relay::Behaviour {
    relay::Behaviour::new(local_peer_id, relay::Config::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_relay_behaviour() {
        let _ = make_relay_behaviour();
    }
}
