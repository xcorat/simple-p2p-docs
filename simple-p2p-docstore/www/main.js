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
let connectedRelayAddr = null; // Store the relay address we connected to

// Network status rendering
function renderNetworkStatus(status) {
  // My Peer ID
  const myPeerIdEl = document.getElementById("myPeerId");
  if (node) {
    myPeerIdEl.textContent = node.peer_id;
    myPeerIdEl.classList.remove('empty');
  }

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
    relaysEl.innerHTML = status.relays.map(relay => {
      const supportsBadge = relay.supports_relay 
        ? '<span style="color: green;">✓ Relay</span>' 
        : '<span style="color: orange;">⚠ Unvalidated</span>';
      const timeAgo = relay.connected_at ? new Date(relay.connected_at).toLocaleTimeString() : 'Unknown';
      return `<div class="peer-item">
        ${relay.peer_id} ${supportsBadge}
        <div class="peer-addrs">${relay.full_addr}</div>
        <div class="peer-addrs">Connected: ${timeAgo}</div>
      </div>`;
    }).join('');
    relaysEl.classList.remove('empty');
  } else {
    relaysEl.innerHTML = "None";
    relaysEl.classList.add('empty');
  }
}

// Status polling loop (1s interval)
async function pollStatus() {
  if (!node) return;
  
  try {
    const status = await node.get_network_status();
    // console.log("Network status:", status); // Debug
    renderNetworkStatus(status);
  } catch (e) {
    console.error("Error polling network status:", e);
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
      case "directMessageReceived":
        log(`💬 Direct message from ${event.peer_id}: ${event.data}`);
        break;
      case "directMessageSent":
        log(`✓ Direct message sent to ${event.peer_id}`);
        break;
      case "listenStarted":
        log(`👂 Started listening on ${event.addr}`);
        break;
      case "relayReservationCreated":
        log(`🎉 Relay reservation created!`);
        log(`📋 Your browser address: ${event.addr}`);
        log(`💡 Share this address with other browsers to connect`);
        // Display in UI
        const reservationEl = document.getElementById("reservationAddr");
        reservationEl.textContent = event.addr;
        reservationEl.classList.remove('empty');
        reservationEl.style.cursor = 'pointer';
        reservationEl.onclick = () => {
          navigator.clipboard.writeText(event.addr);
          log("✓ Copied to clipboard!");
        };
        // Enable dial button
        document.getElementById("dialBrowserBtn").disabled = false;
        break;
      case "relayConnectionEstablished":
        log(`🔗 Relay connection established with ${event.peer_id}`);
        break;
      case "webrtcConnectionEstablished":
        log(`✅ Direct WebRTC connection established with ${event.peer_id}`);
        log(`🚀 You are now connected peer-to-peer!`);
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
      log("Please enter a relay server multiaddr (webrtc-direct)");
      return;
    }
    try {
      node = new wasm.WasmNode(addr);
      connectedRelayAddr = addr; // Store for later use
      log(`Started node (peer_id: ${node.peer_id})`);
      log("Connecting to relay server...");
      pollEvents(); // Start event polling
      
      // Start status polling every 1s
      if (statusPollInterval) {
        clearInterval(statusPollInterval);
      }
      statusPollInterval = setInterval(pollStatus, 1000);
      pollStatus(); // Initial poll
      
      // Enable relay listen button after connection
      document.getElementById("listenRelayBtn").disabled = false;
    } catch (e) {
      log("Connection error: " + e);
    }
  });

  document.getElementById("listenRelayBtn").addEventListener("click", async () => {
    if (!node || !connectedRelayAddr) {
      log("Not connected to relay yet");
      return;
    }
    try {
      // Step 1: Listen on relay circuit
      node.listen_on_relay(connectedRelayAddr);
      log("📡 Requesting relay reservation...");
      document.getElementById("listenRelayBtn").disabled = true;
      
      // Step 2: Start WebRTC listener (after reservation is created)
      // We'll enable this button after reservation is created
      document.getElementById("listenWebRTCBtn").disabled = false;
    } catch (e) {
      log("listen_on_relay error: " + e);
    }
  });

  document.getElementById("listenWebRTCBtn").addEventListener("click", async () => {
    if (!node) {
      log("Not connected yet");
      return;
    }
    try {
      node.listen_for_webrtc();
      log("👂 Starting WebRTC listener for incoming connections...");
      document.getElementById("listenWebRTCBtn").disabled = true;
    } catch (e) {
      log("listen_for_webrtc error: " + e);
    }
  });

  document.getElementById("dialBrowserBtn").addEventListener("click", async () => {
    if (!node) {
      log("Not connected yet");
      return;
    }
    const peerAddr = document.getElementById("browserPeerAddr").value.trim();
    if (!peerAddr) {
      log("Please enter a browser peer address");
      return;
    }
    try {
      node.dial_peer(peerAddr);
      log(`🔗 Dialing browser peer: ${peerAddr}`);
      log("⏳ Establishing WebRTC connection via relay...");
    } catch (e) {
      log("dial_peer error: " + e);
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

  document.getElementById("sendDirectBtn").addEventListener("click", async () => {
    if (!node) {
      log("Not connected yet");
      return;
    }
    const peerId = document.getElementById("directPeerId").value.trim();
    const msg = document.getElementById("directMsg").value.trim();
    if (!peerId || !msg) {
      log("Please enter both peer id and message");
      return;
    }
    try {
      node.send_direct(peerId, msg);
      log(`Sending direct message to ${peerId}: ${msg}`);
      document.getElementById("directMsg").value = ""; // Clear input
    } catch (e) {
      log("send_direct error: " + e);
    }
  });
})();
