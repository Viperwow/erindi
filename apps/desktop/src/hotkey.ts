export type KeyLike = {
  code: string;
  key: string;
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
  const code = physicalCode(e);
  if (!code || MODIFIER_CODES.test(code)) return null;
  const mods = [
    e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
    e.metaKey && "Super",
  ].filter(Boolean);
  const isFunctionKey = /^F\d{1,2}$/.test(code);
  if (mods.length === 0 && !isFunctionKey) return null;
  const key = code.replace(/^Key([A-Z])$/, "$1").replace(/^Digit(\d)$/, "$1");
  return [...mods, key].join("+");
}

/** Some devices and synthetic events report no code; rebuild one from the key value. */
function physicalCode(e: KeyLike): string | null {
  if (e.code && e.code !== "Unidentified") return e.code;
  if (/^[a-z]$/i.test(e.key)) return `Key${e.key.toUpperCase()}`;
  if (/^\d$/.test(e.key)) return `Digit${e.key}`;
  if (e.key === " ") return "Space";
  return null;
}
