import type { ComponentChildren } from "preact";
import { AgentIcon } from "../agents";
import type { Agent } from "../agent";
import { SessionMark } from "./Mark";
import { type MenuItem, MoreMenu } from "./Menu";
import type { Mark, Rail } from "./status";

/** The first line of a row or a result group: mark, title, Active chip, agent, ⋯. */
export function TitleLine(props: {
  mark: Mark;
  title: ComponentChildren;
  fullTitle: string;
  active: boolean;
  agent: Agent;
  meta: string;
}) {
  return (
    <div class="flex min-w-0 items-baseline gap-2 leading-5">
      <SessionMark mark={props.mark} />
      <span class="ses-strong min-w-0 truncate font-semibold" title={props.fullTitle}>
        {props.title}
      </span>
      {props.active && <span class="ses-chip">Active</span>}
      <span class="ses-muted ml-auto flex shrink-0 items-center gap-1.5 self-center whitespace-nowrap text-xs">
        <AgentIcon agent={props.agent} class="h-3.5 w-3.5 shrink-0" />
        {props.meta}
      </span>
    </div>
  );
}

export function SessionRow(props: {
  id: string;
  title: string;
  active: boolean;
  selected: boolean;
  mark: Mark;
  agent: Agent;
  meta: string;
  line: { rail: Rail; text: string };
  menu: MenuItem[];
  onOpen: () => void;
}) {
  return (
    <li class={`ses-row relative rounded-md ${props.selected ? "selected bg-[var(--selected)]" : "hover:bg-[var(--hover)]"}`}>
      <button
        type="button"
        data-row={props.id}
        aria-current={props.selected ? "true" : undefined}
        class="block w-full rounded-md py-[7px] pl-[19px] pr-[34px] text-left focus-visible:outline-2 focus-visible:outline-blue-500"
        onClick={props.onOpen}
      >
        <TitleLine
          mark={props.mark}
          title={props.title}
          fullTitle={props.title}
          active={props.active}
          agent={props.agent}
          meta={props.meta}
        />
        {/* The rail sits outside the text box, which clips to cut long lines. */}
        <div class={`ses-railed rail-${props.line.rail} ses-muted mt-0.5 text-[13px]`}>
          <div class="truncate">{props.line.text}</div>
        </div>
      </button>
      <MoreMenu label={`Actions for ${props.title}`} items={props.menu} class="!absolute right-1.5 top-[7px]" />
    </li>
  );
}

/** ↑ and ↓ move between rows; Enter opens the focused one. */
export function rowKeys(e: KeyboardEvent) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("[data-row]")];
  const i = rows.indexOf(document.activeElement as HTMLElement);
  if (i < 0) return;
  e.preventDefault();
  rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))].focus();
}

export function Skeleton() {
  return (
    <div aria-busy="true" aria-label="Loading sessions">
      {[
        [62, 48],
        [55, 70],
        [68, 40],
        [50, 58],
      ].map(([a, b]) => (
        <div class="py-2 pl-[19px]">
          <div class="ses-skel" style={{ width: `${a}%` }} />
          <div class="ses-skel" style={{ width: `${b}%`, height: "10px" }} />
        </div>
      ))}
    </div>
  );
}

/** A failed action floats over the bottom of the list, so no row moves. */
export function ErrorToast(props: { text: string; onClose: () => void }) {
  return (
    <div
      role="alert"
      class="absolute inset-x-4 bottom-4 flex items-center gap-2 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-[13px] text-red-800 shadow-lg dark:border-red-900 dark:bg-[#2a1215] dark:text-red-200"
    >
      <span class="min-w-0 flex-1">{props.text}</span>
      <button type="button" aria-label="Dismiss" class="shrink-0 rounded px-1 hover:bg-red-100 dark:hover:bg-red-950" onClick={props.onClose}>
        ✕
      </button>
    </div>
  );
}
