import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { accelerator, heldModifiers } from "./hotkey";

export type Mode = "default" | "acceptEdits" | "auto" | "plan" | "dontAsk" | "bypassPermissions";

export type SessionPolicy = "continue" | "continueIfRecent" | "alwaysNew";

export type Command = "newSession" | "openTerminal" | "cancel";

export type Patterns = Record<Command, string[]>;

export type Settings = {
  talkHotkey: string;
  newSessionHotkey: string;
  terminalHotkey: string;
  patterns: Patterns;
  sessionPolicy: SessionPolicy;
  recentMinutes: number;
  cwd: string;
  mode: Mode;
  model: string;
  microphone: string;
  silenceSecs: number;
  dictionary: [string, string][];
  modelCommands: boolean;
};

export type ModelStatus = {
  id: "speech" | "cleanup";
  label: string;
  installed: boolean;
  downloading: boolean;
};

export const modes: [Mode, string][] = [
  ["default", "Claude settings (no flag)"],
  ["acceptEdits", "Accept edits"],
  ["auto", "Auto"],
  ["plan", "Plan"],
  ["dontAsk", "Don't ask"],
  ["bypassPermissions", "Bypass permissions (unsafe)"],
];

export const policies: [SessionPolicy, string][] = [
  ["continue", "Continue the active session"],
  ["continueIfRecent", "Continue if used recently"],
  ["alwaysNew", "Always start a new session"],
];

export const input =
  "w-full rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-900 disabled:cursor-not-allowed disabled:bg-neutral-100 disabled:text-neutral-500 dark:disabled:bg-neutral-800 dark:disabled:text-neutral-400";

export function Field(props: { label: string; hint?: string; children: ComponentChildren }) {
  return (
    <label class="block space-y-1">
      <span class="font-medium">{props.label}</span>
      {props.children}
      {props.hint && <span class="block text-xs text-neutral-500">{props.hint}</span>}
    </label>
  );
}

export function Section(props: {
  title: string;
  description: string;
  highlight?: boolean;
  children: ComponentChildren;
}) {
  return (
    <section
      class={`space-y-3 rounded-lg border p-4 ${
        props.highlight ? "border-red-500" : "border-neutral-200 dark:border-neutral-800"
      }`}
    >
      <div>
        <h3 class="font-semibold">{props.title}</h3>
        <p class="text-xs text-neutral-500">{props.description}</p>
      </div>
      {props.children}
    </section>
  );
}

export function ModelRow(props: { model: ModelStatus; onInstalled: () => void }) {
  const [progress, setProgress] = useState<number | null>(props.model.downloading ? 0 : null);
  const [error, setError] = useState("");

  useEffect(() => {
    const offProgress = listen<{ id: string; done: number; total: number }>("model-progress", (e) => {
      if (e.payload.id === props.model.id) setProgress(e.payload.done / e.payload.total);
    });
    const offDone = listen<{ id: string; error: string | null }>("model-done", (e) => {
      if (e.payload.id !== props.model.id) return;
      setProgress(null);
      if (e.payload.error) setError(e.payload.error);
      else props.onInstalled();
    });
    return () => {
      offProgress.then((f) => f());
      offDone.then((f) => f());
    };
  }, []);

  const download = () => {
    setError("");
    setProgress(0);
    invoke("download_model", { id: props.model.id }).catch((err) => {
      setProgress(null);
      setError(String(err));
    });
  };

  return (
    <div class="space-y-1">
      <div class="flex items-center gap-3">
        <select class={input} disabled aria-label="Model">
          <option>{props.model.label}</option>
        </select>
        {props.model.installed ? (
          <span class="shrink-0 text-green-700 dark:text-green-400">Downloaded</span>
        ) : progress !== null ? (
          <progress class="w-32 shrink-0" value={progress} max={1} aria-label="Download progress" />
        ) : (
          <button
            type="button"
            class="shrink-0 rounded-md border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            onClick={download}
          >
            Download
          </button>
        )}
        {progress !== null && <span class="w-10 shrink-0 tabular-nums">{Math.floor(progress * 100)}%</span>}
      </div>
      {error && <p class="text-xs text-red-600">{error}</p>}
    </div>
  );
}

/** Click, then press the combination. Esc cancels. */
export function HotkeyInput(props: { value: string; label: string; onChange: (value: string) => void }) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState("");

  useEffect(() => {
    if (!recording) return;
    setHeld("");
    invoke("set_hotkeys_paused", { paused: true });
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") return setRecording(false);
      const combo = accelerator(e);
      if (combo) {
        props.onChange(combo);
        setRecording(false);
      } else {
        setHeld(heldModifiers(e));
      }
    };
    const onKeyUp = (e: KeyboardEvent) => setHeld(heldModifiers(e));
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKeyUp, true);
      invoke("set_hotkeys_paused", { paused: false });
    };
  }, [recording]);

  return (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={recording}
      class={`${input} text-left font-mono ${recording ? "ring-2 ring-blue-500 text-neutral-500" : ""}`}
      onClick={() => setRecording(!recording)}
      onBlur={() => setRecording(false)}
    >
      {recording ? (held ? `${held.replaceAll("+", " + ")} + …` : "Press keys… (Esc to cancel)") : props.value}
    </button>
  );
}

export type Status = { ok: boolean; text: string } | null;

export function SaveBar(props: { status: Status }) {
  return (
    <div class="flex items-center gap-3 pt-2">
      <button
        type="submit"
        class="rounded-md bg-neutral-900 px-4 py-1.5 font-medium text-white hover:bg-neutral-700 dark:bg-neutral-100 dark:text-neutral-900"
      >
        Save
      </button>
      {props.status && (
        <span
          class={`whitespace-pre-line ${props.status.ok ? "text-green-700 dark:text-green-400" : "text-red-600"}`}
        >
          {props.status.text}
        </span>
      )}
    </div>
  );
}
