// Optional integration check for an isolated Brave profile started with
// --load-extension=<absolute path to extension> --remote-debugging-port=0.
const port = Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("Usage: node extension/smoke.mjs <DevTools port>");
}

let worker;
for (let attempt = 0; attempt < 80 && !worker; attempt++) {
  const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
  worker = targets.find((target) =>
    target.type === "service_worker" &&
    /^chrome-extension:\/\/[^/]+\/service_worker\.js$/.test(target.url)
  );
  if (!worker) await new Promise((resolve) => setTimeout(resolve, 250));
}
if (!worker) throw new Error("Memory Pilot service worker was not found");

const socket = new WebSocket(worker.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

let nextId = 0;
function evaluate(expression) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => reject(new Error("DevTools timed out")), 20000);
    function onMessage(event) {
      const message = JSON.parse(event.data);
      if (message.id !== id) return;
      socket.removeEventListener("message", onMessage);
      clearTimeout(timer);
      if (message.error || message.result?.exceptionDetails) {
        reject(new Error(JSON.stringify(message.error || message.result.exceptionDetails)));
      } else {
        resolve(message.result?.result?.value);
      }
    }
    socket.addEventListener("message", onMessage);
    socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });
}

try {
  const result = await evaluate(`(async () => {
    if (!globalThis.MemoryPilotPolicy || !globalThis.MemoryPilotSweep) throw new Error('Scripts not loaded');
    const tab = await chrome.tabs.create({ url: 'https://example.com', active: false });
    const realNow = Date.now;
    try {
      for (let i = 0; i < 40; i++) {
        const current = (await chrome.tabs.query({})).find(candidate => candidate.id === tab.id);
        if (current?.status === 'complete') break;
        await new Promise(resolve => setTimeout(resolve, 250));
      }
      await chrome.storage.local.set({ enabled: true, idleMinutes: 5, protectedDomains: [] });
      Date.now = () => realNow() + 6 * 60_000;
      return await runSweep();
    } finally {
      Date.now = realNow;
      await chrome.storage.local.set({ enabled: false });
      try { await chrome.tabs.remove(tab.id); } catch { /* browser may have closed it */ }
    }
  })()`);
  if (result?.discarded !== 1) {
    throw new Error(`Unexpected discard result: ${JSON.stringify(result)}`);
  }
  console.log("Brave acknowledged one discarded test tab");
} finally {
  socket.close();
}
