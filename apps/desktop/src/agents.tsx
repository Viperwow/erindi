import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type Agent, type AgentSettings, type AgentStatus, Field, Spinner, input, pair, unsafePermissions } from "./controls";
import claudeIcon from "./icons/claude.svg";
import openaiIcon from "./icons/openai.svg";
import piIcon from "./icons/pi.svg";

const icons: Record<Agent, string> = { claude: claudeIcon, codex: openaiIcon, pi: piIcon };
const placeholders: Record<Agent, string> = { claude: "claude-opus-4-8", codex: "gpt-5.5", pi: "anthropic/claude-sonnet-5" };

export function AgentIcon(props: { agent: Agent; class?: string }) {
  // OpenAI allows its mark only in black or white; the other marks keep their brand colors.
  const tone = props.agent === "codex" ? "dark:invert" : "";
  return <img src={icons[props.agent]} alt="" class={`${tone} ${props.class ?? "h-4 w-4"}`} />;
}

/** Agent state from Rust, refreshed on `agents-changed` and re-checked when a tab opens or the window gains focus. */
export function useAgents() {
  const [agents, setAgents] = useState<AgentStatus[]>([]);
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    invoke<AgentStatus[]>("agent_status").then(setAgents);
    const off = listen<AgentStatus[]>("agents-changed", (e) => setAgents(e.payload));
    const onFocus = () => {
      setChecking(true);
      invoke<AgentStatus[]>("recheck_agents", { force: false })
        .then(setAgents)
        .finally(() => setChecking(false));
    };
    window.addEventListener("focus", onFocus);
    onFocus();
    return () => {
      off.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  const recheck = () =>
    invoke<AgentStatus[]>("recheck_agents", { force: true }).then((fresh) => {
      setAgents(fresh);
      return fresh;
    });
  return { agents, checking, recheck };
}

/** Re-check with a spinner while it runs, then "Installed ✓", "Not found" or "Failed" for 3 s; each label fades in and out. */
export function RecheckButton(props: { recheck: () => Promise<boolean> }) {
  const [phase, setPhase] = useState<"idle" | "checking" | "installed" | "missing" | "failed">("idle");
  const [visible, setVisible] = useState(true);
  const run = () => {
    setPhase("checking");
    const spinnerSeen = new Promise((done) => setTimeout(done, 400));
    Promise.all([props.recheck(), spinnerSeen])
      .then(([installed]) => setPhase(installed ? "installed" : "missing"), () => setPhase("failed"))
      .then(() => {
        setTimeout(() => setVisible(false), 3000);
        setTimeout(() => {
          setPhase("idle");
          setVisible(true);
        }, 3200);
      });
  };
  return (
    <button
      type="button"
      disabled={phase !== "idle"}
      aria-live="polite"
      title="Look for the agent CLIs again and reload their models and permission modes"
      class={`inline-flex w-32 shrink-0 items-center justify-center gap-1.5 rounded-md border border-neutral-300 px-3 py-1.5 enabled:hover:bg-neutral-100 dark:border-neutral-700 dark:enabled:hover:bg-neutral-800 ${phase === "checking" ? "opacity-70" : ""}`}
      onClick={run}
    >
      <span
        key={phase}
        class={`inline-flex items-center gap-1.5 transition-opacity motion-safe:animate-[fade-in_150ms_ease-out] ${visible ? "opacity-100 duration-150 ease-out" : "opacity-0 duration-200 ease-in"}`}
      >
        {phase === "checking" && <Spinner />}
        {phase === "idle" && "Re-check"}
        {phase === "checking" && "Checking"}
        {phase === "installed" && <span class="text-green-700 dark:text-green-400">Installed ✓</span>}
        {phase === "missing" && <span class="text-red-600">Not found</span>}
        {phase === "failed" && <span class="text-red-600">Failed</span>}
      </span>
    </button>
  );
}

const CUSTOM = "\u0000custom";
const DEFAULT = "";

export function AgentFields(props: {
  status: AgentStatus;
  /** The model list is being asked for again. */
  checking: boolean;
  value: AgentSettings;
  onChange: (v: AgentSettings) => void;
}) {
  const { status, value } = props;
  const model = value.model;
  const selected = model === null ? DEFAULT : "custom" in model ? CUSTOM : model.listed;
  const pick = (id: string) =>
    props.onChange({
      ...value,
      model: id === DEFAULT ? null : id === CUSTOM ? { custom: "" } : { listed: id },
    });
  const custom = model !== null && "custom" in model;
  // Only a list that is missing or failed shows the spinner, so a quick answer from the cache does not flash one.
  const loading = props.checking && (status.modelsError !== null || status.models.length === 0);
  return (
    <div class="space-y-3">
      <div class={pair}>
        <Field label="Model" error={loading ? undefined : (status.modelsError ?? undefined)}>
          <div class="relative">
            <select class={input} value={selected} aria-busy={loading} onChange={(e) => pick(e.currentTarget.value)}>
              <option value={DEFAULT}>Default ({status.label})</option>
              {status.models.map((m) => (
                <option value={m.id}>{m.label}</option>
              ))}
              <option value={CUSTOM}>Custom model ID…</option>
            </select>
            {loading && (
              <span class="pointer-events-none absolute inset-y-0 right-7 flex items-center text-neutral-500">
                <Spinner />
              </span>
            )}
          </div>
        </Field>
        <Field
          label="Permission"
          error={
            unsafePermissions.includes(value.permission)
              ? "The agent can change anything on this computer."
              : undefined
          }
        >
          <select
            class={input}
            value={value.permission}
            onChange={(e) => props.onChange({ ...value, permission: e.currentTarget.value })}
          >
            <option value="default">Default ({status.label} settings)</option>
            {status.permissions.map((p) => (
              <option value={p}>{p}</option>
            ))}
          </select>
        </Field>
      </div>
      <Field label="Model ID" hint={custom ? undefined : "Choose Custom model ID to type one."}>
        <input
          class={input}
          disabled={!custom}
          value={custom ? model.custom : ""}
          placeholder={placeholders[status.agent]}
          onInput={(e) => props.onChange({ ...value, model: { custom: e.currentTarget.value } })}
        />
      </Field>
    </div>
  );
}
