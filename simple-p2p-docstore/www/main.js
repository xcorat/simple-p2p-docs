import init, * as wasm from "../pkg/simple_p2p_docstore.js";

const logEl = document.getElementById("log");
function log(msg) {
  const line = document.createElement("div");
  line.textContent = msg;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

let node = null;

// Event polling loop
async function pollEvents() {
  if (!node) return;
  
  try {
    const event = await node.next_event();
    
    switch (event.type) {
      case "connected":
        log(`✓ Connected to ${event.peer_id}`);
        break;
      case "disconnected":
        log(`✗ Disconnected from ${event.peer_id}`);
        break;
      case "messageReceived":
        log(`📨 Message from ${event.peer_id}: ${event.data}`);
        break;
      case "messagePublished":
        log(`📤 Published message ${event.msg_id}`);
        break;
      case "error":
        log(`❌ Error: ${event.msg}`);
        break;
    }
  } catch (e) {
    // No events available or error
  }
  
  // Continue polling
  setTimeout(pollEvents, 100);
}

(async function main() {
  await init();
  wasm.init_panic_hook();

  document.getElementById("connectBtn").addEventListener("click", async () => {
    const addr = document.getElementById("serverAddr").value.trim();
    if (!addr) {
      log("Please enter a server multiaddr (webrtc-direct)");
      return;
    }
    try {
      node = new wasm.WasmNode(addr);
      log(`Started node (peer_id: ${node.peer_id})`);
      log("Dialing server...");
      pollEvents(); // Start event polling
    } catch (e) {
      log("Connection error: " + e);
    }
  });

  document.getElementById("sendBtn").addEventListener("click", async () => {
    if (!node) {
      log("Not connected yet");
      return;
    }
    const text = document.getElementById("msg").value.trim();
    if (!text) return;
    try {
      node.publish_update(text);
      document.getElementById("msg").value = ""; // Clear input
    } catch (e) {
      log("publish_update error: " + e);
    }
  });
})();
