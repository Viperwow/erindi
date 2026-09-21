import { test } from "node:test";
import assert from "node:assert/strict";
import { accelerator } from "./hotkey.ts";

const key = (code: string, mods: Partial<Record<"ctrl" | "alt" | "shift" | "meta", boolean>> = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
});

test("modifiers come first in a fixed order", () => {
  assert.equal(accelerator(key("Space", { shift: true, alt: true, ctrl: true })), "Ctrl+Alt+Shift+Space");
  assert.equal(accelerator(key("KeyN", { meta: true, ctrl: true })), "Ctrl+Super+N");
});

test("physical keys ignore the keyboard layout", () => {
  assert.equal(accelerator(key("KeyN", { ctrl: true, alt: true })), "Ctrl+Alt+N");
  assert.equal(accelerator(key("Digit5", { ctrl: true })), "Ctrl+5");
  assert.equal(accelerator(key("Backquote", { alt: true })), "Alt+Backquote");
});

test("a lone modifier press is not a hotkey yet", () => {
  assert.equal(accelerator(key("ControlLeft", { ctrl: true })), null);
  assert.equal(accelerator(key("ShiftRight", { shift: true, ctrl: true })), null);
});

test("plain keys need a modifier, function keys do not", () => {
  assert.equal(accelerator(key("KeyA")), null);
  assert.equal(accelerator(key("F9")), "F9");
  assert.equal(accelerator(key("F13", { shift: true })), "Shift+F13");
});
