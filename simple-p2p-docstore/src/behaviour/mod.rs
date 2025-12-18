//! Behaviour modules for composing node roles.

pub mod peer_dht;
pub mod docstore;

#[cfg(not(target_arch = "wasm32"))]
pub mod relay;

pub use peer_dht::*;
pub use docstore::*;
#[cfg(not(target_arch = "wasm32"))]
pub use relay::*;
