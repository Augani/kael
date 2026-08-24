import init from "./browser_worker_smoke.js";

// A module worker can receive its first host message while its JavaScript and
// WebAssembly modules are still loading. Buffer that startup window explicitly
// so the Kael protocol handshake is not dependent on browser queueing details.
const startupMessages = [];
const bufferStartupMessage = (event) => startupMessages.push(event);
self.addEventListener("message", bufferStartupMessage);

await init();

self.removeEventListener("message", bufferStartupMessage);
for (const event of startupMessages) {
  self.onmessage?.call(self, event);
}
