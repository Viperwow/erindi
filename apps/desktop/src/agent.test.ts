import { test } from "node:test";
import assert from "node:assert/strict";
import { agentName, pickableAgents } from "./agent.ts";

test("the picker offers installed agents, the local model and the selected one", () => {
  const statuses = [
    { agent: "claude" as const, path: "C:/bin/claude.exe" },
    { agent: "codex" as const, path: null },
    { agent: "pi" as const, path: null },
    { agent: "cursor" as const, path: null },
  ];
  assert.deepEqual(pickableAgents(statuses, "claude"), ["claude", "api"]);
  assert.deepEqual(pickableAgents(statuses, "pi"), ["claude", "pi", "api"]);
  assert.deepEqual(pickableAgents([], "claude"), ["claude", "codex", "pi", "cursor", "api"]);
});

test("the local model goes by the name the user gave it", () => {
  assert.equal(agentName("api", { apiName: "LM Studio" }), "LM Studio");
  assert.equal(agentName("api", { apiName: "  " }), "Local model");
  assert.equal(agentName("claude", { apiName: "LM Studio" }), "Claude");
  assert.equal(agentName("cursor", { apiName: "LM Studio" }), "Cursor");
});
