import { type Agent, agentLabels } from "./agent.ts";

export type AppState =
  | "LoadingModel"
  | "NoModel"
  | "Idle"
  | "Listening"
  | "Transcribing"
  | "Classifying"
  | "Running"
  | "Cancelling"
  | "Succeeded"
  | "Failed";

export type View = {
  op: number;
  state: AppState;
  text: string;
  detail: string;
  sessionId: string | null;
  continued: boolean;
  agent: Agent;
  limited: boolean;
};

export type Rail = "run" | "speak" | "decode" | "wait" | "ok" | "err" | "gone";
export type Phrase = { text: string; rail: Rail; outcome: string | null; clickHint: string | null; newest: boolean };
export type Running = { icon: "terminal" | "pencil"; text: string; dots: boolean } | null;
export type Mic = "waiting" | "listening" | "off" | "error";
export type Strip = "idle" | "speak" | "decode" | "off" | "error";
export type Bubble = {
  phrases: Phrase[];
  running: Running;
  globalError: string | null;
  mic: Mic;
  strip: Strip;
  clickable: boolean;
};

/** The audio level above which the microphone counts as hearing speech. */
export const SPEECH_LEVEL = 0.02;

const withDetail = (text: string, detail: string) => (detail ? `${text} · ${detail}` : text);

export function bubble(view: View, level: number): Bubble {
  const label = agentLabels[view.agent];
  const empty: Bubble = { phrases: [], running: null, globalError: null, mic: "off", strip: "off", clickable: false };
  const phrase = (rail: Rail, outcome: string | null = null, clickHint: string | null = null): Phrase[] =>
    view.text ? [{ text: view.text, rail, outcome, clickHint, newest: true }] : [];
  const clickable = view.sessionId !== null;
  const hint = clickable ? (view.limited ? "Click to open in Codex and trust" : "Click to open in terminal") : null;

  switch (view.state) {
    case "Listening": {
      const speaking = level >= SPEECH_LEVEL;
      return { ...empty, phrases: phrase("speak"), mic: speaking ? "listening" : "waiting", strip: speaking ? "speak" : "idle" };
    }
    case "Transcribing":
      return { ...empty, phrases: phrase("decode"), running: { icon: "pencil", text: "Transcribing", dots: true }, strip: "decode" };
    case "Classifying":
      return { ...empty, phrases: phrase("decode"), running: { icon: "pencil", text: "Checking command", dots: true }, strip: "decode" };
    case "Running": {
      const status = withDetail(`${label} is working`, view.limited ? "limited mode" : view.detail);
      return { ...empty, phrases: phrase("run"), running: { icon: "terminal", text: status, dots: false } };
    }
    case "Cancelling":
      return { ...empty, phrases: phrase("gone"), running: { icon: "terminal", text: "Cancelling", dots: false } };
    case "Succeeded":
      return { ...empty, phrases: phrase("ok", withDetail("Done", view.detail), hint), clickable };
    case "Failed":
      // A failure before any phrase exists is not about a phrase: it is a global error.
      if (!view.text) return { ...empty, globalError: view.detail || "Failed", mic: "error", strip: "error" };
      return { ...empty, phrases: phrase("err", withDetail(`${label} failed`, view.detail), hint), clickable };
    default:
      return empty;
  }
}
