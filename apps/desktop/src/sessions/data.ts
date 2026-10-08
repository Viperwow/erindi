import type { Agent } from "../agent.ts";
import { type Prompt, textOf } from "../model.ts";

export type Entry = {
  id: string;
  cwd: string;
  prompts: Prompt[];
  createdMs: number;
  updatedMs: number;
  agent: Agent;
  nativeId: string | null;
  startedModel: string | null;
  startedPermission: string | null;
};

export type Details = { model: string | null; permission: string | null };

export type Sessions = { entries: Entry[]; active: string | null; details: Record<string, Details> };

export const titleOf = (e: Entry) => (e.prompts.length ? textOf(e.prompts[0]) : "");

/** "214 turns" for the local model, whose answers Erindi keeps; "12 prompts" for an agent. */
export const countOf = (e: Entry) => {
  const n = e.prompts.length;
  const word = e.agent === "api" ? "turn" : "prompt";
  return `${n} ${word}${n === 1 ? "" : "s"}`;
};
