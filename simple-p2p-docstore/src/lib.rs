// Root library: expose behaviour and node modules to binaries and tests.
pub mod behaviour;
pub mod node;

// WASM-specific bindings are implemented in a separate module to avoid
// compiling wasm-only code for native targets.
#[cfg(target_arch = "wasm32")]
mod wasm_bindings;
#[cfg(target_arch = "wasm32")]
pub use wasm_bindings::*;
