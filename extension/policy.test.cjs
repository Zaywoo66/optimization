const test = require("node:test");
const assert = require("node:assert/strict");
const policy = require("./policy.js");
const { sweepTabs } = require("./sweep.js");

const now = Date.now();
const settings = { enabled: true, idleMinutes: 30, protectedDomains: ["chatgpt.com"] };
const oldTab = (overrides = {}) => ({
  id: 7, active: false, pinned: false, audible: false, discarded: false,
  autoDiscardable: true, incognito: false, status: "complete",
  lastAccessed: now - 45 * 60_000, url: "https://example.org/article", ...overrides
});

test("keeps active, pinned, audible, protected, and recent tabs", () => {
  for (const change of [
    { active: true }, { pinned: true }, { audible: true },
    { autoDiscardable: false }, { discarded: true }, { incognito: true },
    { status: "loading" }, { lastAccessed: now - 5 * 60_000 },
    { url: "https://chatgpt.com/c/123" }, { url: "chrome://settings" }
  ]) {
    assert.equal(policy.isEligible(oldTab(change), settings, now), false);
  }
});

test("selects only old ordinary web tabs when enabled", () => {
  assert.equal(policy.isEligible(oldTab(), settings, now), true);
  assert.equal(policy.isEligible(oldTab(), { ...settings, enabled: false }, now), false);
  assert.equal(policy.isEligible(oldTab({ lastAccessed: undefined }), settings, now), false);
  assert.equal(policy.isEligible(oldTab({ url: "https://sub.chatgpt.com/c/123" }), settings, now), false);
});

test("disabled extension does not query or discard tabs", async () => {
  const tabs = { query: () => { throw new Error("should not be called"); } };
  const result = await sweepTabs(tabs, { ...settings, enabled: false }, policy, now);
  assert.deepEqual(result, { scanned: 0, candidates: 0, discarded: 0 });
});

test("rechecks tab state before discarding", async () => {
  const discarded = [];
  let queries = 0;
  const tabs = {
    query: async () => {
      queries++;
      return [oldTab({ id: 1 }), oldTab({ id: 2, active: queries > 1 })];
    },
    discard: async (id) => { discarded.push(id); return { discarded: true }; }
  };
  const result = await sweepTabs(tabs, settings, policy, now);
  assert.deepEqual(discarded, [1]);
  assert.deepEqual(result, { scanned: 2, candidates: 2, discarded: 1 });
});
