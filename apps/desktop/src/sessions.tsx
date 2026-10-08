import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AgentIcon, useAgents } from "./agents";
import { type Agent, Keys, Reveal, type Settings, agentName, useBusy } from "./controls";
import { drawDiagrams } from "./diagram";
import { markdown } from "./markdown";
import { type Prompt, replyOf, sessionLine, textOf } from "./model";
import { ago } from "./time";

type Entry = {
  id: string;
  cwd: string;
  prompts: Prompt[];
  createdMs: number;
  updatedMs: number;
  agent: Agent;
  nativeId: string | null;
  startedModel: string | null;
  startedPermission: string | null;
};

type Details = { model: string | null; permission: string | null };

type Sessions = { entries: Entry[]; active: string | null; details: Record<string, Details> };


const button =
  "rounded-md border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800";
const dangerButton =
  "rounded-md border border-red-300 px-2.5 py-1 text-xs font-medium text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40";

/** How many seconds the Delete button waits for the confirming second click. */
const CONFIRM_S = 3;

/** A small mark at the end of the agent line; the text shows on hover and to screen readers. */
function Note(props: { tone: "error" | "info"; text: string }) {
  const color = props.tone === "error" ? "text-red-600" : "text-neutral-400";
  return (
    <span role="img" aria-label={props.text} title={props.text} class={`shrink-0 cursor-help ${color}`}>
      <svg aria-hidden="true" viewBox="0 0 16 16" class="h-3.5 w-3.5">
        <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.5" />
        {props.tone === "error" ? (
          <path d="M8 4.5v4.2M8 11.2v.3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
        ) : (
          <path d="M8 7.3v4.2M8 4.6v.3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
        )}
      </svg>
    </span>
  );
}

