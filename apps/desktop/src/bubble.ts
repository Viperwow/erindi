import { type Agent, agentName } from "./agent.ts";

export type Status =
  | "speaking"
  | "transcribing"
  | "queued"
  | "classifying"
  | "running"
  | "cancelling"
  | "done"
  | "failed"
  | "cancelled";

export type Phrase = { id: number; kind: "speech" | "terminal"; status: Status; text: string; outcome: string };
export type Mic = "waiting" | "listening" | "off" | "error";

export type View = {
  series: number;
  visible: boolean;
  idle: boolean;
  rest: number;
  hideAfterMs: number | null;
  mic: Mic;
  transcribing: boolean;
  phrases: Phrase[];
  detail: string;
  reply: string;
  apiName: string;
  agent: Agent;
  limited: boolean;
  sessionId: string | null;
  globalError: string | null;
};

export type Rail = "run" | "speak" | "decode" | "wait" | "ok" | "err" | "gone";
export type Row = { text: string; rail: Rail; outcome: string | null; clickHint: string | null };
export type Running = { icon: "terminal" | "pencil"; text: string; dots: boolean } | null;
export type Strip = "idle" | "speak" | "decode" | "off" | "error";
export type Bubble = {
  phrases: Row[];
  running: Running;
  globalError: string | null;
  mic: Mic;
  strip: Strip;
  clickable: boolean;
  countdown: string | null;
};

const rails: Record<Status, Rail> = {
  speaking: "speak",
  transcribing: "decode",
  queued: "wait",
  classifying: "decode",
  running: "run",
  cancelling: "run",
  done: "ok",
  failed: "err",
  cancelled: "gone",
};

const finished = (p: Phrase) => p.status === "done" || p.status === "failed" || p.status === "cancelled";

/** The newest words of a streaming reply, so the tooltip stays inside the bubble's window. */
const tail = (text: string, max = 300) => (text.length <= max ? text : `…${text.slice(-(max - 1)).trimStart()}`);

const withDetail = (text: string, detail: string) => (detail ? `${text} · ${detail}` : text);

function outcome(p: Phrase): string | null {
  switch (p.status) {
    case "done":
      return withDetail("Done", p.outcome);
    case "failed":
      return p.outcome || "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      return null;
  }
}

/** `hidesIn` is the seconds left before an idle bubble hides. */
export function bubble(view: View, hidesIn: number | null = null): Bubble {
  const agent = view.phrases.find((p) => p.status === "classifying" || p.status === "running" || p.status === "cancelling");
  const busy = agent !== undefined || view.phrases.some((p) => p.status === "queued");
  // The local model has no terminal, so its answer is nothing to click.
  const clickable = view.sessionId !== null && view.agent !== "api" && !busy && view.phrases.some(finished);
  const hint = clickable ? (view.limited ? "Click to open in Codex and trust" : "Click to open in terminal") : null;
  const newest = view.phrases.filter(finished).at(-1);

  // A finished phrase without words (a failed transcription) shows its outcome as its text.
  const phrases = view.phrases
    .map((p) => ({ p, text: p.text || (finished(p) ? (outcome(p) ?? "") : "") }))
    .filter(({ text }) => text)
    .map(({ p, text }) => ({
      text,
      rail: rails[p.status],
      outcome: p === agent && p.status === "running" && view.reply ? tail(view.reply) : outcome(p),
      clickHint: p === newest ? hint : null,
    }));

  let running: Running = null;
  if (agent?.status === "running") {
    const status = withDetail(`${agentName(view.agent, view)} is working`, view.limited ? "limited mode" : view.detail);
    running = { icon: "terminal", text: status, dots: false };
  } else if (agent?.status === "cancelling") {
    running = { icon: "terminal", text: "Cancelling", dots: false };
  } else if (agent?.status === "classifying") {
    running = { icon: "pencil", text: "Checking command", dots: true };
  } else if (view.transcribing) {
    running = { icon: "pencil", text: "Transcribing", dots: true };
  }

  const strip: Strip =
    view.mic === "error"
      ? "error"
      : view.mic === "listening"
        ? "speak"
        : view.transcribing
          ? "decode"
          : view.mic === "waiting"
            ? "idle"
            : "off";

  const countdown = running === null && hidesIn !== null ? `Hides in ${hidesIn}s` : null;
  return { phrases, running, globalError: view.globalError, mic: view.mic, strip, clickable, countdown };
}
