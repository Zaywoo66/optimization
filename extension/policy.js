(function (root) {
  "use strict";

  const defaults = Object.freeze({
    enabled: false,
    idleMinutes: 30,
    protectedDomains: ["chatgpt.com", "openai.com", "docs.google.com", "office.com"]
  });
  const aiDomains = Object.freeze(["chatgpt.com", "openai.com", "claude.ai", "gemini.google.com", "perplexity.ai"]);

  function normalizeSettings(raw = {}) {
    const minutes = Number(raw.idleMinutes);
    return {
      enabled: raw.enabled === true,
      idleMinutes: Number.isFinite(minutes) ? Math.max(5, Math.min(240, minutes)) : defaults.idleMinutes,
      protectedDomains: Array.isArray(raw.protectedDomains)
        ? raw.protectedDomains.map((domain) => String(domain).trim().toLowerCase()).filter(Boolean)
        : [...defaults.protectedDomains]
    };
  }

  function domainIsProtected(hostname, domains) {
    return domains.some((domain) => hostname === domain || hostname.endsWith(`.${domain}`));
  }

  function isEligible(tab, settings, now = Date.now(), mode = "ordinary") {
    if (!settings.enabled || !Number.isInteger(tab.id) || tab.id < 0) return false;
    if (tab.active || tab.pinned || tab.audible || tab.discarded || tab.autoDiscardable === false) return false;
    if (tab.incognito || tab.status !== "complete") return false;
    if (!Number.isFinite(tab.lastAccessed) || tab.lastAccessed > now) return false;
    try {
      const url = new URL(tab.url);
      if (url.protocol !== "http:" && url.protocol !== "https:") return false;
      const hostname = url.hostname.toLowerCase();
      const isAi = domainIsProtected(hostname, aiDomains);
      // AI tabs are only eligible during a separate, user-initiated sweep.
      if (mode === "ai-only") return isAi && now - tab.lastAccessed >= 120 * 60_000;
      return !isAi && now - tab.lastAccessed >= settings.idleMinutes * 60_000
        && !domainIsProtected(hostname, settings.protectedDomains);
    } catch {
      return false;
    }
  }

  const api = { defaults, aiDomains, normalizeSettings, domainIsProtected, isEligible };
  root.MemoryPilotPolicy = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
