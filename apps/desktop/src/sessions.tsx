import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ago } from "./time";

type Entry = {
  id: string;
  cwd: string;
  prompts: string[];
  createdMs: number;
  updatedMs: number;
};

type Sessions = { entries: Entry[]; active: string | null };

const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

const button =
  "rounded-md border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800";

export function SessionsView() {
  const [data, setData] = useState<Sessions | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = () => invoke<Sessions>("list_sessions").then(setData);
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
          const more = entry.prompts.length - 1;
          return (
            <li
              class={`rounded-lg border p-3 ${
                active
                  ? "border-blue-500 bg-blue-50 dark:bg-blue-950/40"
                  : "border-neutral-200 dark:border-neutral-800"
              }`}
            >
              <button
                type="button"
                class="block w-full text-left"
                aria-expanded={expanded}
                onClick={() => setOpen(expanded ? null : entry.id)}
              >
                <p class={expanded ? "" : "line-clamp-2"}>{entry.prompts[0]}</p>
                {expanded && more > 0 && (
                  <ol class="mt-2 space-y-1 border-l-2 border-neutral-300 pl-3 text-neutral-600 dark:border-neutral-700 dark:text-neutral-400">
                    {entry.prompts.slice(1).map((p) => (
                      <li>{p}</li>
                    ))}
                  </ol>
                )}
                <p class="mt-1 text-xs text-neutral-500">
                  {[
                    folderName(entry.cwd),
                    ago(entry.updatedMs),
                    !expanded && more > 0 && `+${more} more`,
                    entry.id.slice(0, 8),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </button>
              <div class="mt-2 flex gap-2">
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
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
