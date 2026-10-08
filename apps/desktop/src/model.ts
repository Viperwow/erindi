/** `claude-opus-5-5` reads as "Claude Opus 5.5"; other IDs stay as they are. */
export const modelLabel = (raw: string) => {
  if (!raw.startsWith("claude-")) return raw;
  const parts = raw
    .slice(7)
    .split("-")
    .filter((p) => !/^\d{8}$/.test(p));
  const words = parts.filter((p) => !/^\d+$/.test(p)).map((p) => p[0].toUpperCase() + p.slice(1));
  const version = parts.filter((p) => /^\d+$/.test(p)).join(".");
  return ["Claude", ...words, version].filter(Boolean).join(" ");
};

/** "Claude · Opus 5.5 · plan": the agent, its model without the agent's name, the permission. */
export const sessionLine = (agent: string, model: string | null, permission: string, listed?: string) => {
  const name = model ? (listed ?? modelShort(model, agent)) : "Default model";
  return [agent, name, permission].join(" · ");
};

/** One phrase of a session, as the history stores it. */
export type Prompt =
  | string
  | { text: string; raw: string }
  | { text: string; reply: string; raw?: string; model?: string; failed?: boolean };

export const textOf = (p: Prompt) => (typeof p === "string" ? p : p.text);

/** The local model's reply to the phrase, if it has one. */
export const replyOf = (p: Prompt) => (typeof p !== "string" && "reply" in p ? p.reply : null);

/** What the person said before Erindi cleaned it up, if it did. */
export const rawOf = (p: Prompt) => (typeof p !== "string" && p.raw ? p.raw : null);

/** The model that answered the phrase, when the agent said. */
export const modelOf = (p: Prompt) => (typeof p !== "string" && "reply" in p && p.model ? p.model : null);

/** The answer is the run's error. */
export const failedOf = (p: Prompt) => typeof p !== "string" && "reply" in p && p.failed === true;

/** The model's name without its agent's: "Opus 5.5" for Claude's `claude-opus-5-5`. */
export const modelShort = (model: string, agent: string) => {
  const label = modelLabel(model);
  return label.startsWith(`${agent} `) ? label.slice(agent.length + 1) : label;
};
