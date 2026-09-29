import { test } from "node:test";
import assert from "node:assert/strict";
import { bubble, type View } from "./bubble.ts";

const view = (overrides: Partial<View>): View => ({
  op: 1,
  state: "Idle",
  text: "",
  detail: "",
  sessionId: null,
  continued: false,
  agent: "claude",
  limited: false,
  speaking: false,
  ...overrides,
});

test("listening without speech waits", () => {
  const b = bubble(view({ state: "Listening", text: "проверь" }));
  assert.deepEqual([b.strip, b.mic, b.running, b.phrases.length], ["idle", "waiting", null, 1]);
});

test("speech listens", () => {
  const b = bubble(view({ state: "Listening", text: "проверь diff", speaking: true }));
  assert.deepEqual([b.strip, b.mic, b.phrases[0].rail], ["speak", "listening", "speak"]);
});

test("transcribing shows the pencil", () => {
  const b = bubble(view({ state: "Transcribing", text: "проверь diff" }));
  assert.deepEqual(
    [b.strip, b.mic, b.running, b.phrases[0].rail],
    ["decode", "off", { icon: "pencil", text: "Transcribing", dots: true }, "decode"],
  );
});

test("classifying reads as transcribing", () => {
  assert.deepEqual(bubble(view({ state: "Classifying", text: "x" })).running, {
    icon: "pencil",
    text: "Checking command",
    dots: true,
  });
});

test("running shows the agent and the tool", () => {
  const b = bubble(view({ state: "Running", text: "x", detail: "Bash cargo test", agent: "codex" }));
  assert.deepEqual(
    [b.running, b.mic, b.strip, b.phrases[0].rail],
    [{ icon: "terminal", text: "Codex is working · Bash cargo test", dots: false }, "off", "off", "run"],
  );
});

test("limited mode shows in the status", () => {
  assert.equal(
    bubble(view({ state: "Running", text: "x", agent: "codex", limited: true })).running?.text,
    "Codex is working · limited mode",
  );
});

test("cancelling strikes the phrase", () => {
  const b = bubble(view({ state: "Cancelling", text: "x" }));
  assert.deepEqual([b.phrases[0].rail, b.running], ["gone", { icon: "terminal", text: "Cancelling", dots: false }]);
});

test("done goes on the phrase", () => {
  const b = bubble(view({ state: "Succeeded", text: "x", detail: "no lint errors", sessionId: "s" }));
  assert.deepEqual(
    [b.phrases[0].rail, b.phrases[0].outcome, b.phrases[0].clickHint, b.running, b.mic, b.clickable],
    ["ok", "Done · no lint errors", "Click to open in terminal", null, "off", true],
  );
});

test("failure goes on the phrase", () => {
  const b = bubble(view({ state: "Failed", text: "x", detail: "rate limit reached", sessionId: "s" }));
  assert.deepEqual(
    [b.phrases[0].rail, b.phrases[0].outcome, b.globalError],
    ["err", "Claude failed · rate limit reached", null],
  );
});

test("limited click hint", () => {
  const b = bubble(view({ state: "Failed", text: "x", agent: "codex", limited: true, sessionId: "s" }));
  assert.equal(b.phrases[0].clickHint, "Click to open in Codex and trust");
});

test("failure without a phrase is a global error", () => {
  const b = bubble(view({ state: "Failed", detail: "Microphone unavailable" }));
  assert.deepEqual(
    [b.phrases.length, b.globalError, b.strip, b.mic, b.clickable],
    [0, "Microphone unavailable", "error", "error", false],
  );
});

test("no session, no click", () => {
  assert.equal(bubble(view({ state: "Succeeded", text: "x" })).clickable, false);
});

test("long unbroken text stays one phrase", () => {
  const b = bubble(view({ state: "Running", text: "C:/a/".repeat(80) }));
  assert.equal(b.phrases.length, 1);
});
