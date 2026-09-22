import { render } from "preact";
import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { accelerator, heldModifiers } from "./hotkey";
import { SessionsView } from "./sessions";
import logo from "./logo.svg";
import "./style.css";

type Mode = "default" | "acceptEdits" | "auto" | "plan" | "dontAsk" | "bypassPermissions";

type SessionPolicy = "continue" | "continueIfRecent" | "alwaysNew";

type Settings = {
  holdHotkey: string;
  toggleHotkey: string;
  newSessionHotkey: string;
  sessionPolicy: SessionPolicy;
  recentMinutes: number;
  cwd: string;
  mode: Mode;
  model: string;
  microphone: string;
  silenceSecs: number;
  dictionary: [string, string][];
  cleanup: boolean;
};

type ModelStatus = { id: "speech" | "cleanup"; label: string; installed: boolean };

const modes: [Mode, string][] = [
  ["default", "Claude settings (no flag)"],
  ["acceptEdits", "Accept edits"],
  ["auto", "Auto"],
  ["plan", "Plan"],
  ["dontAsk", "Don't ask"],
  ["bypassPermissions", "Bypass permissions (unsafe)"],
];

const policies: [SessionPolicy, string][] = [
  ["continue", "Continue the active session"],
  ["continueIfRecent", "Continue if used recently"],
  ["alwaysNew", "Always start a new session"],
];

const input =
  "w-full rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-900 disabled:cursor-not-allowed disabled:bg-neutral-100 disabled:text-neutral-500 dark:disabled:bg-neutral-800 dark:disabled:text-neutral-400";

function Field(props: { label: string; hint?: string; children: ComponentChildren }) {
  return (
    <label class="block space-y-1">
      <span class="font-medium">{props.label}</span>
      {props.children}
      {props.hint && <span class="block text-xs text-neutral-500">{props.hint}</span>}
    </label>
  );
}

