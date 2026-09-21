import { render } from "preact";
import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Mode = "default" | "acceptEdits" | "auto" | "plan" | "dontAsk" | "bypassPermissions";

type Settings = {
  holdHotkey: string;
  toggleHotkey: string;
  cwd: string;
  mode: Mode;
  model: string;
  microphone: string;
  silenceSecs: number;
  dictionary: [string, string][];
};

const modes: [Mode, string][] = [
  ["default", "Claude settings (no flag)"],
  ["acceptEdits", "Accept edits"],
  ["auto", "Auto"],
  ["plan", "Plan"],
  ["dontAsk", "Don't ask"],
  ["bypassPermissions", "Bypass permissions (unsafe)"],
];

const input =
  "w-full rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-900";

function Field(props: { label: string; hint?: string; children: ComponentChildren }) {
  return (
    <label class="block space-y-1">
      <span class="font-medium">{props.label}</span>
      {props.children}
      {props.hint && <span class="block text-xs text-neutral-500">{props.hint}</span>}
    </label>
  );
}

function App() {
  const [s, setS] = useState<Settings | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS);
    invoke<string[]>("list_microphones").then(setMics);
  }, []);

  if (!s) return null;
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
      class="min-h-screen space-y-4 bg-neutral-50 p-6 text-sm text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100"
    >
      <h1 class="text-lg font-semibold">Ella</h1>

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
        <Field label="Hold to talk">
          <input
            class={input}
            value={s.holdHotkey}
            onInput={(e) => set({ holdHotkey: e.currentTarget.value })}
          />
        </Field>
        <Field label="Toggle hands-free">
          <input
            class={input}
            value={s.toggleHotkey}
            onInput={(e) => set({ toggleHotkey: e.currentTarget.value })}
          />
        </Field>
      </div>

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

      <fieldset class="space-y-2">
        <legend class="font-medium">Dictionary</legend>
        <p class="text-xs text-neutral-500">Replaces what you say with how it should be written.</p>
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
      </fieldset>

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

render(<App />, document.getElementById("app")!);
