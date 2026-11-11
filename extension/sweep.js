(function (root) {
  "use strict";

  async function sweepTabs(api, settings, policy, now = Date.now(), mode = "ordinary") {
    if (!settings.enabled) return { scanned: 0, candidates: 0, discarded: 0 };
    const tabs = await api.query({});
    let candidates = 0;
    let discarded = 0;
    for (const tab of tabs) {
      if (!policy.isEligible(tab, settings, now, mode)) continue;
      candidates++;
      try {
        // A tab can become active between query() and discard(). Check again.
        const current = (await api.query({})).find((item) => item.id === tab.id);
        if (!policy.isEligible(current, settings, Date.now(), mode)) continue;
        const result = await api.discard(tab.id);
        if (result?.discarded) discarded++;
      } catch {
        // A tab can close or refuse discard while the sweep is running.
      }
    }
    return { scanned: tabs.length, candidates, discarded };
  }

  const api = { sweepTabs };
  root.MemoryPilotSweep = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
