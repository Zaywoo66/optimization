importScripts("policy.js", "sweep.js");

const ALARM_NAME = "memory-pilot-sweep";
const policy = globalThis.MemoryPilotPolicy;

async function settings() {
  return policy.normalizeSettings(await chrome.storage.local.get(policy.defaults));
}

async function syncAlarm() {
  const current = await settings();
  await chrome.alarms.clear(ALARM_NAME);
  if (current.enabled) {
    chrome.alarms.create(ALARM_NAME, { periodInMinutes: 5 });
  }
}

async function runSweep(mode = "ordinary") {
  const current = await settings();
  const sweepSettings = mode === "ai-only" ? { ...current, enabled: true } : current;
  const result = await globalThis.MemoryPilotSweep.sweepTabs(chrome.tabs, sweepSettings, policy, Date.now(), mode);
  await chrome.storage.local.set({ lastResult: { ...result, mode, time: Date.now() } });
  return result;
}

chrome.runtime.onInstalled.addListener(() => { void syncAlarm(); });
chrome.runtime.onStartup.addListener(() => { void syncAlarm(); });
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && (changes.enabled || changes.idleMinutes)) void syncAlarm();
});
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) void runSweep();
});
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "sweep-now" && message?.type !== "sweep-ai") return false;
  runSweep(message.type === "sweep-ai" ? "ai-only" : "ordinary")
    .then(sendResponse, (error) => sendResponse({ error: error.message }));
  return true;
});
