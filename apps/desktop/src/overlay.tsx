import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type AppState =
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

type View = {
  op: number;
  state: AppState;
  text: string;
  detail: string;
  sessionId: string | null;
  continued: boolean;
};

const palette: Record<AppState, [string, string]> = {
  LoadingModel: ["#64748b", "#94a3b8"],
  NoModel: ["#64748b", "#94a3b8"],
  Idle: ["#64748b", "#94a3b8"],
  Listening: ["#06b6d4", "#3b82f6"],
  Transcribing: ["#a855f7", "#6366f1"],
  Classifying: ["#a855f7", "#f59e0b"],
  Running: ["#f59e0b", "#eab308"],
  Cancelling: ["#f59e0b", "#78716c"],
  Succeeded: ["#22c55e", "#10b981"],
  Failed: ["#ef4444", "#f43f5e"],
};

const labels: Partial<Record<AppState, string>> = {
  Transcribing: "Transcribing…",
  Classifying: "Checking command…",
  Running: "Claude is working",
  Cancelling: "Cancelling…",
  Succeeded: "Done",
  Failed: "Failed",
};

function Overlay() {
  const [view, setView] = useState<View | null>(null);
  const [level, setLevel] = useState(0);

  useEffect(() => {
    const offView = listen<View>("view", (e) => setView(e.payload));
    const offLevel = listen<number>("level", (e) => setLevel(e.payload));
    return () => {
      offView.then((f) => f());
      offLevel.then((f) => f());
    };
  }, []);

  if (!view) return null;
  const [a, b] = palette[view.state];
  const listening = view.state === "Listening";
  const intensity = listening ? Math.min(1, 0.6 + level * 8) : 0.8;
  const status = labels[view.state];
  const target =
    view.sessionId && view.state !== "Listening"
      ? view.continued
        ? `↩ ${view.sessionId.slice(0, 6)}`
        : "+ new session"
      : null;
  const hasBubble = view.text || view.detail || status;
  const canOpen =
    view.sessionId && (view.state === "Succeeded" || view.state === "Failed");

  return (
    <div class="fixed inset-0 flex flex-col items-center justify-end select-none">
      {hasBubble && (
        <div
          class={`mb-3 max-w-2xl rounded-2xl bg-neutral-950/80 px-4 py-2 text-sm text-white shadow-lg backdrop-blur ${canOpen ? "cursor-pointer hover:bg-neutral-900/90" : ""}`}
          onClick={canOpen ? () => invoke("open_session") : undefined}
        >
          {view.text && <p class="leading-snug">{view.text}</p>}
          {(status || view.detail) && (
            <p class="mt-0.5 truncate text-xs text-white/60">
              {[status, target, view.detail, canOpen && "click to open in terminal"]
                .filter(Boolean)
                .join(" · ")}
            </p>
          )}
        </div>
      )}
      <div
        class={`aurora h-12 w-full ${listening ? "" : "aurora-sweep"}`}
        style={{
          "--a": a,
          "--b": b,
          opacity: intensity,
          transform: `scaleY(${listening ? 0.6 + intensity * 0.4 : 1})`,
        }}
      />
    </div>
  );
}

render(<Overlay />, document.getElementById("app")!);
