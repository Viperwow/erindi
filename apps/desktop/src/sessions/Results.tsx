import type { ComponentChildren } from "preact";
import type { Agent } from "../agent";
import { TitleLine } from "./List";
import { type MenuItem, MoreMenu } from "./Menu";
import type { Group, Hit } from "./search";
import type { Mark } from "./status";

export type GroupInfo = { mark: Mark; active: boolean; agent: Agent; meta: string; menu: MenuItem[]; label: string };

/** The title with the matched part highlighted, when the match is in the session's name. */
function highlight(title: string, re: RegExp | null): ComponentChildren {
  const m = re?.exec(title);
  if (!m || !m[0]) return title;
  return (
    <>
      {title.slice(0, m.index)}
      <mark>{m[0]}</mark>
      {title.slice(m.index + m[0].length)}
    </>
  );
}

export function Results(props: {
  groups: Group[];
  info: (id: string) => GroupInfo;
  pattern: RegExp | null;
  current: { id: string; turn: number; kind: Hit["kind"] } | null;
  onOpen: (id: string, turn: number | null, kind: Hit["kind"] | null) => void;
}) {
  return (
    <div class="space-y-3">
      {props.groups.map((g) => {
        const info = props.info(g.id);
        const title = g.titleHit ? highlight(g.title, props.pattern) : g.title;
        return (
          <section class="ses-row" aria-label={g.title}>
            {g.hits.length ? (
              <div class="relative py-0.5 pl-[19px] pr-[34px]">
                <TitleLine mark={info.mark} title={title} fullTitle={g.title} active={info.active} agent={info.agent} meta={info.meta} />
                <MoreMenu label={info.label} items={info.menu} class="!absolute right-1.5 top-0.5" />
              </div>
            ) : (
              <div class="relative">
                <button
                  type="button"
                  data-row={g.id}
                  class="block w-full rounded-md py-[7px] pl-[19px] pr-[34px] text-left hover:bg-[var(--hover)]"
                  onClick={() => props.onOpen(g.id, null, null)}
                >
                  <TitleLine mark={info.mark} title={title} fullTitle={g.title} active={info.active} agent={info.agent} meta={info.meta} />
                </button>
                <MoreMenu label={info.label} items={info.menu} class="!absolute right-1.5 top-[7px]" />
              </div>
            )}
            {g.hits.map((h) => {
              const on = props.current?.id === g.id && props.current.turn === h.turn && props.current.kind === h.kind;
              return (
                <button
                  type="button"
                  data-row={`${g.id}:${h.turn}:${h.kind}`}
                  aria-current={on ? "true" : undefined}
                  class={`grid w-full grid-cols-[68px_minmax(0,1fr)] items-baseline gap-x-1 rounded-md py-1 pl-[19px] pr-2 text-left text-[13px] ${
                    on ? "ses-strong bg-[var(--hit)]" : "hover:bg-[var(--hover)]"
                  }`}
                  onClick={() => props.onOpen(g.id, h.turn, h.kind)}
                >
                  <span class={`ses-railed ses-muted text-xs ${h.kind === "q" ? "rail-speak" : "rail-ok"}`}>
                    {h.kind === "q" ? "You" : "Answer"} · {h.turn}
                  </span>
                  <span class="truncate">
                    {h.before}
                    <mark>{h.match}</mark>
                    {h.after}
                  </span>
                </button>
              );
            })}
          </section>
        );
      })}
    </div>
  );
}
