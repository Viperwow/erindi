import { test } from "node:test";
import assert from "node:assert/strict";
import { modelLabel, sessionLine } from "./model.ts";

test("Claude model IDs read as names", () => {
  assert.equal(modelLabel("claude-opus-5-5"), "Claude Opus 5.5");
  assert.equal(modelLabel("claude-sonnet-4-6"), "Claude Sonnet 4.6");
  assert.equal(modelLabel("claude-haiku-4-5-20251001"), "Claude Haiku 4.5");
  assert.equal(modelLabel("claude-fable-5"), "Claude Fable 5");
});

test("other IDs stay as they are", () => {
  assert.equal(modelLabel("gpt-5.6-sol"), "gpt-5.6-sol");
});

test("session line reads agent · model · permission", () => {
  assert.equal(sessionLine("Claude", "claude-opus-5-5", "plan"), "Claude · Opus 5.5 · plan");
  assert.equal(sessionLine("Codex", "gpt-5.6-sol", "workspace-write", "GPT-5.6-Sol"), "Codex · GPT-5.6-Sol · workspace-write");
  assert.equal(sessionLine("Claude", null, "default"), "Claude · Default model · default");
});
