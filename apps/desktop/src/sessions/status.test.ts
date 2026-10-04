import { test } from "node:test";
import assert from "node:assert/strict";
import type { Phrase, Status, View } from "../bubble.ts";
import { liveOf, markOf, railOf } from "./status.ts";

const phrase = (status: Status, text = "x"): Phrase => ({ id: 1, kind: "speech", status, text, outcome: "" });
const view = (over: Partial<View>): View =>
  ({ mic: "off", phrases: [], sessionId: "a", ...over }) as View;

test("other sessions are idle", () => {
  assert.equal(markOf(view({ phrases: [phrase("running")] }), "b"), "idle");
  assert.equal(markOf(null, "a"), "idle");
});

test("marks follow the most active phrase", () => {
  assert.equal(markOf(view({ phrases: [phrase("speaking"), phrase("running")] }), "a"), "run");
  assert.equal(markOf(view({ phrases: [phrase("cancelling")] }), "a"), "run");
  assert.equal(markOf(view({ phrases: [phrase("transcribing"), phrase("speaking")] }), "a"), "speak");
  assert.equal(markOf(view({ phrases: [phrase("classifying")] }), "a"), "decode");
  assert.equal(markOf(view({ mic: "waiting", phrases: [phrase("done")] }), "a"), "waiting");
  assert.equal(markOf(view({ phrases: [phrase("done")] }), "a"), "idle");
});

test("statuses map to the bubble's rails", () => {
  assert.deepEqual(
    (["queued", "failed", "cancelled", "done", "running", "speaking", "transcribing"] as Status[]).map(railOf),
    ["queue", "err", "gone", "ok", "run", "speak", "decode"],
  );
});

test("live phrases skip what history already keeps", () => {
  const v = view({ phrases: [phrase("done", "a"), phrase("failed", "b"), phrase("cancelled", "c"), phrase("queued", "d")] });
  assert.deepEqual(liveOf(v, "a"), [
    { rail: "gone", text: "c" },
    { rail: "queue", text: "d" },
  ]);
});

test("a waiting mic with nothing said shows Waiting", () => {
  assert.deepEqual(liveOf(view({ mic: "waiting" }), "a"), [{ rail: "waiting", text: "Waiting" }]);
  assert.deepEqual(liveOf(view({ mic: "waiting" }), "b"), []);
});

test("a running phrase is already in history", () => {
  assert.deepEqual(liveOf(view({ phrases: [phrase("running"), phrase("cancelling"), phrase("speaking", "s")] }), "a"), [
    { rail: "speak", text: "s" },
  ]);
});

test("a cancelled phrase that already ran shows once, from history", () => {
  const v = view({ phrases: [phrase("cancelled", "Run the release"), phrase("cancelled", "never sent")] });
  assert.deepEqual(liveOf(v, "a", "Run the release"), [{ rail: "gone", text: "never sent" }]);
});
