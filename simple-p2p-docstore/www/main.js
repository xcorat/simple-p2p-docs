import init, * as wasm from "../pkg/simple_p2p_docstore.js";

const logEl = document.getElementById("log");
function log(msg) {
  const line = document.createElement("div");
  line.textContent = msg;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

let node = null;
let statusPollInterval = null;

// Network status rendering
function renderNetworkStatus(status) {
  // Listen addresses
  const listenAddrsEl = document.getElementById("listenAddrs");
  if (status.listenAddrs && status.listenAddrs.length > 0) {
    listenAddrsEl.innerHTML = status.listenAddrs.map(addr => `<div>${addr}</div>`).join('');
    listenAddrsEl.classList.remove('empty');
  } else {
    listenAddrsEl.innerHTML = "None";
    listenAddrsEl.classList.add('empty');
  }

  // Connected peers
  const connectedPeersEl = document.getElementById("connectedPeers");
  const connectedEntries = Object.entries(status.connectedPeers || {});
  if (connectedEntries.length > 0) {
    connectedPeersEl.innerHTML = connectedEntries.map(([peerId, addrs]) => {
      const addrList = addrs.map(addr => `<div class="peer-addrs">${addr}</div>`).join('');
      return `<div class="peer-item">${peerId}${addrList}</div>`;
    }).join('');
    connectedPeersEl.classList.remove('empty');
  } else {
    connectedPeersEl.innerHTML = "None";
    connectedPeersEl.classList.add('empty');
  }

  // Discovered peers
  const discoveredPeersEl = document.getElementById("discoveredPeers");
  const discoveredEntries = Object.entries(status.discoveredPeers || {});
  if (discoveredEntries.length > 0) {
    discoveredPeersEl.innerHTML = discoveredEntries.map(([peerId, addrs]) => {
      const addrList = addrs.map(addr => `<div class="peer-addrs">${addr}</div>`).join('');
      return `<div class="peer-item">${peerId}${addrList}</div>`;
    }).join('');
    discoveredPeersEl.classList.remove('empty');
  } else {
    discoveredPeersEl.innerHTML = "None";
    discoveredPeersEl.classList.add('empty');
  }

  // Subscriptions
  const subscriptionsEl = document.getElementById("subscriptions");
  if (status.subscriptions && status.subscriptions.length > 0) {
    subscriptionsEl.innerHTML = status.subscriptions.map(sub => `<div>${sub}</div>`).join('');
    subscriptionsEl.classList.remove('empty');
  } else {
    subscriptionsEl.innerHTML = "None";
    subscriptionsEl.classList.add('empty');
  }

  // Relays
  const relaysEl = document.getElementById("relays");
  if (status.relays && status.relays.length > 0) {
    relaysEl.innerHTML = status.relays.map(relay => `<div>${relay}</div>`).join('');
    relaysEl.classList.remove('empty');
  } else {
    relaysEl.innerHTML = "None (relay support not yet implemented)";
    relaysEl.classList.add('empty');
  }
}

// Status polling loop (1s interval)
async function pollStatus() {
  if (!node) return;
  
  try {
    const status = await node.get_network_status();
    renderNetworkStatus(status);
  } catch (e) {
    // Ignore errors during polling
  }
}

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
      case "peerDiscovery":
        // event.addrs is an array of addresses, event.peer_id is string
        log(`🔎 Peer discovery: ${event.peer_id}`);
        if (event.addrs && event.addrs.length > 0) {
          log(`   Addresses: ${event.addrs.join(', ')}`);
        } else {
          log('   No addresses discovered');
        }
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
      
      // Start status polling every 1s
      if (statusPollInterval) {
        clearInterval(statusPollInterval);
      }
      statusPollInterval = setInterval(pollStatus, 1000);
      pollStatus(); // Initial poll
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

  document.getElementById("findPeerBtn").addEventListener("click", async () => {
    if (!node) {
      log("Not connected yet");
      return;
    }
    const peerId = document.getElementById("peerId").value.trim();
    if (!peerId) {
      log("Please enter a peer id to find");
      return;
    }
    try {
      node.find_peer(peerId);
      log(`Started Kademlia find_peer query for ${peerId}`);
    } catch (e) {
      log("find_peer error: " + e);
    }
  });
})();
