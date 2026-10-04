import { memo } from "preact/compat";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { AgentIcon } from "../agents";
import { drawDiagrams } from "../diagram";
import { markdown } from "../markdown";
import { failedOf, modelOf, modelShort, type Prompt, rawOf, replyOf, textOf } from "../model";
import { copyText } from "./clipboard";
import { answerMarkdown, qaMarkdown } from "./copy";
import type { Entry } from "./data";
import { type MenuItem, MoreMenu } from "./Menu";
import { ScrollMap, type Tick } from "./ScrollMap";
import type { Rail } from "./status";

export type Live = { rail: Rail; text: string };

const liveLabel: Partial<Record<Rail, string>> = {
  speak: "listening",
  decode: "transcribing",
  queue: "queued",
  gone: "cancelled",
};

const globalOf = (re: RegExp) => new RegExp(re.source, re.flags.includes("g") ? re.flags : `${re.flags}g`);

/** Text with every match of `re` marked. */
function Highlight(props: { text: string; re: RegExp | null }) {
  if (!props.re) return <>{props.text}</>;
  const out: (string | preact.JSX.Element)[] = [];
  let last = 0;
  for (const m of props.text.matchAll(globalOf(props.re))) {
    if (!m[0]) continue;
    out.push(props.text.slice(last, m.index), <mark>{m[0]}</mark>);
    last = m.index + m[0].length;
  }
  out.push(props.text.slice(last));
  return <>{out}</>;
}

/** Marks matches inside rendered Markdown, which Preact sets as HTML and does not track. */
function markMatches(root: HTMLElement, re: RegExp) {
  const global = globalOf(re);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    if (node.parentElement?.closest("mark, svg, .ses-copy")) continue;
    const text = node.data;
    const parts: (string | HTMLElement)[] = [];
    let last = 0;
    for (const m of text.matchAll(global)) {
      if (!m[0]) continue;
      parts.push(text.slice(last, m.index));
      const mark = document.createElement("mark");
      mark.textContent = m[0];
      parts.push(mark);
      last = m.index + m[0].length;
    }
    if (!last) continue;
    parts.push(text.slice(last));
    node.replaceWith(...parts);
  }
}