export function SessionsView() {
  const [data, setData] = useState<Sessions | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [formatted, setFormatted] = useState(true);
  // The local model's reply while it streams; the saved one replaces it when the run ends.
  const [streamed, setStreamed] = useState<{ id: string; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<{ id: string; left: number } | null>(null);
  const load = () => invoke<Sessions>("list_sessions").then(setData);
  const { agents } = useAgents();
  const [talk, setTalk] = useState("");
  const [apiName, setApiName] = useState("");
  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => {
      setTalk(s.talkHotkey);
      setApiName(s.apiName);
    });
  }, []);

  useEffect(() => {
    load();
    const off = listen("sessions-changed", () => {
      setStreamed(null);
      load();
    });
    const streaming = listen<{ id: string; text: string }>("session-reply", (e) => setStreamed(e.payload));
    window.addEventListener("focus", load);
    return () => {
      off.then((f) => f());
      streaming.then((f) => f());
      window.removeEventListener("focus", load);
    };
  }, []);

  useEffect(() => {
    if (!confirming) return;
    const tick = setTimeout(
      () => setConfirming(confirming.left > 1 ? { ...confirming, left: confirming.left - 1 } : null),
      1000,
    );
    return () => clearTimeout(tick);
  }, [confirming]);

  const { run: guard, busy } = useBusy(600);
  const act = (command: string, id: string) =>
    invoke(command, { id }).then(
      () => setError(null),
      (e) => setError(String(e)),
    );
  const run = guard(act);

  // A second click inside the guard's cooldown is not taken as the delete confirmation.
  const remove = guard((id: string) => {
    if (confirming?.id !== id) {
      setConfirming({ id, left: CONFIRM_S });
      return;
    }
    setConfirming(null);
    return act("delete_session", id).then(load);
  });

  if (!data) return null;
  if (data.entries.length === 0) {
    return (
      <div class="p-6 text-neutral-500">
        No sessions yet. Hold <Keys combo={talk} /> and say a task.
      </div>
    );
  }

  return (
    <div class="max-w-4xl space-y-3 p-6">
      <h2 class="text-base font-semibold">Sessions</h2>
      {error && <p class="text-red-600">{error}</p>}
      <ul class="space-y-2">
        {data.entries.map((entry) => {
          const active = entry.id === data.active;
          const expanded = open === entry.id;
          const count = entry.prompts.length;
          const live = data.details[entry.id];
          const model = live?.model ?? entry.startedModel;
          const status = agents.find((a) => a.agent === entry.agent);
          const listed = status?.models.find((m) => m.id === model)?.label;
          const agentLabel = entry.agent === "api" ? agentName("api", { apiName }) : (status?.label ?? agentName(entry.agent, { apiName }));
          // The local model has no permissions, and Erindi keeps its conversation, so there is no log to read.
          const api = entry.agent === "api";
          const permission = live?.permission ?? entry.startedPermission ?? "default";
          const resumable = entry.nativeId !== null;
          return (
            <li
              class={`rounded-lg border p-3 ${
                active
                  ? "border-blue-500 bg-blue-50 dark:bg-blue-950/40"
                  : "border-neutral-200 dark:border-neutral-800"
              }`}
            >
              <p class="line-clamp-2 font-medium">{textOf(entry.prompts[0])}</p>
              <p class="mt-1 flex h-4 min-w-0 items-center gap-1.5 whitespace-nowrap text-xs text-neutral-600 dark:text-neutral-400">
                <AgentIcon agent={entry.agent} class="h-4 w-4 shrink-0" />
                <span class="truncate">{api ? [agentLabel, model].filter(Boolean).join(" · ") : sessionLine(agentLabel, model, permission, listed)}</span>
                {!resumable ? (
                  <Note tone="error" text="This session didn't start, so it can't be continued." />
                ) : (
                  !live && !api && entry.agent !== "cursor" && <Note tone="info" text="Couldn't read the agent's log. Showing the values the session started with." />
                )}
              </p>
              <p class="mt-1 flex min-w-0 gap-1 text-xs text-neutral-500">
                <span class="truncate" title={entry.cwd}>
                  {entry.cwd}
                </span>
                <span class="shrink-0">· {[ago(entry.updatedMs), entry.id.slice(0, 8)].join(" · ")}</span>
              </p>
              <div class="mt-2 flex items-center justify-between">
                <button
                  type="button"
                  class="-ml-1.5 flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
                  aria-expanded={expanded}
                  onClick={() => setOpen(expanded ? null : entry.id)}
                >
                  <svg
                    aria-hidden="true"
                    viewBox="0 0 16 16"
                    class={`h-3.5 w-3.5 transition-transform ${expanded ? "rotate-90" : ""}`}
                  >
                    <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                  {expanded ? "Hide" : "Show"} {count} {count === 1 ? "prompt" : "prompts"}
                </button>
                {api && expanded && (
                  <button
                    type="button"
                    aria-pressed={formatted}
                    aria-label="Format Markdown"
                    title={formatted ? "Showing formatted replies. Show plain text" : "Showing plain text. Format Markdown"}
                    class={`rounded-md p-1 hover:bg-neutral-100 focus-visible:outline-2 focus-visible:outline-blue-500 dark:hover:bg-neutral-800 ${
                      formatted ? "text-blue-600 dark:text-blue-400" : "text-neutral-500"
                    }`}
                    onClick={() => setFormatted(!formatted)}
                  >
                    <svg aria-hidden="true" viewBox="0 0 208 128" class="h-4 w-6">
                      <rect x="5" y="5" width="198" height="118" rx="15" fill="none" stroke="currentColor" stroke-width="10" />
                      <path d="M30 98V30h20l20 25 20-25h20v68H90V59L70 84 50 59v39zm125 0-30-33h20V30h20v35h20z" fill="currentColor" />
                    </svg>
                  </button>
                )}
              </div>
              <Reveal open={expanded}>
                <ol class="mt-2 space-y-1.5 rounded-md bg-neutral-100 p-3 dark:bg-neutral-900">
                  {entry.prompts.map((p, i) => {
                    const last = i === entry.prompts.length - 1;
                    const streaming = last && replyOf(p) === null && streamed?.id === entry.id;
                    const reply = streaming ? streamed.text : replyOf(p);
                    return (
                      <li class="flex gap-2">
                        <span class="w-5 shrink-0 text-right text-xs leading-5 text-neutral-500 tabular-nums">
                          {i + 1}.
                        </span>
                        <span class="min-w-0 flex-1">
                          {textOf(p)}
                          {typeof p !== "string" && "raw" in p && (
                            <span class="block text-xs text-neutral-500">Said: {p.raw}</span>
                          )}
                          {reply !== null && (
                            <div aria-busy={streaming}>
                              {formatted ? (
                                <div
                                  class="markdown mt-1 text-neutral-700 dark:text-neutral-300"
                                  // A diagram still streaming does not parse yet, so it is drawn once the reply ends.
                                  ref={(el) => {
                                    if (el && !streaming) void drawDiagrams(el);
                                  }}
                                  dangerouslySetInnerHTML={{ __html: markdown(reply, streaming) }}
                                />
                              ) : (
                                <span class="mt-1 block whitespace-pre-wrap text-neutral-600 dark:text-neutral-400">
                                  {reply}
                                  {streaming && <span class="caret" aria-hidden="true" />}
                                </span>
                              )}
                            </div>
                          )}
                        </span>
                      </li>
                    );
                  })}
                </ol>
              </Reveal>
              <div class="mt-3 flex gap-2">
                {!api && (
                  <button
                    type="button"
                    class={button}
                    disabled={busy || !resumable}
                    onClick={() => run("open_history_session", entry.id)}
                  >
                    Open in terminal
                  </button>
                )}
                <button
                  type="button"
                  class={button}
                  disabled={busy || active || !resumable}
                  onClick={() => run("continue_session", entry.id)}
                >
                  {active ? "Active" : "Continue by voice"}
                </button>
                <button
                  type="button"
                  class={`${dangerButton} ml-auto tabular-nums`}
                  disabled={busy}
                  onClick={() => remove(entry.id)}
                >
                  {confirming?.id === entry.id ? `Confirm delete (${confirming.left})` : "Delete"}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
