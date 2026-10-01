import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { AgentFields, RecheckButton, useAgents } from "./agents";
import { CommandsView } from "./commands";
import { DictionaryView } from "./dictionary";
import {
  type Agent,
  type AgentStatus,
  agentLabels,
  Field,
  GestureSelect,
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
  label: agentLabels[agent],
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
  const { agents, checking: checkingAgents, recheck } = useAgents();
  const [limited, setLimited] = useState(false);
  const { run: guard, busy } = useBusy();
  // The notice follows the saved folder, so typing a path shows nothing until Save.
  const [cwd, setCwd] = useState<string>();
  useEffect(() => {
    if (cwd === undefined) return;
    let current = true;
    setLimited(false);
    const check = () => invoke<boolean>("codex_limited", { cwd }).then((l) => current && setLimited(l));
    check();
    window.addEventListener("focus", check);
    return () => {
      current = false;
      window.removeEventListener("focus", check);
    };
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
          <Reveal open={limited && s.cwd === cwd && s.agent === "codex" && !!agentStatus?.path}>
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
          hint="New sessions use it. Say “claude”, “codex” or “pi” to pick one for a new session. Installed or updated a CLI? Re-check finds it and reloads its models."
          error={
            agentStatus && !agentStatus.path
              ? `${agentStatus.label} CLI not found. Install it, then press Re-check.`
              : undefined
          }
        >
          <div class="flex items-center gap-3">
            <select class={input} aria-label="Agent" value={s.agent} onChange={(e) => set({ agent: e.currentTarget.value as Agent })}>
              {Object.entries(agentLabels).map(([agent, label]) => (
                <option value={agent}>{label}</option>
              ))}
            </select>
            <RecheckButton
              recheck={() => recheck().then((fresh) => !!fresh.find((a) => a.agent === s.agent)?.path)}
            />
          </div>
        </Field>
        <AgentFields
          status={agentStatus ?? checking(s.agent)}
          checking={checkingAgents}
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
          <Field label="Silence before sending (s)" hint="A pause this long sends the phrase in hands-free mode.">
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
          <Field label="Hide after (s)" hint="Once nothing is running and hands-free is off, the bubble counts down, then hides.">
            <input
              class={input}
              type="number"
              min="2"
              max="120"
              step="1"
              value={s.hideSecs}
              onInput={(e) => set({ hideSecs: Number(e.currentTarget.value) })}
            />
          </Field>
          <Field label="Double-press window (s)" hint="How soon the second press of a Double-tap must follow. A Tap on the same shortcut waits this long.">
            <input
              class={input}
              type="number"
              min="0.2"
              max="2"
              step="0.1"
              value={s.doubleSecs}
              onInput={(e) => set({ doubleSecs: Number(e.currentTarget.value) })}
            />
          </Field>
        </div>

      </Section>

      <Section title="Hotkeys" description="How you start, send and cancel a recording.">
        {(
          [
            ["Push to talk", "talkHotkey", "talkGesture"],
            ["Cancel", "cancelHotkey", "cancelGesture"],
            ["Hands-free", "handsFreeHotkey", "handsFreeGesture"],
          ] as const
        ).map(([name, hotkey, gesture]) => (
          <div class="flex min-w-0 items-center gap-3">
            <span class="w-32 shrink-0">{name}</span>
            <HotkeyInput label={name} value={s[hotkey]} onChange={(value) => set({ [hotkey]: value })} />
            <GestureSelect label={`${name} mode`} value={s[gesture]} onChange={(value) => set({ [gesture]: value })} />
          </div>
        ))}
        <ul class="list-disc space-y-0.5 pl-5 text-xs text-neutral-500">
          <li>Push to talk: Hold talks while held and sends on release; Tap or Double-tap starts a phrase and sends it on the next one.</li>
          <li>Cancel: cancels the phrase being transcribed or the running agent; otherwise the phrase you are saying.</li>
          <li>Hands-free: turns listening on or off; each pause sends a phrase.</li>
          <li>A Tap acts on release, unless a Double-tap shares its shortcut.</li>
        </ul>
        <p class="text-xs text-neutral-500">Command hotkeys are on the Commands tab.</p>
      </Section>

      <Section title="Startup" description="How Erindi starts.">
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            checked={s.launchAtLogin}
            onChange={(e) => set({ launchAtLogin: e.currentTarget.checked })}
          />
          Launch at login
        </label>
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            checked={s.openOnLaunch}
            onChange={(e) => set({ openOnLaunch: e.currentTarget.checked })}
          />
          Open this window on launch instead of starting in the tray
        </label>
      </Section>

      <Section title="Debugging" description="What Erindi records for diagnosing problems.">
        <Field label="Debug log" hint="Hotkeys, timing and transcripts. Empty turns it off.">
          <input class={input} value={s.logPath} onInput={(e) => set({ logPath: e.currentTarget.value })} />
        </Field>
      </Section>

      <SaveBar status={status} busy={busy} />
    </form>
  );
}

const tabs = [
  ["settings", "Settings"],
  ["sessions", "Sessions"],
  ["commands", "Commands"],
  ["dictionary", "Dictionary"],
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