function Section(props: {
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

function ModelRow(props: { model: ModelStatus; onInstalled: () => void }) {
  const [progress, setProgress] = useState<number | null>(null);
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
function HotkeyInput(props: { value: string; label: string; onChange: (value: string) => void }) {
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

function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const refreshModels = () => invoke<ModelStatus[]>("model_status").then(setModels);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS);
    invoke<string[]>("list_microphones").then(setMics);
    refreshModels();
  }, []);

  if (!s) return null;
  const speech = models.find((m) => m.id === "speech");
  const cleanup = models.find((m) => m.id === "cleanup");
  const set = (patch: Partial<Settings>) => {
    setS({ ...s, ...patch });
    setStatus(null);
  };
  const setEntry = (i: number, j: 0 | 1, value: string) => {
    const dictionary = s.dictionary.map((e) => [...e] as [string, string]);
    dictionary[i][j] = value;
    set({ dictionary });
  };
  const save = async (e: Event) => {
    e.preventDefault();
    try {
      await invoke("save_settings", { settings: s });
      setStatus({ ok: true, text: "Saved" });
    } catch (err) {
      setStatus({ ok: false, text: String(err) });
    }
  };

  return (
    <form
      onSubmit={save}
      class="max-w-2xl space-y-4 p-6"
    >
      <h2 class="text-base font-semibold">Settings</h2>

      <Section title="Agent" description="Where Claude runs and how it treats your requests.">
        <Field label="Project folder" hint="Claude runs here.">
          <input class={input} value={s.cwd} onInput={(e) => set({ cwd: e.currentTarget.value })} />
        </Field>

        <div class="grid grid-cols-2 gap-3">
          <Field label="Permission mode">
            <select
              class={input}
              value={s.mode}
              onChange={(e) => set({ mode: e.currentTarget.value as Mode })}
            >
              {modes.map(([value, label]) => (
                <option value={value}>{label}</option>
              ))}
            </select>
          </Field>
          <Field label="Model" hint="Empty uses the Claude default.">
            <input
              class={input}
              value={s.model}
              placeholder="default"
              onInput={(e) => set({ model: e.currentTarget.value })}
            />
          </Field>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <Field label="Session" hint={'Say "new session" or "same session" to override.'}>
            <select
              class={input}
              value={s.sessionPolicy}
              onChange={(e) => set({ sessionPolicy: e.currentTarget.value as SessionPolicy })}
            >
              {policies.map(([value, label]) => (
                <option value={value}>{label}</option>
              ))}
            </select>
          </Field>
          {s.sessionPolicy === "continueIfRecent" && (
            <Field label="Recent means within (min)">
              <input
                class={input}
                type="number"
                min="1"
                max="1440"
                value={s.recentMinutes}
                onInput={(e) => set({ recentMinutes: Number(e.currentTarget.value) })}
              />
            </Field>
          )}
        </div>

      </Section>

      <Section
        title="Voice"
        description="Speech recognition runs on this computer."
        highlight={speech && !speech.installed}
      >
        {speech && <ModelRow model={speech} onInstalled={refreshModels} />}
        {speech && !speech.installed && (
          <p class="text-xs text-red-600">Speech model is not installed. Download it to start dictating.</p>
        )}
        <div class="grid grid-cols-2 gap-3">
          <Field label="Microphone">
            <select
              class={input}
              value={s.microphone}
              onChange={(e) => set({ microphone: e.currentTarget.value })}
            >
              <option value="">System default</option>
              {mics.map((m) => (
                <option value={m}>{m}</option>
              ))}
            </select>
          </Field>
          <Field label="Silence before sending (s)" hint="Hands-free mode only.">
            <input
              class={input}
              type="number"
              min="0.5"
              max="10"
              step="0.5"
              value={s.silenceSecs}
              onInput={(e) => set({ silenceSecs: Number(e.currentTarget.value) })}
            />
          </Field>
        </div>

      </Section>

      <Section title="Hotkeys" description="Click a field, then press the combination.">
        {(
          [
            ["holdHotkey", "Hold to talk"],
            ["toggleHotkey", "Hands-free"],
            ["newSessionHotkey", "Hands-free, new session"],
          ] as const
        ).map(([field, label]) => (
          <div class="flex items-center gap-3">
            <span class="w-44 shrink-0">{label}</span>
            <HotkeyInput label={label} value={s[field]} onChange={(value) => set({ [field]: value })} />
          </div>
        ))}
      </Section>

      <Section
        title="Prompt cleanup"
        description="A local model removes slips and voice commands before the agent sees your words."
      >
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            checked={s.cleanup}
            disabled={!cleanup?.installed}
            onChange={(e) => set({ cleanup: e.currentTarget.checked })}
          />
          Clean up prompt
        </label>
        {cleanup && <ModelRow model={cleanup} onInstalled={refreshModels} />}
      </Section>

      <Section title="Dictionary" description="Replaces what you say with how it should be written.">
        {s.dictionary.map(([from, to], i) => (
          <div class="flex gap-2">
            <input
              class={input}
              value={from}
              placeholder="клод"
              aria-label="Spoken"
              onInput={(e) => setEntry(i, 0, e.currentTarget.value)}
            />
            <input
              class={input}
              value={to}
              placeholder="Claude"
              aria-label="Written"
              onInput={(e) => setEntry(i, 1, e.currentTarget.value)}
            />
            <button
              type="button"
              class="px-2 text-neutral-500 hover:text-red-600"
              aria-label="Remove entry"
              onClick={() => set({ dictionary: s.dictionary.filter((_, k) => k !== i) })}
            >
              ✕
            </button>
          </div>
        ))}
        <button
          type="button"
          class="text-blue-600 hover:underline dark:text-blue-400"
          onClick={() => set({ dictionary: [...s.dictionary, ["", ""]] })}
        >
          Add word
        </button>
      </Section>

      <div class="flex items-center gap-3 pt-2">
        <button
          type="submit"
          class="rounded-md bg-neutral-900 px-4 py-1.5 font-medium text-white hover:bg-neutral-700 dark:bg-neutral-100 dark:text-neutral-900"
        >
          Save
        </button>
        {status && (
          <span class={`whitespace-pre-line ${status.ok ? "text-green-700 dark:text-green-400" : "text-red-600"}`}>
            {status.text}
          </span>
        )}
      </div>
    </form>
  );
}

const tabs = [
  ["sessions", "Sessions"],
  ["settings", "Settings"],
] as const;

function App() {
  const initial = location.hash === "#settings" ? "settings" : "sessions";
  const [tab, setTab] = useState<(typeof tabs)[number][0]>(initial);
  useEffect(() => {
    const onHash = () => setTab(location.hash === "#settings" ? "settings" : "sessions");
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  return (
    <div class="flex h-screen bg-neutral-50 text-sm text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <nav class="flex w-44 shrink-0 flex-col gap-1 border-r border-neutral-200 p-3 dark:border-neutral-800">
        <div class="flex items-center gap-2 px-2 pb-3 font-semibold tracking-wide">
          <img src={logo} alt="" class="h-6 w-6" />
          Erindi
        </div>
        {tabs.map(([id, label]) => (
          <button
            type="button"
            aria-current={tab === id ? "page" : undefined}
            class={`rounded-md px-2 py-1.5 text-left ${
              tab === id
                ? "bg-neutral-200 font-medium dark:bg-neutral-800"
                : "text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900"
            }`}
            onClick={() => (location.hash = id)}
          >
            {label}
          </button>
        ))}
      </nav>
      <main class="flex-1 overflow-y-auto">{tab === "sessions" ? <SessionsView /> : <SettingsView />}</main>
    </div>
  );
}

render(<App />, document.getElementById("app")!);
