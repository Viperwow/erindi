import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ago } from "./time";

type Prompt = string | { text: string; raw: string };

const textOf = (p: Prompt) => (typeof p === "string" ? p : p.text);

type Entry = {
  id: string;
  cwd: string;
  prompts: Prompt[];
  createdMs: number;
  updatedMs: number;
};

type Sessions = { entries: Entry[]; active: string | null };

const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

const button =
  "rounded-md border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800";
const dangerButton =
  "rounded-md border border-red-300 px-2.5 py-1 text-xs font-medium text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40";

/** How long the Delete button waits for the confirming second click. */
const CONFIRM_MS = 3000;

export function SessionsView() {
  const [data, setData] = useState<Sessions | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const load = () => invoke<Sessions>("list_sessions").then(setData);

  useEffect(() => {
    load();
    const off = listen("sessions-changed", load);
    return () => {
      off.then((f) => f());
    };
  }, []);

  const act = (command: string, id: string) =>
    invoke(command, { id }).then(
      () => setError(null),
      (e) => setError(String(e)),
    );

  const remove = (id: string) => {
    if (confirming !== id) {
      setConfirming(id);
      setTimeout(() => setConfirming((current) => (current === id ? null : current)), CONFIRM_MS);
      return;
    }
    setConfirming(null);
    act("delete_session", id).then(load);
  };

  if (!data) return null;
  if (data.entries.length === 0) {
    return (
      <div class="p-6 text-neutral-500">
        No sessions yet. Hold <kbd class="font-mono">Ctrl+Alt+Space</kbd> and say a task.
      </div>
    );
  }

  return (
    <div class="space-y-3 p-6">
      <h2 class="text-base font-semibold">Sessions</h2>
      {error && <p class="text-red-600">{error}</p>}
      <ul class="space-y-2">
        {data.entries.map((entry) => {
          const active = entry.id === data.active;
          const expanded = open === entry.id;
          const count = entry.prompts.length;
          return (
            <li
              class={`rounded-lg border p-3 ${
                active
                  ? "border-blue-500 bg-blue-50 dark:bg-blue-950/40"
                  : "border-neutral-200 dark:border-neutral-800"
              }`}
            >
              <p class="line-clamp-2 font-medium">{textOf(entry.prompts[0])}</p>
              <p class="mt-1 text-xs text-neutral-500">
                {[folderName(entry.cwd), ago(entry.updatedMs), entry.id.slice(0, 8)].join(" · ")}
              </p>
              <button
                type="button"
                class="mt-2 -ml-1.5 flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
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
              {expanded && (
                <ol class="mt-2 space-y-1.5 rounded-md bg-neutral-100 p-3 dark:bg-neutral-900">
                  {entry.prompts.map((p, i) => (
                    <li class="flex gap-2">
                      <span class="w-5 shrink-0 text-right text-xs leading-5 text-neutral-500 tabular-nums">
                        {i + 1}.
                      </span>
                      <span>
                        {textOf(p)}
                        {typeof p !== "string" && (
                          <span class="block text-xs text-neutral-500">Said: {p.raw}</span>
                        )}
                      </span>
                    </li>
                  ))}
                </ol>
              )}
              <div class="mt-3 flex gap-2">
                <button type="button" class={button} onClick={() => act("open_history_session", entry.id)}>
                  Open in terminal
                </button>
                <button
                  type="button"
                  class={button}
                  disabled={active}
                  onClick={() => act("continue_session", entry.id)}
                >
                  {active ? "Active" : "Continue by voice"}
                </button>
                <button type="button" class={`${dangerButton} ml-auto`} onClick={() => remove(entry.id)}>
                  {confirming === entry.id ? "Confirm delete" : "Delete"}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
