import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { AgentFields, RecheckButton, useAgents } from "./agents";
import { CommandsView } from "./commands";
import { DictionaryView } from "./dictionary";
import {
  type Agent,
  type AgentStatus,
  Field,
  HotkeyInput,
  ModelRow,
  type ModelStatus,
  SaveBar,
  Section,
  type SessionPolicy,
  type Settings,
  type Status,
  input,
  pair,
  policies,
  Reveal,
  useBusy,
  Spinner,
} from "./controls";
import { SessionsView } from "./sessions";
import logo from "./logo.svg";
import "./style.css";

/** Shown until the first agent check answers, so the fields keep their place. */
const checking = (agent: Agent): AgentStatus => ({
  agent,
  label: agent === "codex" ? "Codex" : "Claude",
  path: "",
  models: [],
  modelsError: null,
  permissions: [],
});

function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const refreshModels = () => invoke<ModelStatus[]>("model_status").then(setModels);
  const { agents, recheck } = useAgents();
  const [limited, setLimited] = useState(false);
  const { run: guard, busy } = useBusy();
  // The notice follows the saved folder, so typing a path shows nothing until Save.
  const [cwd, setCwd] = useState<string>();
  useEffect(() => {
    if (cwd === undefined) return;
    const check = () => invoke<boolean>("codex_limited", { cwd }).then(setLimited);
    check();
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, [cwd]);

  useEffect(() => {
    invoke<Settings>("get_settings").then((loaded) => {
      setS(loaded);
      setCwd(loaded.cwd);
    });
    invoke<string[]>("list_microphones").then(setMics);
    refreshModels();
  }, []);

  if (!s) return null;
  const speech = models.find((m) => m.id === "speech");
  const agentStatus = agents.find((a) => a.agent === s.agent);
  const set = (patch: Partial<Settings>) => {
    setS({ ...s, ...patch });
    setStatus(null);
  };
  const save = async (e: Event) => {
    e.preventDefault();
    try {
      await invoke("save_settings", { settings: s });
      setStatus({ ok: true, text: "Saved" });
      setCwd(s.cwd);
    } catch (err) {
      setStatus({ ok: false, text: String(err) });
    }
  };

  return (
    <form
      onSubmit={guard(save)}
      class="@container max-w-4xl space-y-4 p-6"
    >
      <h2 class="text-base font-semibold">Settings</h2>

      <Section title="Agent" description="Which agent runs your requests and how.">
        <div>
          <Field label="Project folder" hint="Agents run here. Pick only folders you trust.">
            <input class={input} value={s.cwd} onInput={(e) => set({ cwd: e.currentTarget.value })} />
          </Field>
          <Reveal open={limited && agents.some((a) => a.agent === "codex" && a.path)}>
            <div class="mt-2 flex flex-wrap items-center gap-3 rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-amber-900 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200">
              <span class="min-w-0 flex-1">Codex runs this folder read-only, without its hooks and MCP servers, until you trust it.</span>
              <button
                type="button"
                disabled={busy}
                class="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-amber-400 px-3 py-1.5 disabled:opacity-70 hover:bg-amber-100 dark:border-amber-700 dark:hover:bg-amber-900/40"
                onClick={guard(() =>
                  invoke("trust_in_codex", { cwd }).catch((err) => setStatus({ ok: false, text: String(err) })),
                )}
              >
                {busy && <Spinner />}
                Trust in Codex
              </button>
            </div>
          </Reveal>
        </div>

        <Field
          label="Agent"
          hint="New sessions use it. Say “claude” or “codex” to pick one for a new session."
          error={
            agentStatus && !agentStatus.path
              ? `${agentStatus.label} CLI not found. Install it, then press Re-check.`
              : undefined
          }
        >
          <div class="flex items-center gap-3">
            <select class={input} aria-label="Agent" value={s.agent} onChange={(e) => set({ agent: e.currentTarget.value as Agent })}>
              {agents.map((a) => (
                <option value={a.agent}>{a.label}</option>
              ))}
            </select>
            <RecheckButton recheck={recheck} />
          </div>
        </Field>
        <AgentFields
          status={agentStatus ?? checking(s.agent)}
          value={s.agents[s.agent] ?? { model: null, permission: "default" }}
          onChange={(v) => set({ agents: { ...s.agents, [s.agent]: v } })}
        />

        <div class={pair}>
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
          <Field label="Recent means within (min)" hint="Only for “Continue if used recently”.">
            <input
              class={input}
              type="number"
              min="1"
              max="1440"
              disabled={s.sessionPolicy !== "continueIfRecent"}
              value={s.recentMinutes}
              onInput={(e) => set({ recentMinutes: Number(e.currentTarget.value) })}
            />
          </Field>
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
        <div class={pair}>
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

      <SaveBar status={status} busy={busy} />
    </form>
  );
}

const tabs = [
  ["sessions", "Sessions"],
  ["commands", "Commands"],
  ["dictionary", "Dictionary"],
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
      <main class="flex-1 overflow-y-auto [scrollbar-gutter:stable]">
        {tab === "sessions" ? (
          <SessionsView />
        ) : tab === "commands" ? (
          <CommandsView />
        ) : tab === "dictionary" ? (
          <DictionaryView />
        ) : (
          <SettingsView />
        )}
      </main>
    </div>
  );
}

render(<App />, document.getElementById("app")!);
