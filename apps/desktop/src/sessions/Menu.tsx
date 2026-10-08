import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";

/** An action; `onSelect` returns "keep" to leave the menu open, as a delete waiting for its confirmation does. */
export type Action = {
  label: string;
  hint?: string;
  onSelect: () => void | "keep";
  disabled?: boolean;
  danger?: boolean;
  confirm?: boolean;
};
export type MenuItem = Action | "separator" | { switch: string; on: boolean; onToggle: () => void };

/** Closes a popover on a click outside it or on Esc, and gives focus back to its button. */
export function usePopover() {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const close = () => {
    setOpen(false);
    button.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    pop.current?.querySelector<HTMLElement>("button:not(:disabled), input")?.focus();
    const down = (e: MouseEvent) => {
      if (!pop.current?.contains(e.target as Node) && !button.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key, true);
    };
  }, [open]);
  return { open, setOpen, close, button, pop };
}

/** ↑ and ↓ move between a menu's enabled items. */
function arrows(e: KeyboardEvent) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("[role^=menuitem]:not(:disabled)")];
  const i = items.indexOf(document.activeElement as HTMLElement);
  items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
}

export const Dots = () => (
  <svg aria-hidden="true" width="14" height="14" viewBox="0 0 16 16">
    <circle cx="3" cy="8" r="1.5" fill="currentColor" />
    <circle cx="8" cy="8" r="1.5" fill="currentColor" />
    <circle cx="13" cy="8" r="1.5" fill="currentColor" />
  </svg>
);

export function MoreMenu(props: { label: string; items: MenuItem[]; class?: string }) {
  const p = usePopover();
  return (
    <span class={`relative ${props.class ?? ""}`}>
      <button
        ref={p.button}
        type="button"
        class="ses-more"
        aria-label={props.label}
        aria-haspopup="menu"
        aria-expanded={p.open}
        onClick={(e) => {
          e.stopPropagation();
          p.setOpen(!p.open);
        }}
      >
        <Dots />
      </button>
      {p.open && (
        <div ref={p.pop} role="menu" class="ses-pop right-0 top-6" onKeyDown={arrows} onClick={(e) => e.stopPropagation()}>
          {props.items.map((item) =>
            item === "separator" ? (
              <div role="separator" class="ses-sep" />
            ) : "switch" in item ? (
              <button type="button" role="menuitemcheckbox" aria-checked={item.on} class="ses-item" onClick={item.onToggle}>
                {item.switch}
                <span class={`ses-switch ${item.on ? "on" : ""}`} aria-hidden="true" />
              </button>
            ) : (
              <MenuAction item={item} close={p.close} />
            ),
          )}
        </div>
      )}
    </span>
  );
}

function MenuAction(props: { item: Action; close: () => void }) {
  const { item } = props;
  return (
    <button
      type="button"
      role="menuitem"
      disabled={item.disabled}
      class={`ses-item ${item.danger ? "danger" : ""} ${item.confirm ? "confirm" : ""}`}
      onClick={() => {
        if (item.onSelect() !== "keep") props.close();
      }}
    >
      {item.label}
      {item.hint && <Hint>{item.hint}</Hint>}
    </button>
  );
}

const Hint = (props: { children: ComponentChildren }) => <span class="text-xs opacity-70">{props.children}</span>;
