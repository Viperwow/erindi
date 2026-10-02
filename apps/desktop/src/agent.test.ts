import { test } from "node:test";
import assert from "node:assert/strict";
import { agentName } from "./agent.ts";

test("the local model goes by the name the user gave it", () => {
  assert.equal(agentName("api", { apiName: "LM Studio" }), "LM Studio");
  assert.equal(agentName("api", { apiName: "  " }), "Local model");
  assert.equal(agentName("claude", { apiName: "LM Studio" }), "Claude");
});
