import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAgents } from "./agents";
import type { View } from "./bubble";
import { type Settings, agentName } from "./controls";
import { failedOf, textOf } from "./model";
import { Conversation } from "./sessions/Conversation";
import { copyText } from "./sessions/clipboard";
import { sessionMarkdown } from "./sessions/copy";
import { countOf, type Entry, type Sessions, titleOf } from "./sessions/data";
import { ErrorToast, rowKeys, SessionRow, Skeleton } from "./sessions/List";
import type { MenuItem } from "./sessions/Menu";
import { type GroupInfo, Results } from "./sessions/Results";
import { defaults, SearchBox } from "./sessions/SearchBox";
import { type Found, type Hit, type Options, pattern, search } from "./sessions/search";
import { liveOf, markOf } from "./sessions/status";
import { ago } from "./time";

/** How long Delete waits for the confirming second click. */
const CONFIRM_MS = 3000;
/** Search runs this long after the last keystroke. */
const DEBOUNCE_MS = 200;
/** Below this window width one pane shows at a time. */
const NARROW = 900;

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

export function SessionsView() {
  const [data, setData] = useState<Sessions | null>(null);
  const [view, setView] = useState<View | null>(null);
  // The local model's reply while it streams; the saved one replaces it when the run ends.
  const [streamed, setStreamed] = useState<{ id: string; text: string } | null>(null);
  const [apiName, setApiName] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [current, setCurrent] = useState<{ turn: number; kind: Hit["kind"] | null } | null>(null);
  const [options, setOptions] = useState<Options>(defaults);
  const [settled, setSettled] = useState<Options>(defaults);
  const [preview, setPreview] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [narrow, setNarrow] = useState(window.innerWidth < NARROW);
  const { agents } = useAgents();
  const load = () => invoke<Sessions>("list_sessions").then(setData);

  useEffect(() => {
    load();
    invoke<Settings>("get_settings").then((s) => setApiName(s.apiName));
    const offs = [
      listen("sessions-changed", () => {
        setStreamed(null);
        load();
      }),
      listen<{ id: string; text: string }>("session-reply", (e) => setStreamed(e.payload)),
      listen<View>("view", (e) => setView(e.payload)),
    ];
    const resize = () => setNarrow(window.innerWidth < NARROW);
    window.addEventListener("focus", load);
    window.addEventListener("resize", resize);
    return () => {
      offs.forEach((off) => off.then((f) => f()));
      window.removeEventListener("focus", load);
      window.removeEventListener("resize", resize);
    };
  }, []);

  useEffect(() => {
    const t = setTimeout(() => setSettled(options), DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [options]);

  const entries = data?.entries ?? [];
  const searching = settled.query !== "";
  // An invalid expression keeps the previous results on screen.
  const lastGood = useRef<Found>({ groups: [], total: 0 });
  const found = useMemo(() => {
    const f = search(entries, settled);
    if (!("error" in f && f.error === "regex")) lastGood.current = f;
    return f;
  }, [entries, settled]);
  const invalid = "error" in found && found.error === "regex";
  const shown = invalid ? lastGood.current : found;
  const re = useMemo(() => (searching ? pattern(settled) : null), [settled, searching]);

  const labelOf = (e: Entry) => {
    const status = agents.find((a) => a.agent === e.agent);
    return e.agent === "api" ? agentName("api", { apiName }) : (status?.label ?? agentName(e.agent, { apiName }));
  };

  const act = (command: string, id: string) =>
    invoke(command, { id }).then(
      () => {
        setError(null);
        return load();
      },
      (e) => setError(String(e)),
    );

  const remove = (id: string): "keep" | undefined => {
    if (confirming !== id) {
      setConfirming(id);
      setTimeout(() => setConfirming((c) => (c === id ? null : c)), CONFIRM_MS);
      return "keep";
    }
    setConfirming(null);
    if (selected === id) setSelected(null);
    void act("delete_session", id);
  };

  const flash = (text: string) => {
    setNote(text);
    setTimeout(() => setNote((n) => (n === text ? null : n)), 1500);
  };

  const menuOf = (e: Entry, inPane: boolean): MenuItem[] => {
    const active = e.id === data?.active;
    const resumable = e.nativeId !== null;
    const items: MenuItem[] = [
      {
        label: "Make active",
        hint: active ? "Active" : undefined,
        disabled: active || !resumable,
        onSelect: () => void act("continue_session", e.id),
      },
    ];
    if (e.agent !== "api") {
      items.push({ label: "Open in terminal", disabled: !resumable, onSelect: () => void act("open_history_session", e.id) });
    }
    if (inPane) {
      items.push({
        label: "Copy session",
        hint: "Markdown",
        onSelect: () => {
          void copyText(sessionMarkdown(titleOf(e), e.prompts)).then((ok) =>
            flash(ok ? "✓ Copied the session as Markdown" : "The session was not copied"),
          );
        },
      });
      items.push("separator", { switch: "Preview", on: preview, onToggle: () => setPreview(!preview) });
    }
    const confirm = confirming === e.id;
    items.push("separator", {
      label: confirm ? "Confirm delete" : "Delete",
      hint: confirm ? "3 s" : undefined,
      danger: true,
      confirm,
      onSelect: () => remove(e.id),
    });
    return items;
  };

  const lastText = (e: Entry) => {
    const last = e.prompts[e.prompts.length - 1];
    return last ? textOf(last) : undefined;
  };

  const lineOf = (e: Entry) => {
    const live = liveOf(view, e.id, lastText(e));
    if (live.length) return live[live.length - 1];
    const last = e.prompts[e.prompts.length - 1];
    return { rail: last && failedOf(last) ? ("err" as const) : ("speak" as const), text: last ? textOf(last) : "" };
  };

  const open = (id: string, turn: number | null, kind: Hit["kind"] | null) => {
    const e = entries.find((x) => x.id === id);
    setSelected(id);
    setCurrent({ turn: turn ?? e?.prompts.length ?? 1, kind });
  };

  const entry = entries.find((e) => e.id === selected) ?? null;
  const stops = useMemo(() => {
    if (!entry) return [];
    const group = searching && "groups" in shown ? shown.groups.find((g) => g.id === entry.id) : undefined;
    if (group?.hits.length) return [...new Set(group.hits.map((h) => h.turn))];
    return entry.prompts.map((_, i) => i + 1);
  }, [entry, shown, searching]);

  if (data && entries.length === 0) {
    return (
      <div class="ses items-center justify-center text-center">
        <div>
          <p class="ses-strong font-semibold">No sessions yet</p>
          <p class="ses-muted mt-1">Ask an agent something by voice. Its sessions appear here.</p>
        </div>
      </div>
    );
  }

  const info = (id: string): GroupInfo => {
    const e = entries.find((x) => x.id === id)!;
    return {
      mark: markOf(view, id),
      active: id === data?.active,
      agent: e.agent,
      meta: `${labelOf(e)} · ${countOf(e)}`,
      menu: menuOf(e, false),
      label: `Actions for ${titleOf(e)}`,
    };
  };

  const toggleOn = settled.matchCase || settled.word || settled.regex;
  const summary = !data
    ? " "
    : !searching
      ? count(entries.length, "session", "sessions")
      : "error" in shown
        ? " "
        : `${count(shown.total, "match", "matches")}${shown.total ? ` in ${count(shown.groups.length, "session", "sessions")}` : ""}`;

  const list = (
    <aside class="ses-list" aria-label="Sessions">
      <div class="space-y-2 p-4 pb-2">
        <h2 class="ses-strong text-base font-semibold">Sessions</h2>
        <SearchBox options={options} invalid={invalid} onChange={setOptions} />
        <p class="ses-muted text-xs" aria-live="polite">
          {summary}
        </p>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-16" onKeyDown={rowKeys}>
        {!data ? (
          <Skeleton />
        ) : "error" in found && found.error === "nothing" ? (
          <Empty title="Nothing to search in" text="Pick Questions, Answers or Session names in Filter." />
        ) : searching && "groups" in shown ? (
          shown.groups.length ? (
            <Results
              groups={shown.groups}
              info={info}
              pattern={re}
              current={selected && current?.kind ? { id: selected, turn: current.turn, kind: current.kind } : null}
              onOpen={open}
            />
          ) : (
            <Empty title="No matches" text={toggleOn ? "Turn off Aa, ab or .* to widen the search." : ""} />
          )
        ) : (
          <ul>
            {entries.map((e) => (
              <SessionRow
                key={e.id}
                id={e.id}
                title={titleOf(e)}
                active={e.id === data.active}
                selected={e.id === selected}
                mark={markOf(view, e.id)}
                agent={e.agent}
                meta={`${labelOf(e)} · ${countOf(e)}`}
                line={lineOf(e)}
                menu={menuOf(e, false)}
                onOpen={() => open(e.id, null, null)}
              />
            ))}
          </ul>
        )}
      </div>
      {error && <ErrorToast text={error} onClose={() => setError(null)} />}
    </aside>
  );

  const details = entry && data?.details[entry.id];
  const pane = (
    <section class="ses-pane" aria-label="Conversation">
      {entry ? (
        <Conversation
          entry={entry}
          title={titleOf(entry)}
          agentLabel={labelOf(entry)}
          details={[
            entry.cwd,
            ...(entry.agent === "api" ? [] : [details?.permission ?? entry.startedPermission ?? "default"]),
            ago(entry.updatedMs),
            entry.id.slice(0, 8),
          ]}
          logUnread={!details && entry.agent !== "api" && entry.agent !== "cursor" && entry.nativeId !== null}
          notStarted={entry.nativeId === null}
          menu={menuOf(entry, true)}
          preview={preview}
          stops={stops}
          current={current?.turn ?? null}
          onCurrent={(turn) => setCurrent({ turn, kind: null })}
          pattern={re}
          live={liveOf(view, entry.id, lastText(entry))}
          running={
            markOf(view, entry.id) === "run"
              ? { detail: view?.detail ?? "", streamed: streamed?.id === entry.id ? streamed.text : null }
              : null
          }
          note={note}
          narrow={narrow}
          onBack={() => setSelected(null)}
        />
      ) : (
        <Empty
          title="No session selected"
          text={searching ? "Choose a session or a match on the left." : "Choose a session on the left."}
          center
        />
      )}
    </section>
  );

  return (
    <div
      class={`ses ${narrow ? "ses-narrow" : ""}`}
      onKeyDown={(e) => {
        if (e.key === "Escape" && selected) setSelected(null);
      }}
    >
      {(!narrow || !entry) && list}
      {(!narrow || entry) && pane}
    </div>
  );
}

function Empty(props: { title: string; text: string; center?: boolean }) {
  return (
    <div class={`px-6 py-7 text-center text-[13px] ${props.center ? "m-auto" : ""}`}>
      <p class="ses-strong font-semibold">{props.title}</p>
      {props.text && <p class="ses-muted mt-1">{props.text}</p>}
    </div>
  );
}
