import { usePopover } from "./Menu";
import type { Options } from "./search";

export const defaults: Options = {
  query: "",
  matchCase: false,
  word: false,
  regex: false,
  questions: true,
  answers: true,
  names: false,
};

const toggles: [keyof Options, string, string][] = [
  ["matchCase", "Aa", "Match case"],
  ["word", "ab", "Match whole word"],
  ["regex", ".*", "Use regular expression"],
];

const filters: [keyof Options, string][] = [
  ["questions", "Questions"],
  ["answers", "Answers"],
  ["names", "Session names"],
];

export function SearchBox(props: { options: Options; invalid: boolean; onChange: (o: Options) => void }) {
  const o = props.options;
  const set = (patch: Partial<Options>) => props.onChange({ ...o, ...patch });
  const changed = filters.filter(([k]) => o[k] !== defaults[k]).length;
  const filter = usePopover();
  return (
    <div class="relative">
      <div class="flex items-center gap-1 rounded-md border border-[var(--line)] py-1 pl-2.5 pr-1 focus-within:border-blue-500">
        <input
          type="search"
          aria-label="Search questions and answers"
          placeholder="Search questions and answers"
          aria-invalid={props.invalid}
          class="ses-strong min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[var(--muted)] [&::-webkit-search-cancel-button]:hidden"
          value={o.query}
          onInput={(e) => set({ query: e.currentTarget.value })}
          onKeyDown={(e) => {
            if (e.key === "Escape" && o.query) {
              e.stopPropagation();
              set({ query: "" });
            }
          }}
        />
        {/* The clear button keeps its slot, so the toggles never move. */}
        <button
          type="button"
          aria-label="Clear search"
          class={`ses-muted rounded px-1.5 hover:text-[var(--strong)] ${o.query ? "" : "invisible"}`}
          onClick={() => set({ query: "" })}
        >
          ✕
        </button>
        {toggles.map(([key, label, title]) => (
          <button
            type="button"
            title={title}
            aria-label={title}
            aria-pressed={o[key] === true}
            class={`rounded px-1.5 py-1 font-mono text-xs ${o[key] ? "bg-blue-700 text-white" : "ses-muted hover:bg-[var(--hover)]"}`}
            onClick={() => set({ [key]: !o[key] })}
          >
            {label}
          </button>
        ))}
        <button
          ref={filter.button}
          type="button"
          aria-haspopup="true"
          aria-expanded={filter.open}
          class="ses-strong ml-0.5 whitespace-nowrap border-l border-[var(--line)] py-0.5 pl-2 pr-1 text-xs"
          onClick={() => filter.setOpen(!filter.open)}
        >
          Filter{changed ? ` · ${changed}` : ""} ▾
        </button>
      </div>
      {filter.open && (
        <div ref={filter.pop} class="ses-pop right-0 top-10 w-48" role="group" aria-label="Search in">
          {filters.map(([key, label]) => (
            <label class="ses-item cursor-pointer !justify-start gap-2">
              <input type="checkbox" checked={o[key] === true} onChange={() => set({ [key]: !o[key] })} />
              {label}
            </label>
          ))}
          <div class="ses-sep" />
          <div class="flex justify-end px-2 py-1">
            <button
              type="button"
              class="text-xs text-blue-700 hover:underline dark:text-blue-300"
              onClick={() => set({ questions: true, answers: true, names: false })}
            >
              Reset
            </button>
          </div>
        </div>
      )}
      {props.invalid && <p class="mt-1 text-xs text-red-600 dark:text-red-400">Invalid regular expression</p>}
    </div>
  );
}
