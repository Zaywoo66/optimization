const policy = globalThis.MemoryPilotPolicy;
const enabled = document.getElementById("enabled");
const minutes = document.getElementById("minutes");
const domains = document.getElementById("domains");
const status = document.getElementById("status");

function showStatus(text) { status.textContent = text; }

async function load() {
  const stored = await chrome.storage.local.get({ ...policy.defaults, lastResult: null });
  const current = policy.normalizeSettings(stored);
  enabled.checked = current.enabled;
  minutes.value = String(current.idleMinutes);
  domains.value = current.protectedDomains.join(", ");
  if (stored.lastResult?.time) {
    showStatus(`Последняя проверка: выгружено ${stored.lastResult.discarded} из ${stored.lastResult.scanned} вкладок.`);
  } else {
    showStatus("Автоматическая выгрузка выключена до вашего включения.");
  }
}

async function save() {
  const list = domains.value.split(",").map((value) => value.trim().toLowerCase()).filter(Boolean);
  if (list.some((domain) => !/^[a-z0-9.-]+$/.test(domain))) {
    showStatus("Введите только домены без адреса страницы, через запятую.");
    return false;
  }
  await chrome.storage.local.set({
    enabled: enabled.checked,
    idleMinutes: Number(minutes.value),
    protectedDomains: [...new Set(list)]
  });
  showStatus("Настройки сохранены.");
  return true;
}

document.getElementById("save").addEventListener("click", () => { void save(); });
document.getElementById("run").addEventListener("click", async () => {
  if (!(await save())) return;
  if (!enabled.checked) { showStatus("Сначала включите выгрузку вкладок."); return; }
  const result = await chrome.runtime.sendMessage({ type: "sweep-now" });
  showStatus(result?.error || `Выгружено ${result.discarded} из ${result.scanned} вкладок.`);
});

void load();
