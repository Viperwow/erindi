import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

type AppState =
  | "LoadingModel"
  | "Idle"
  | "Listening"
  | "Transcribing"
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
};

const palette: Record<AppState, [string, string]> = {
  LoadingModel: ["#64748b", "#94a3b8"],
  Idle: ["#64748b", "#94a3b8"],
  Listening: ["#06b6d4", "#3b82f6"],
  Transcribing: ["#a855f7", "#6366f1"],
  Running: ["#f59e0b", "#eab308"],
  Cancelling: ["#f59e0b", "#78716c"],
  Succeeded: ["#22c55e", "#10b981"],
  Failed: ["#ef4444", "#f43f5e"],
};

const labels: Partial<Record<AppState, string>> = {
  Transcribing: "Transcribing…",
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
  const hasBubble = view.text || view.detail || status;

  return (
    <div class="fixed inset-0 flex flex-col items-center justify-end select-none">
      {hasBubble && (
        <div class="mb-3 max-w-2xl rounded-2xl bg-neutral-950/80 px-4 py-2 text-sm text-white shadow-lg backdrop-blur">
          {view.text && <p class="leading-snug">{view.text}</p>}
          {(status || view.detail) && (
            <p class="mt-0.5 truncate text-xs text-white/60">
              {[status, view.detail].filter(Boolean).join(" · ")}
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
