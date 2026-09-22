import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import {
  type Command,
  HotkeyInput,
  ModelRow,
  type ModelStatus,
  type Patterns,
  SaveBar,
  Section,
  type Settings,
  type Status,
  input,
} from "./controls";

const commands: { id: Command; name: string; example: string; hotkey?: "newSessionHotkey" | "terminalHotkey" }[] = [
  { id: "newSession", name: "New session", example: "new session, check the diff", hotkey: "newSessionHotkey" },
  { id: "openTerminal", name: "Open in terminal", example: "open in terminal", hotkey: "terminalHotkey" },
  { id: "cancel", name: "Cancel", example: "check the diff… cancel" },
];

const names: Record<Command, string> = {
  newSession: "New session",
  openTerminal: "Open in terminal",
  cancel: "Cancel",
};

/** What the parser makes of a phrase, with the patterns on screen. */
function TryPhrase(props: { patterns: Patterns }) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<{ commands: Command[]; rest: string } | string | null>(null);

  useEffect(() => {
    if (!text.trim()) return setResult(null);
    invoke<{ commands: Command[]; rest: string }>("test_command", { patterns: props.patterns, text })
      .then(setResult)
      .catch((err) => setResult(String(err)));
  }, [text, props.patterns]);

  return (
    <div class="space-y-1">
      <input
        class={input}
        value={text}
        placeholder="Try a phrase, e.g. «new session, check the diff»"
        aria-label="Try a phrase"
        onInput={(e) => setText(e.currentTarget.value)}
      />
      {typeof result === "string" && <p class="text-xs text-red-600">{result}</p>}
      {result && typeof result !== "string" && (
        <p class="text-xs text-neutral-600 dark:text-neutral-400">
          {result.commands.length === 0
            ? "No command; the whole phrase goes to Claude."
            : result.commands.includes("cancel")
              ? "Cancel: nothing is sent."
              : `${result.commands.map((c) => names[c]).join(" + ")}${
                  result.rest ? ` · Claude gets: «${result.rest}»` : " · nothing goes to Claude"
                }`}
        </p>
      )}
    </div>
  );
}

function Chips(props: { values: string[]; onChange: (values: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const value = draft.trim();
    if (value && !props.values.includes(value)) props.onChange([...props.values, value]);
    setDraft("");
  };
  return (
    <div class="flex flex-wrap items-center gap-1.5">
      {props.values.map((value) => (
        <span class="inline-flex items-center gap-1 rounded-full bg-neutral-200 px-2.5 py-0.5 font-mono text-xs dark:bg-neutral-800">
          {value}
          <button
            type="button"
            class="text-neutral-500 hover:text-red-600"
            aria-label={`Remove ${value}`}
            onClick={() => props.onChange(props.values.filter((v) => v !== value))}
          >
            ✕
          </button>
        </span>
      ))}
      <input
        class="min-w-40 flex-1 rounded-md border border-dashed border-neutral-300 bg-transparent px-2 py-0.5 font-mono text-xs dark:border-neutral-700"
        value={draft}
        placeholder="Add pattern, Enter"
        aria-label="Add pattern"
        onInput={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            add();
          }
        }}
      />
    </div>
  );
}

export function CommandsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [status, setStatus] = useState<Status>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const refreshModels = () => invoke<ModelStatus[]>("model_status").then(setModels);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS);
    refreshModels();
  }, []);

  if (!s) return null;
  const model = models.find((m) => m.id === "cleanup");
  const set = (patch: Partial<Settings>) => {
    setS({ ...s, ...patch });
    setStatus(null);
  };
  const save = async (e: Event) => {
    e.preventDefault();
    try {
      await invoke("save_settings", { settings: s });
      setStatus({ ok: true, text: "Saved" });
    } catch (err) {
      setStatus({ ok: false, text: String(err) });
    }
  };

  return (
    <form onSubmit={save} class="max-w-2xl space-y-4 p-6">
      <div class="space-y-1">
        <h2 class="text-base font-semibold">Commands</h2>
        <p class="text-neutral-600 dark:text-neutral-400">
          Say a command at the start or end of a phrase. In the middle it counts as part of the task.
        </p>
        <p class="text-xs text-neutral-500">
          Patterns are regular expressions. <code>\w*</code> matches any word ending, <code>(a|b)</code> matches
          either word, <code>?</code> makes the previous part optional. Words like "and" or "then" between a command
          and the task are skipped.
        </p>
      </div>

      <TryPhrase patterns={s.patterns} />

      {commands.map((c) => (
        <Section title={c.name} description={`Example: «${c.example}»`}>
          {c.hotkey ? (
            <div class="flex items-center gap-3">
              <span class="w-20 shrink-0 text-neutral-500">Hotkey</span>
              <HotkeyInput
                label={`${c.name} hotkey`}
                value={s[c.hotkey]}
                onChange={(value) => set({ [c.hotkey!]: value })}
              />
            </div>
          ) : (
            <p class="text-xs text-neutral-500">Hotkey: press the talk key once.</p>
          )}
          <Chips
            values={s.patterns[c.id]}
            onChange={(values) => set({ patterns: { ...s.patterns, [c.id]: values } })}
          />
        </Section>
      ))}

      <button
        type="button"
        class="text-blue-600 hover:underline dark:text-blue-400"
        onClick={() => invoke<Patterns>("default_patterns").then((patterns) => set({ patterns }))}
      >
        Reset patterns to defaults
      </button>

      <Section
        title="Commands in your own words"
        description="A local model recognises commands such as «let's start fresh». Adds about 0.1 s."
      >
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            checked={s.modelCommands}
            disabled={!model?.installed}
            onChange={(e) => set({ modelCommands: e.currentTarget.checked })}
          />
          Understand commands in my own words
        </label>
        {model && <ModelRow model={model} onInstalled={refreshModels} />}
      </Section>

      <SaveBar status={status} />
    </form>
  );
}
