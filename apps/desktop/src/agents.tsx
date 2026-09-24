import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type Agent, type AgentSettings, type AgentStatus, Field, input, unsafePermissions } from "./controls";
import claudeIcon from "./icons/claude.svg";
import openaiIcon from "./icons/openai.svg";

export function AgentIcon(props: { agent: Agent; class?: string }) {
  const src = props.agent === "claude" ? claudeIcon : openaiIcon;
  return <img src={src} alt="" class={`dark:invert ${props.class ?? "h-4 w-4"}`} />;
}

/** Agent state from Rust, refreshed on `agents-changed` and re-checked when the window gains focus. */
export function useAgents() {
  const [agents, setAgents] = useState<AgentStatus[]>([]);
  useEffect(() => {
    invoke<AgentStatus[]>("agent_status").then(setAgents);
    const off = listen<AgentStatus[]>("agents-changed", (e) => setAgents(e.payload));
    const onFocus = () => invoke("recheck_agents", { force: false });
    window.addEventListener("focus", onFocus);
    onFocus();
    return () => {
      off.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  return { agents, recheck: () => invoke("recheck_agents", { force: true }) };
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
  return (
    <div class="space-y-3">
      <div class="grid grid-cols-2 gap-3">
        <Field label="Model" hint={status.modelsError ?? undefined}>
          <select class={input} value={selected} onChange={(e) => pick(e.currentTarget.value)}>
            <option value={DEFAULT}>{status.agent === "codex" ? "Default (config.toml)" : "Default"}</option>
            {status.models.map((m) => (
              <option value={m.id}>{m.label}</option>
            ))}
            <option value={CUSTOM}>Custom model ID…</option>
          </select>
        </Field>
        <Field
          label="Permission"
          hint={
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
      {model !== null && "custom" in model && (
        <Field label="Model ID">
          <input
            class={input}
            value={model.custom}
            placeholder={status.agent === "codex" ? "gpt-5.5" : "claude-opus-4-8"}
            onInput={(e) => props.onChange({ ...value, model: { custom: e.currentTarget.value } })}
          />
        </Field>
      )}
    </div>
  );
}
