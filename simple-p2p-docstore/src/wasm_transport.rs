#![cfg(target_arch = "wasm32")]
//! Modular transport builder for browser-to-browser WebRTC communication.
//!
//! This module provides a composite transport pattern that combines:
//! - WebRTC transport (for direct browser-to-browser connections)
//! - Relay client transport (for Circuit Relay v2 signaling)
//! - WebRTC-direct transport (for connecting to relay servers via WebRTC)
//!
//! The design is modular to allow future expansion (e.g., adding WebSocket transport).

use std::sync::Arc;

use futures::task::AtomicWaker;
use libp2p::{
    core::muxing::StreamMuxerBox,
    core::upgrade::Version,
    identity::Keypair,
    noise, yamux, Transport as LibP2PTransport,
};
use libp2p_webrtc_websys::browser::{
    Behaviour as WebRTCBehaviour, Config as WebRTCConfig, SignalingConfig,
    Transport as BrowserWebrtcTransport,
};

/// Configuration for the composite transport
#[derive(Clone)]
pub struct TransportConfig {
    pub keypair: Keypair,
    pub enable_websocket: bool, // Future expansion: WebSocket transport
    pub stun_servers: Vec<String>,
    pub signaling_config: SignalingConfig,
}

impl Default for TransportConfig {
    fn default() -> Self {
        let keypair = Keypair::generate_ed25519();
        let local_peer_id = keypair.public().to_peer_id();
        
        let stun_servers = vec!["stun:stun.l.google.com:19302".to_string()];
        
        let signaling_config = SignalingConfig::new(
            3,                                      // max retries
            100,                                    // max ice gathering attempts
            std::time::Duration::from_millis(0),   // signaling delay
            std::time::Duration::from_millis(100), // connection check delay
            300,                                    // max connection checks (30 seconds)
            std::time::Duration::from_secs(10),    // ICE gathering timeout
            local_peer_id,                          // The local peer's peer_id
            Some(stun_servers.clone()),            // STUN servers
        );

        Self {
            keypair,
            enable_websocket: false,
            stun_servers,
            signaling_config,
        }
    }
}

impl TransportConfig {
    pub fn new(keypair: Keypair) -> Self {
        let local_peer_id = keypair.public().to_peer_id();
        let stun_servers = vec!["stun:stun.l.google.com:19302".to_string()];
        
        let signaling_config = SignalingConfig::new(
            3,
            100,
            std::time::Duration::from_millis(0),
            std::time::Duration::from_millis(100),
            300,
            std::time::Duration::from_secs(10),
            local_peer_id,
            Some(stun_servers.clone()),
        );

        Self {
            keypair,
            enable_websocket: false,
            stun_servers,
            signaling_config,
        }
    }

    pub fn with_stun_servers(mut self, servers: Vec<String>) -> Self {
        self.stun_servers = servers.clone();
        // Update signaling config with new STUN servers
        let local_peer_id = self.keypair.public().to_peer_id();
        self.signaling_config = SignalingConfig::new(
            3,
            100,
            std::time::Duration::from_millis(0),
            std::time::Duration::from_millis(100),
            300,
            std::time::Duration::from_secs(10),
            local_peer_id,
            Some(servers),
        );
        self
    }

    pub fn with_websocket(mut self, enable: bool) -> Self {
        self.enable_websocket = enable;
        self
    }
}

/// Composite transport type that supports WebRTC, Relay, and optionally WebSocket
pub type CompositeTransport = libp2p::core::transport::Boxed<(libp2p::PeerId, StreamMuxerBox)>;

/// Build a composite transport for browser-to-browser communication.
///
/// Returns (transport, webrtc_behaviour, relay_behaviour)
pub fn build_composite_transport(
    config: TransportConfig,
    transport_waker: Arc<AtomicWaker>,
) -> Result<
    (
        CompositeTransport,
        WebRTCBehaviour,
        libp2p_relay::client::Behaviour,
    ),
    Box<dyn std::error::Error>,
> {
    let local_peer_id = config.keypair.public().to_peer_id();

    // 1. Create relay client transport and behaviour
    let (relay_transport, relay_behaviour) = libp2p_relay::client::new(local_peer_id);

    // Upgrade relay transport with security and multiplexing
    let relay_transport_upgraded = relay_transport
        .upgrade(Version::V1)
        .authenticate(noise::Config::new(&config.keypair)?)
        .multiplex(yamux::Config::default())
        .boxed();

    // 2. Create Browser WebRTC transport and behaviour for browser-to-browser connections
    let webrtc_config = WebRTCConfig {
        keypair: config.keypair.clone(),
    };

    let (webrtc_transport, webrtc_behaviour) =
        BrowserWebrtcTransport::new(webrtc_config, config.signaling_config, transport_waker);
    let browser_webrtc_transport_boxed = webrtc_transport.boxed();

    // 3. Create Standard WebRTC transport for browser-to-server connections
    let standard_webrtc_transport = libp2p_webrtc_websys::Transport::new(
        libp2p_webrtc_websys::Config::new(&config.keypair),
    )
    .boxed();

    // Combine standard and browser webrtc transports
    let combined_webrtc = standard_webrtc_transport
        .or_transport(browser_webrtc_transport_boxed)
        .map(|either_webrtc, _| match either_webrtc {
            futures::future::Either::Left(output) => output,
            futures::future::Either::Right(output) => output,
        });

    // 4. Build the final composite transport
    // StandardWebRTC OR BrowserWebRTC OR Relay
    let final_transport = if config.enable_websocket {
        // Future expansion: Add WebSocket transport
        combined_webrtc
            .or_transport(relay_transport_upgraded)
            .map(|either_output, _| match either_output {
                futures::future::Either::Left((peer_id, connection)) => {
                    (peer_id, StreamMuxerBox::new(connection)) // It's already StreamMuxerBox actually, but wait...
                }
                // Relay output
                futures::future::Either::Right(output) => output,
            })
            .boxed()
    } else {
        combined_webrtc
            .or_transport(relay_transport_upgraded)
            .map(|either_output, _| match either_output {
                futures::future::Either::Left((peer_id, connection)) => {
                    (peer_id, connection) // Connection is already StreamMuxerBox from .boxed()
                }
                // Relay output
                futures::future::Either::Right(output) => output,
            })
            .boxed()
    };

    Ok((final_transport, webrtc_behaviour, relay_behaviour))
}
