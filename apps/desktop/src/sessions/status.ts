import { bubble, type Status, type View } from "../bubble.ts";

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

/** The bubble's state for the session it serves. */
export function markOf(view: View | null, sessionId: string): Mark {
  if (!view || view.sessionId !== sessionId) return "idle";
  const b = bubble(view);
  if (b.running?.icon === "terminal") return "run";
  if (b.strip === "speak") return "speak";
  if (b.strip === "decode" || b.running) return "decode";
  return b.mic === "waiting" ? "waiting" : "idle";
}

/** Phrases the history records once their run starts. */
const recorded: Status[] = ["running", "cancelling", "done", "failed"];

/**
 * The session's phrases the history does not keep yet, in order. A phrase cancelled while it ran is
 * already the session's last prompt (`lastText`); one cancelled before it ran never reached history.
 */
export function liveOf(view: View | null, sessionId: string, lastText?: string): { rail: Rail; text: string }[] {
  if (!view || view.sessionId !== sessionId) return [];
  const live = view.phrases
    // A held key opens a phrase before any words arrive; like the bubble, it shows once it has text.
    .filter((p) => p.text && !recorded.includes(p.status) && !(p.status === "cancelled" && p.text === lastText))
    .map((p) => ({ rail: railOf(p.status), text: p.text }));
  if (!live.length && view.mic === "waiting") return [{ rail: "waiting", text: "Waiting" }];
  return live;
}
