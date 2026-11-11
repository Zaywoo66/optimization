// Reproducible memory check in a temporary, isolated Brave profile.
// Run: node extension/benchmark.mjs "C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const bravePath = process.argv[2];
if (!bravePath) throw new Error("Pass the Brave executable path");
const profile = await mkdtemp(join(tmpdir(), "memory-pilot-benchmark-"));
const extension = resolve("extension");
const server = createServer((_request, response) => {
  response.setHeader("Content-Type", "text/html; charset=utf-8");
  response.end(`<title>Memory Pilot benchmark</title><script>
    window.blocks = Array.from({length: 16}, (_, i) => {
      const block = new Uint8Array(1024 * 1024);
      for (let j = 0; j < block.length; j += 4096) block[j] = i + j;
      return block;
    });
  </script>`);
});
await new Promise((resolveServer) => server.listen(0, "127.0.0.1", resolveServer));
const webPort = server.address().port;
const browser = spawn(bravePath, [
  `--user-data-dir=${profile}`, "--headless=new", "--no-first-run", "--no-default-browser-check",
  "--disable-background-networking", "--remote-debugging-port=0",
  `--disable-extensions-except=${extension}`, `--load-extension=${extension}`, "about:blank"
], { stdio: "ignore" });

let socket;
let nextId = 0;
function evaluate(expression) {
  return new Promise((resolveValue, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => reject(new Error("DevTools timed out")), 30000);
    function onMessage(event) {
      const message = JSON.parse(event.data);
      if (message.id !== id) return;
      socket.removeEventListener("message", onMessage);
      clearTimeout(timer);
      if (message.error || message.result?.exceptionDetails) {
        reject(new Error(JSON.stringify(message.error || message.result.exceptionDetails)));
      } else resolveValue(message.result?.result?.value);
    }
    socket.addEventListener("message", onMessage);
    socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });
}

function processTreeMiB(rootPid) {
  const script = `$p=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize; $ids=@(${rootPid}); for($i=0;$i -lt 10;$i++){ $new=@($p | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object ProcessId); if(!$new.Count){break}; $ids+=@($new) }; [math]::Round((($p | Where-Object { $ids -contains $_.ProcessId } | Measure-Object WorkingSetSize -Sum).Sum / 1MB),1)`;
  return Number(execFileSync("powershell.exe", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim());
}

try {
  let port;
  for (let attempt = 0; attempt < 100 && !port; attempt++) {
    try { port = Number((await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0]); }
    catch { await delay(200); }
  }
  if (!port) throw new Error("Brave did not start DevTools");
  let worker;
  for (let attempt = 0; attempt < 100 && !worker; attempt++) {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
    worker = targets.find((target) => target.type === "service_worker" && /chrome-extension:\/\/[^/]+\/service_worker\.js$/.test(target.url));
    if (!worker) await delay(200);
  }
  if (!worker) throw new Error("Extension service worker was not loaded");
  socket = new WebSocket(worker.webSocketDebuggerUrl);
  await new Promise((resolveSocket, reject) => {
    socket.addEventListener("open", resolveSocket, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  const tabIds = await evaluate(`(async () => {
    const ids = [];
    for (let i = 0; i < 4; i++) ids.push((await chrome.tabs.create({url: 'http://127.0.0.1:${webPort}/heavy?i=' + i, active: false})).id);
    for (let i = 0; i < 100; i++) {
      const tabs = await chrome.tabs.query({});
      if (ids.every(id => tabs.find(tab => tab.id === id)?.status === 'complete')) return ids;
      await new Promise(resolve => setTimeout(resolve, 200));
    }
    throw new Error('Test tabs did not load');
  })()`);
  await delay(3000);
  const before = processTreeMiB(browser.pid);
  const result = await evaluate(`(async () => {
    await chrome.storage.local.set({enabled: true, idleMinutes: 5, protectedDomains: []});
    const realNow = Date.now;
    try { Date.now = () => realNow() + 6 * 60_000; return await runSweep(); }
    finally { Date.now = realNow; await chrome.storage.local.set({enabled: false}); }
  })()`);
  await delay(3000);
  const after = processTreeMiB(browser.pid);
  console.log(JSON.stringify({ tabs: tabIds.length, discarded: result.discarded, beforeMiB: before, afterMiB: after, changeMiB: Math.round((after - before) * 10) / 10, note: "Isolated synthetic tabs; Windows working sets fluctuate and may count shared pages." }, null, 2));
  if (result.discarded !== tabIds.length) process.exitCode = 1;
} finally {
  socket?.close();
  browser.kill();
  server.close();
  await delay(1000);
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
}