// Scrolling and every live event re-render the pane, so an unchanged answer must not parse again.
const Body = memo(function Body(props: { text: string; preview: boolean; caret?: boolean; re?: RegExp | null }) {
  const html = useMemo(() => (props.preview ? markdown(props.text, props.caret) : ""), [props.text, props.preview, props.caret]);
  if (!props.preview) {
    return (
      <div class="whitespace-pre-wrap font-mono text-xs leading-relaxed">
        <Highlight text={props.text} re={props.re ?? null} />
        {props.caret && <span class="caret" aria-hidden="true" />}
      </div>
    );
  }
  return (
    <div
      // Search marks are added to this HTML by hand, so a new search remounts it clean.
      key={props.re?.source ?? ""}
      class="markdown"
      // A diagram still streaming does not parse yet, so it is drawn once the reply ends.
      ref={(el) => {
        if (el && !props.caret && !el.dataset.drawn) {
          el.dataset.drawn = "1";
          void drawDiagrams(el);
        }
      }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
});

function CopyButtons(props: { prompt: Prompt }) {
  const [copied, setCopied] = useState<{ what: "a" | "qa"; ok: boolean } | null>(null);
  const copy = async (what: "a" | "qa") => {
    const ok = await copyText(what === "a" ? answerMarkdown(props.prompt) : qaMarkdown(props.prompt));
    const shown = { what, ok };
    setCopied(shown);
    setTimeout(() => setCopied((c) => (c === shown ? null : c)), 1500);
  };
  const label = (what: "a" | "qa", idle: string) =>
    copied?.what === what ? (copied.ok ? "✓ Copied" : "Not copied") : idle;
  const button = "rounded border border-[var(--line)] bg-[var(--bg)] px-2 py-0.5 text-xs hover:bg-[var(--hover)]";
  return (
    <div class="ses-copy absolute right-2 top-1.5 flex gap-1">
      {replyOf(props.prompt) !== null && (
        <button type="button" class={`${button} w-[78px]`} onClick={() => copy("a")}>
          {label("a", "⧉ Answer")}
        </button>
      )}
      <button type="button" class={`${button} w-[78px]`} onClick={() => copy("qa")}>
        {label("qa", "⧉ Q&A")}
      </button>
    </div>
  );
}

export function Conversation(props: {
  entry: Entry;
  title: string;
  agentLabel: string;
  details: string[];
  logUnread: boolean;
  notStarted: boolean;
  menu: MenuItem[];
  preview: boolean;
  /** The turns ‹ › steps through: the matches while searching, otherwise every question. */
  stops: number[];
  current: number | null;
  onCurrent: (turn: number) => void;
  pattern: RegExp | null;
  live: Live[];
  /** The running agent's current step, or the local model's reply so far, for the last turn. */
  running: { detail: string; streamed: string | null } | null;
  note: string | null;
  narrow: boolean;
  onBack: () => void;
}) {
  const { entry } = props;
  const scroller = useRef<HTMLDivElement>(null);
  const n = entry.prompts.length;
  const at = props.current === null ? -1 : props.stops.indexOf(props.current);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    if (props.current === null) {
      el.scrollTop = el.scrollHeight;
      return;
    }
    const turn = el.querySelector<HTMLElement>(`#turn-${props.current}`);
    turn?.scrollIntoView({ block: "center" });
    if (turn && props.pattern) turn.querySelectorAll<HTMLElement>(".markdown").forEach((m) => markMatches(m, props.pattern!));
  }, [entry.id, props.current, props.pattern, props.preview]);

  const dictating = props.live.map((l) => `${l.rail}:${l.text}`).join("\n");
  useEffect(() => {
    const el = scroller.current;
    if (el && dictating) el.scrollTop = el.scrollHeight;
  }, [dictating]);

  const step = (by: number) => {
    if (!props.stops.length) return;
    const i = at < 0 ? (by > 0 ? 0 : props.stops.length - 1) : Math.max(0, Math.min(props.stops.length - 1, at + by));
    props.onCurrent(props.stops[i]);
  };

  return (
    <>
      <header class="border-b border-[var(--line)] py-2.5 pl-[38px] pr-4">
        <div class="flex min-w-0 items-center gap-2.5">
          {props.narrow && (
            <button type="button" class="-ml-6 shrink-0 text-[13px] text-blue-700 hover:underline dark:text-blue-300" onClick={props.onBack}>
              ← Sessions
            </button>
          )}
          <h3 class="ses-strong min-w-0 flex-1 truncate font-semibold" title={props.title}>
            {props.title}
          </h3>
          {props.stops.length > 0 && (
            <span class="ses-muted flex shrink-0 items-center text-xs tabular-nums">
              <button type="button" aria-label="Previous" class="rounded px-1.5 hover:bg-[var(--hover)]" onClick={() => step(-1)}>
                ‹
              </button>
              {at < 0 ? "–" : at + 1} of {props.stops.length}
              <button type="button" aria-label="Next" class="rounded px-1.5 hover:bg-[var(--hover)]" onClick={() => step(1)}>
                ›
              </button>
            </span>
          )}
          <MoreMenu label={`Actions for ${props.title}`} items={props.menu} />
        </div>
        <p class="ses-muted mt-0.5 flex min-w-0 items-center gap-1.5 whitespace-nowrap text-xs">
          {props.details.map((d, i) => (
            <>
              {i > 0 && <span aria-hidden="true">·</span>}
              <span class={i === 0 ? "min-w-0 truncate" : "shrink-0"} title={i === 0 ? d : undefined}>
                {d}
              </span>
            </>
          ))}
          {props.logUnread && (
            <span
              role="img"
              aria-label="Couldn't read the agent's log. Showing the values the session started with."
              title="Couldn't read the agent's log. Showing the values the session started with."
              class="inline-flex h-3.5 w-3.5 shrink-0 cursor-help items-center justify-center rounded-full border border-current text-[9px]"
            >
              i
            </span>
          )}
        </p>
      </header>
      {props.notStarted && (
        <p class="mx-4 ml-[38px] mt-2.5 rounded-md border border-[var(--line)] bg-[var(--hover)] px-2.5 py-2 text-[13px]">
          This session didn't start, so it can't be continued.
        </p>
      )}
      <div
        ref={scroller}
        tabIndex={-1}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            step(e.key === "ArrowDown" ? 1 : -1);
          }
        }}
        class="ses-scroll min-h-0 flex-1 overflow-y-auto py-3 pl-4 pr-6 text-sm outline-none"
      >
        <div>
        {entry.prompts.map((p, i) => {
          const turn = i + 1;
          const reply = replyOf(p);
          const raw = rawOf(p);
          const model = modelOf(p);
          const last = turn === n;
          const running = last && reply === null ? props.running : null;
          const re = props.current === turn ? props.pattern : null;
          return (
            <div id={`turn-${turn}`} data-turn key={`${entry.id}-${turn}`} class={`ses-turn ${props.current === turn ? "current" : ""}`}>
              <CopyButtons prompt={p} />
              <div class="ses-railed rail-speak ml-3">
                <div class="ses-muted text-xs">You · {turn}</div>
                <div class="ses-strong pr-40">
                  <Highlight text={textOf(p)} re={re} />
                </div>
                {raw && <div class="ses-muted text-xs">Said: {raw}</div>}
              </div>
              {reply !== null || running ? (
                <div
                  class={`ses-railed ml-3 mt-1.5 ${running ? "rail-run" : failedOf(p) ? "rail-err" : "rail-ok"}`}
                  aria-busy={running ? true : undefined}
                >
                  <div class="ses-muted flex items-center gap-1.5 text-xs">
                    <AgentIcon agent={entry.agent} class="h-3.5 w-3.5 shrink-0" />
                    {props.agentLabel}
                    {model && <span>· {modelShort(model, props.agentLabel)}</span>}
                    {running && !running.streamed && <span>· working</span>}
                  </div>
                  {running ? (
                    running.streamed !== null ? (
                      <Body text={running.streamed} preview={props.preview} caret />
                    ) : (
                      <div class="ses-muted text-[13px]">{running.detail}</div>
                    )
                  ) : failedOf(p) ? (
                    <div class="text-red-600 dark:text-red-400">{reply}</div>
                  ) : (
                    <Body text={reply ?? ""} preview={props.preview} re={re} />
                  )}
                </div>
              ) : (
                <div class="ses-muted ml-3 mt-1 text-xs italic">No answer saved for this turn.</div>
              )}
            </div>
          );
        })}
        {props.live.map((l, i) => (
          <div class="ses-turn" key={`live-${i}`}>
            <div class={`ses-railed rail-${l.rail} ml-3`}>
              <div class="ses-muted text-xs">{l.rail === "waiting" ? "Waiting" : `You · ${n + i + 1} · ${liveLabel[l.rail] ?? ""}`}</div>
              {l.rail !== "waiting" && <div class={`ses-muted ${l.rail === "gone" ? "line-through" : ""}`}>{l.text}</div>}
            </div>
          </div>
        ))}
        </div>
      </div>
      <ScrollMap
        scroller={scroller}
        ticks={entry.prompts.flatMap((p, i): Tick[] => {
          const turn = i + 1;
          const kind =
            props.current === turn ? "current" : failedOf(p) ? "failed" : props.pattern && props.stops.includes(turn) ? "match" : null;
          return kind ? [{ turn, kind, label: `${turn} · ${textOf(p)}` }] : [];
        })}
        onJump={props.onCurrent}
      />
      {props.note && (
        <div role="status" class="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-md border border-[var(--line)] bg-[var(--pop)] px-3 py-1.5 text-[13px] text-green-700 shadow-lg dark:text-green-300">
          {props.note}
        </div>
      )}
    </>
  );
}
