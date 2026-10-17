export type Agent = "claude" | "codex" | "pi" | "cursor" | "api";

export const agentLabels: Record<Agent, string> = { claude: "Claude", codex: "Codex", pi: "Pi", cursor: "Cursor", api: "Local model" };

/**
 * Agents the picker offers: the local model, CLIs found on this machine, and the selected agent even when missing,
 * so its "not found" error stays visible. Before the first check every agent shows.
 */
export function pickableAgents(statuses: { agent: Agent; path: string | null }[], selected: Agent): Agent[] {
  return (Object.keys(agentLabels) as Agent[]).filter(
    (agent) =>
      agent === "api" || agent === selected || statuses.length === 0 || !!statuses.find((s) => s.agent === agent)?.path,
  );
}

/** The local model shows the name the user gave it. */
export function agentName(agent: Agent, settings: { apiName: string }): string {
  return agent === "api" ? settings.apiName.trim() || agentLabels.api : agentLabels[agent];
}
