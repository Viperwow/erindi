import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { SaveBar, type Settings, type Status, input } from "./controls";

export function DictionaryView() {
  const [s, setS] = useState<Settings | null>(null);
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS);
  }, []);

  if (!s) return null;
  const setDictionary = (dictionary: Settings["dictionary"]) => {
    setS({ ...s, dictionary });
    setStatus(null);
  };
  const setEntry = (i: number, j: 0 | 1, value: string) => {
    const dictionary = s.dictionary.map((e) => [...e] as [string, string]);
    dictionary[i][j] = value;
    setDictionary(dictionary);
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
        <h2 class="text-base font-semibold">Dictionary</h2>
        <p class="text-neutral-600 dark:text-neutral-400">Replaces what you say with how it should be written.</p>
      </div>

      <div class="space-y-2">
        {s.dictionary.map(([from, to], i) => (
          <div class="flex gap-2">
            <input
              class={input}
              value={from}
              placeholder="клод"
              aria-label="Spoken"
              onInput={(e) => setEntry(i, 0, e.currentTarget.value)}
            />
            <input
              class={input}
              value={to}
              placeholder="Claude"
              aria-label="Written"
              onInput={(e) => setEntry(i, 1, e.currentTarget.value)}
            />
            <button
              type="button"
              class="px-2 text-neutral-500 hover:text-red-600"
              aria-label="Remove entry"
              onClick={() => setDictionary(s.dictionary.filter((_, k) => k !== i))}
            >
              ✕
            </button>
          </div>
        ))}
        <button
          type="button"
          class="text-blue-600 hover:underline dark:text-blue-400"
          onClick={() => setDictionary([...s.dictionary, ["", ""]])}
        >
          Add word
        </button>
      </div>

      <SaveBar status={status} />
    </form>
  );
}
