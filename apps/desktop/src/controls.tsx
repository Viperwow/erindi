import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { accelerator, heldModifiers } from "./hotkey";

export type SessionPolicy = "continue" | "continueIfRecent" | "alwaysNew";

export type Command = "newSession" | "openTerminal" | "cancel" | "claude" | "codex";

export type Patterns = Record<Command, string[]>;

export type Settings = {
  talkHotkey: string;
  newSessionHotkey: string;
  terminalHotkey: string;
  patterns: Patterns;
  sessionPolicy: SessionPolicy;
  recentMinutes: number;
  cwd: string;
  agent: Agent;
  agents: Partial<Record<Agent, AgentSettings>>;
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

export type Agent = "claude" | "codex";

export type ModelChoice = { listed: string } | { custom: string } | null;

export type AgentSettings = { model: ModelChoice; permission: string };

export type AgentStatus = {
  agent: Agent;
  label: string;
  path: string | null;
  models: { id: string; label: string }[];
  modelsError: string | null;
  permissions: string[];
};

export const unsafePermissions = ["bypassPermissions", "danger-full-access"];

export const policies: [SessionPolicy, string][] = [
  ["continue", "Continue the active session"],
  ["continueIfRecent", "Continue if used recently"],
  ["alwaysNew", "Always start a new session"],
];

export const input =
  "w-full rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-900 disabled:cursor-not-allowed disabled:bg-neutral-100 disabled:text-neutral-500 dark:disabled:bg-neutral-800 dark:disabled:text-neutral-400 aria-invalid:border-red-600 dark:aria-invalid:border-red-500";

/** The line under the field is always there, so a hint or error never pushes the form down. */
export function Field(props: { label: string; hint?: string; error?: string; children: ComponentChildren }) {
  return (
    <label class="block space-y-1">
      <span class="font-medium">{props.label}</span>
      {props.children}
      <span class={`block min-h-4 text-xs ${props.error ? "text-red-600" : "text-neutral-500"}`}>
        {props.error ?? props.hint}
      </span>
    </label>
  );
}

/** Two fields side by side, one above the other when the form is narrow. */
export const pair = "grid gap-3 @md:grid-cols-2";

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
  const { run: guard } = useBusy();

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

  const download = guard(() => {
    setError("");
    setProgress(0);
    invoke("download_model", { id: props.model.id }).catch((err) => {
      setProgress(null);
      setError(String(err));
    });
  });

  return (
    <div class="space-y-1">
      <div class="flex items-center gap-3">
        <select class={input} disabled aria-label="Model">
          <option>{props.model.label}</option>
        </select>
        {props.model.installed ? (
          <span key="installed" class={`shrink-0 text-green-700 dark:text-green-400 ${fadeIn}`}>
            Downloaded
          </span>
        ) : progress !== null ? (
          <progress
            key="progress"
            class={`w-32 shrink-0 ${fadeIn}`}
            value={progress}
            max={1}
            aria-label="Download progress"
          />
        ) : (
          <button
            key="download"
            type="button"
            class="shrink-0 rounded-md border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            onClick={download}
          >
            Download
          </button>
        )}
        {progress !== null && <span class="w-10 shrink-0 tabular-nums">{Math.floor(progress * 100)}%</span>}
      </div>
      <Reveal open={!!error}>
        <p class="text-xs text-red-600">{error}</p>
      </Reveal>
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

/**
 * `run` wraps an action so that while it runs, `busy` is true and further calls are ignored.
 * `busy` lasts at least `minMs`, so a fast action still shows its spinner instead of a flicker.
 * A form submit is still prevented when its call is ignored.
 */
export function useBusy(minMs = 400) {
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const run =
    <A extends unknown[]>(action: (...args: A) => unknown) =>
    async (...args: A) => {
      if (args[0] instanceof SubmitEvent) args[0].preventDefault();
      if (lock.current) return;
      lock.current = true;
      setBusy(true);
      const until = Date.now() + minMs;
      try {
        await action(...args);
      } finally {
        setTimeout(
          () => {
            lock.current = false;
            setBusy(false);
          },
          Math.max(0, until - Date.now()),
        );
      }
    };
  return { run, busy };
}

export function Spinner() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" class="h-4 w-4 motion-safe:animate-spin">
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="2" opacity="0.25" />
      <path d="M14 8a6 6 0 0 0-6-6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
    </svg>
  );
}

/** Unfolds and fades its content in; while it folds away it keeps showing what it showed last. */
export function Reveal(props: { open: boolean; children: ComponentChildren }) {
  const last = useRef<ComponentChildren>(null);
  if (props.open) last.current = props.children;
  return (
    <div
      inert={!props.open}
      class={`grid transition-[grid-template-rows,opacity] motion-reduce:transition-opacity ${
        props.open ? "grid-rows-[1fr] opacity-100 duration-150 ease-out" : "grid-rows-[0fr] opacity-0 duration-200 ease-in"
      }`}
    >
      <div class="min-h-0 overflow-hidden">{last.current}</div>
    </div>
  );
}

/** A short fade-in for an element that just replaced another one. */
export const fadeIn = "motion-safe:animate-[fade-in_150ms_ease-out]";

export type Status = { ok: boolean; text: string } | null;

/** "Saved" fades out after 3 s; an error stays until the next edit. */
export function SaveBar(props: { status: Status; busy?: boolean }) {
  const [shown, setShown] = useState<Status>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!props.status) return setVisible(false);
    setShown(props.status);
    setVisible(true);
    if (!props.status.ok) return;
    const timer = setTimeout(() => setVisible(false), 3000);
    return () => clearTimeout(timer);
  }, [props.status]);
  return (
    <div class="flex items-center gap-3 pt-2">
      <button
        type="submit"
        disabled={props.busy}
        class="inline-flex items-center gap-1.5 rounded-md bg-neutral-900 px-4 py-1.5 font-medium text-white enabled:hover:bg-neutral-800 disabled:opacity-70 dark:bg-neutral-100 dark:text-neutral-900 dark:enabled:hover:bg-neutral-300"
      >
        {props.busy && <Spinner />}
        Save
      </button>
      <span
        id="save-status"
        aria-live="polite"
        class={`whitespace-pre-line transition-[opacity,visibility] ${visible ? "visible opacity-100 duration-150 ease-out" : "invisible opacity-0 duration-200 ease-in"} ${shown?.ok ? "text-green-700 dark:text-green-400" : "text-red-600"}`}
      >
        {shown?.text}
      </span>
    </div>
  );
}
