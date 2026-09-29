import { test } from "node:test";
import assert from "node:assert/strict";
import { bubble, type Phrase, type Status, type View } from "./bubble.ts";

let next = 0;
const phrase = (status: Status, text = "x", extra: Partial<Phrase> = {}): Phrase => ({
  id: ++next,
  kind: "speech",
  status,
  text,
  outcome: "",
  ...extra,
});

const view = (overrides: Partial<View>): View => ({
  series: 1,
  visible: true,
  idle: false,
  mic: "off",
  transcribing: false,
  phrases: [],
  detail: "",
  agent: "claude",
  limited: false,
  sessionId: null,
  globalError: null,
  ...overrides,
});

test("waiting without speech", () => {
  const b = bubble(view({ mic: "waiting" }));
  assert.deepEqual([b.strip, b.mic, b.running, b.phrases.length], ["idle", "waiting", null, 0]);
});

test("rails follow each phrase's status", () => {
  const b = bubble(
    view({
      mic: "listening",
      phrases: [phrase("done"), phrase("running"), phrase("queued"), phrase("transcribing"), phrase("speaking")],
    }),
  );
  assert.deepEqual(
    b.phrases.map((p) => p.rail),
    ["ok", "run", "wait", "decode", "speak"],
  );
});

test("a speaking phrase without words has no row", () => {
  const b = bubble(view({ mic: "listening", phrases: [phrase("speaking", "")] }));
  assert.equal(b.phrases.length, 0);
});

test("the agent's work fills the right slot", () => {
  const b = bubble(view({ agent: "codex", detail: "Bash cargo test", phrases: [phrase("running")] }));
  assert.deepEqual(b.running, { icon: "terminal", text: "Codex is working · Bash cargo test", dots: false });
});

test("limited mode replaces the detail", () => {
  const b = bubble(view({ agent: "codex", limited: true, phrases: [phrase("running")] }));
  assert.equal(b.running?.text, "Codex is working · limited mode");
});

test("classifying reads as checking a command", () => {
  const b = bubble(view({ phrases: [phrase("classifying")] }));
  assert.deepEqual([b.phrases[0].rail, b.running], ["decode", { icon: "pencil", text: "Checking command", dots: true }]);
});

test("cancelling shows on the right", () => {
  const b = bubble(view({ phrases: [phrase("cancelling")] }));
  assert.deepEqual([b.phrases[0].rail, b.running], ["run", { icon: "terminal", text: "Cancelling", dots: false }]);
});

test("transcribing shows when the agent is idle", () => {
  const b = bubble(view({ mic: "waiting", transcribing: true, phrases: [phrase("transcribing")] }));
  assert.deepEqual([b.running, b.strip], [{ icon: "pencil", text: "Transcribing", dots: true }, "decode"]);
});

test("listening while transcribing keeps the mic label", () => {
  const b = bubble(view({ mic: "listening", transcribing: true, phrases: [phrase("transcribing"), phrase("speaking")] }));
  assert.deepEqual([b.mic, b.strip, b.running?.text], ["listening", "speak", "Transcribing"]);
});

test("queued terminal reads Open in terminal", () => {
  const b = bubble(view({ phrases: [phrase("running"), phrase("queued", "Open in terminal", { kind: "terminal" })] }));
  assert.equal(b.phrases[1].text, "Open in terminal");
});

test("done goes on the phrase", () => {
  const b = bubble(view({ idle: true, sessionId: "s", phrases: [phrase("done", "x", { outcome: "no lint errors" })] }));
  assert.deepEqual(
    [b.phrases[0].outcome, b.phrases[0].clickHint, b.running, b.clickable],
    ["Done · no lint errors", "Click to open in terminal", null, true],
  );
});

test("failure goes on the phrase", () => {
  const b = bubble(view({ phrases: [phrase("failed", "x", { outcome: "rate limit reached" })] }));
  assert.deepEqual([b.phrases[0].rail, b.phrases[0].outcome, b.globalError], ["err", "rate limit reached", null]);
});

test("cancelled phrase is struck through", () => {
  const b = bubble(view({ phrases: [phrase("cancelled")] }));
  assert.deepEqual([b.phrases[0].rail, b.phrases[0].outcome], ["gone", "Cancelled"]);
});

test("clickable only when the agent is idle and the queue is empty", () => {
  const done = phrase("done");
  assert.equal(bubble(view({ sessionId: "s", phrases: [done] })).clickable, true);
  assert.equal(bubble(view({ sessionId: "s", phrases: [done, phrase("queued")] })).clickable, false);
  assert.equal(bubble(view({ sessionId: "s", phrases: [done, phrase("running")] })).clickable, false);
  assert.equal(bubble(view({ phrases: [done] })).clickable, false);
});

test("the click hint goes on the newest finished phrase only", () => {
  const b = bubble(view({ sessionId: "s", phrases: [phrase("done"), phrase("failed")] }));
  assert.deepEqual(
    b.phrases.map((p) => p.clickHint),
    [null, "Click to open in terminal"],
  );
});

test("limited click hint", () => {
  const b = bubble(view({ sessionId: "s", agent: "codex", limited: true, phrases: [phrase("failed")] }));
  assert.equal(b.phrases[0].clickHint, "Click to open in Codex and trust");
});

test("a global error keeps the phrases", () => {
  const b = bubble(view({ mic: "error", globalError: "Microphone unavailable", phrases: [phrase("running")] }));
  assert.deepEqual(
    [b.phrases.length, b.globalError, b.strip, b.mic],
    [1, "Microphone unavailable", "error", "error"],
  );
});

test("long unbroken text stays one phrase", () => {
  const b = bubble(view({ phrases: [phrase("running", "C:/a/".repeat(80))] }));
  assert.equal(b.phrases.length, 1);
});

test("a failed transcription without words still shows its reason", () => {
  const b = bubble(view({ sessionId: "s", phrases: [phrase("done"), phrase("failed", "", { outcome: "Couldn't transcribe this phrase" })] }));
  assert.deepEqual(
    b.phrases.map((p) => [p.rail, p.text, p.clickHint]),
    [
      ["ok", "x", null],
      ["err", "Couldn't transcribe this phrase", "Click to open in terminal"],
    ],
  );
});
