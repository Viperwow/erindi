import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type Agent, type AgentSettings, type AgentStatus, Field, Spinner, input, pair, unsafePermissions } from "./controls";
import claudeIcon from "./icons/claude.svg";
import openaiIcon from "./icons/openai.svg";

export function AgentIcon(props: { agent: Agent; class?: string }) {
  const src = props.agent === "claude" ? claudeIcon : openaiIcon;
  // OpenAI allows its mark only in black or white; Claude's mark keeps its brand color.
  const tone = props.agent === "claude" ? "" : "dark:invert";
  return <img src={src} alt="" class={`${tone} ${props.class ?? "h-4 w-4"}`} />;
}

/** Agent state from Rust, refreshed on `agents-changed` and re-checked when the window gains focus. */
export function useAgents() {
  const [agents, setAgents] = useState<AgentStatus[]>([]);
  useEffect(() => {
    invoke<AgentStatus[]>("agent_status").then(setAgents);
    const off = listen<AgentStatus[]>("agents-changed", (e) => setAgents(e.payload));
    const onFocus = () => invoke<AgentStatus[]>("recheck_agents", { force: false }).then(setAgents);
    window.addEventListener("focus", onFocus);
    onFocus();
    return () => {
      off.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  const recheck = () => invoke<AgentStatus[]>("recheck_agents", { force: true }).then(setAgents);
  return { agents, recheck };
}

/** Re-check with a spinner while it runs, then "Re-checked ✓" or the error for 3 s; each label fades in and out. */
export function RecheckButton(props: { recheck: () => Promise<void> }) {
  const [phase, setPhase] = useState<"idle" | "checking" | "done" | "failed">("idle");
  const [visible, setVisible] = useState(true);
  const run = () => {
    setPhase("checking");
    const spinnerSeen = new Promise((done) => setTimeout(done, 400));
    Promise.all([props.recheck(), spinnerSeen])
      .then(() => setPhase("done"), () => setPhase("failed"))
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
        {phase === "done" && <span class="text-green-700 dark:text-green-400">Re-checked ✓</span>}
        {phase === "failed" && <span class="text-red-600">Failed</span>}
      </span>
    </button>
  );
}

const CUSTOM = "\u0000custom";
const DEFAULT = "";

export function AgentFields(props: {
  status: AgentStatus;
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
  return (
    <div class="space-y-3">
      <div class={pair}>
        <Field label="Model" error={status.modelsError ?? undefined}>
          <select class={input} value={selected} onChange={(e) => pick(e.currentTarget.value)}>
            <option value={DEFAULT}>Default ({status.label})</option>
            {status.models.map((m) => (
              <option value={m.id}>{m.label}</option>
            ))}
            <option value={CUSTOM}>Custom model ID…</option>
          </select>
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
          placeholder={status.agent === "codex" ? "gpt-5.5" : "claude-opus-4-8"}
          onInput={(e) => props.onChange({ ...value, model: { custom: e.currentTarget.value } })}
        />
      </Field>
    </div>
  );
}
