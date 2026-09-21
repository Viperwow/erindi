export type KeyLike = {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
};

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/;

/**
 * Turns a key press into an accelerator string for the global shortcut parser.
 * Uses the physical key code so the same keys work in every keyboard layout.
 * Returns null until a non-modifier key is pressed, or for a plain key without modifiers.
 */
export function accelerator(e: KeyLike): string | null {
  if (MODIFIER_CODES.test(e.code)) return null;
  const mods = [
    e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
    e.metaKey && "Super",
  ].filter(Boolean);
  const isFunctionKey = /^F\d{1,2}$/.test(e.code);
  if (mods.length === 0 && !isFunctionKey) return null;
  const key = e.code.replace(/^Key([A-Z])$/, "$1").replace(/^Digit(\d)$/, "$1");
  return [...mods, key].join("+");
}
