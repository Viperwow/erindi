import type { Status, View } from "../bubble.ts";

/** What a session is doing, shown left of its title. */
export type Mark = "waiting" | "speak" | "decode" | "run" | "idle";
/** A phrase's state, drawn as its rail as in the bubble. */
export type Rail = "waiting" | "speak" | "decode" | "queue" | "run" | "ok" | "err" | "gone";

const rails: Record<Status, Rail> = {
  speaking: "speak",
  transcribing: "decode",
  classifying: "decode",
  queued: "queue",
  running: "run",
  cancelling: "run",
  done: "ok",
  failed: "err",
  cancelled: "gone",
};

export const railOf = (s: Status): Rail => rails[s];

export function markOf(view: View | null, sessionId: string): Mark {
  if (!view || view.sessionId !== sessionId) return "idle";
  const has = (...s: Status[]) => view.phrases.some((p) => s.includes(p.status));
  if (has("running", "cancelling")) return "run";
  if (has("speaking") || view.mic === "listening") return "speak";
  if (has("transcribing", "classifying")) return "decode";
  return view.mic === "waiting" ? "waiting" : "idle";
}

/** Phrases the history records once their run starts. */
const recorded: Status[] = ["running", "cancelling", "done", "failed"];

/** The session's phrases the history does not keep yet, in order. */
export function liveOf(view: View | null, sessionId: string): { rail: Rail; text: string }[] {
  if (!view || view.sessionId !== sessionId) return [];
  const live = view.phrases
    .filter((p) => !recorded.includes(p.status))
    .map((p) => ({ rail: railOf(p.status), text: p.text }));
  if (!live.length && view.mic === "waiting") return [{ rail: "waiting", text: "Waiting" }];
  return live;
}
