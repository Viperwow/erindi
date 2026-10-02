export type Agent = "claude" | "codex" | "pi" | "api";

export const agentLabels: Record<Agent, string> = { claude: "Claude", codex: "Codex", pi: "Pi", api: "Local model" };

/** The local model shows the name the user gave it. */
export function agentName(agent: Agent, settings: { apiName: string }): string {
  return agent === "api" ? settings.apiName.trim() || agentLabels.api : agentLabels[agent];
}
