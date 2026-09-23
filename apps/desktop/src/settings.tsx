import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { CommandsView } from "./commands";
import {
  Field,
  HotkeyInput,
  type Mode,
  ModelRow,
  type ModelStatus,
  SaveBar,
  Section,
  type SessionPolicy,
  type Settings,
  type Status,
  input,
  modes,
  policies,
} from "./controls";
import { SessionsView } from "./sessions";
import logo from "./logo.svg";
import "./style.css";

function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const refreshModels = () => invoke<ModelStatus[]>("model_status").then(setModels);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS);
    invoke<string[]>("list_microphones").then(setMics);
    refreshModels();
  }, []);

  if (!s) return null;
  const speech = models.find((m) => m.id === "speech");
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
          <Field label="Session" hint={'Say "new session" to start a new one.'}>
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

      <Section title="Hotkeys" description="How you start, send and cancel a recording.">
        <div class="flex items-center gap-3">
          <span class="w-44 shrink-0">Talk</span>
          <HotkeyInput label="Talk" value={s.talkHotkey} onChange={(talkHotkey) => set({ talkHotkey })} />
        </div>
        <ul class="list-disc space-y-0.5 pl-5 text-xs text-neutral-500">
          <li>Hold: talk while holding, release to send.</li>
          <li>Double-press: hands-free; sends after a pause.</li>
          <li>Double-press while recording hands-free: send now.</li>
          <li>Press once while recording or while Claude works: cancel.</li>
        </ul>
        <p class="text-xs text-neutral-500">Command hotkeys are on the Commands tab.</p>
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

      <SaveBar status={status} />
    </form>
  );
}

const tabs = [
  ["sessions", "Sessions"],
  ["commands", "Commands"],
  ["settings", "Settings"],
] as const;

type Tab = (typeof tabs)[number][0];

const tabFromHash = (): Tab => tabs.find(([id]) => `#${id}` === location.hash)?.[0] ?? "sessions";

function App() {
  const [tab, setTab] = useState<Tab>(tabFromHash());
  useEffect(() => {
    const onHash = () => setTab(tabFromHash());
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
      <main class="flex-1 overflow-y-auto">
        {tab === "sessions" ? <SessionsView /> : tab === "commands" ? <CommandsView /> : <SettingsView />}
      </main>
    </div>
  );
}

render(<App />, document.getElementById("app")!);
