#!/usr/bin/env node

/**
 * Headless browser test for WASM P2P client
 * Tests connection to WebRTC server and logs any closure errors
 */

const puppeteer = require('puppeteer');

async function main() {
  const serverMultiaddr = process.argv[2] || '/ip4/127.0.0.1/udp/9090/webrtc-direct/certhash/uEiCB2BVS9_aEFviG9xbY8LgaUSa6c6zIbCF4fwirz1SOtQ/p2p/12D3KooWA5VQo4Ax8pypjwn9Gz8MHvjgfeAGuoTcJCiAc59QRtt6';
  
  console.log('Starting headless browser test...');
  console.log('Server multiaddr:', serverMultiaddr);
  
  const browser = await puppeteer.launch({
    headless: true,
    args: [
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--enable-features=SharedArrayBuffer',
      '--disable-web-security', // Allow WASM
      '--allow-insecure-localhost',
    ],
  });
  
  const page = await browser.newPage();
  
  // Capture console logs
  page.on('console', msg => {
    const type = msg.type();
    const text = msg.text();
    console.log(`[BROWSER ${type}]`, text);
  });
  
  // Capture page errors
  page.on('pageerror', error => {
    console.error('[PAGE ERROR]', error.message);
    console.error(error.stack);
  });
  
  // Capture uncaught exceptions and promise rejections
  page.on('error', error => {
    console.error('[ERROR]', error.message);
  });
  
  try {
    console.log('Navigating to http://127.0.0.1:8080/...');
    await page.goto('http://127.0.0.1:8080/', {
      waitUntil: 'networkidle0',
      timeout: 30000,
    });
    
    console.log('Page loaded, waiting for WASM to initialize...');
    await new Promise(resolve => setTimeout(resolve, 2000));
    
    console.log('Setting server address and clicking connect...');
    await page.type('#serverAddr', serverMultiaddr);
    await page.click('#connectBtn');
    
    console.log('Waiting for connection events (30s)...');
    await new Promise(resolve => setTimeout(resolve, 30000));
    
    console.log('Attempting to publish a message...');
    await page.type('#msg', 'Test message from headless browser');
    await page.click('#sendBtn');
    
    console.log('Waiting for message events (10s)...');
    await new Promise(resolve => setTimeout(resolve, 10000));
    
    console.log('Test completed successfully');
  } catch (error) {
    console.error('Test failed:', error.message);
    process.exit(1);
  } finally {
    await browser.close();
  }
}

main().catch(err => {
  console.error('Fatal error:', err);
  process.exit(1);
});
