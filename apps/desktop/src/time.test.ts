import { test } from "node:test";
import assert from "node:assert/strict";
import { ago } from "./time.ts";

const now = Date.UTC(2026, 8, 21, 12, 0, 0);
const min = 60_000;

test("recent times read naturally", () => {
  assert.equal(ago(now - 20_000, now), "just now");
  assert.equal(ago(now - 5 * min, now), "5 minutes ago");
  assert.equal(ago(now - 90 * min, now), "2 hours ago");
  assert.equal(ago(now - 26 * 60 * min, now), "yesterday");
  assert.equal(ago(now - 4 * 24 * 60 * min, now), "4 days ago");
});
