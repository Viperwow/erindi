import { test } from "node:test";
import assert from "node:assert/strict";
import { accelerator, heldModifiers } from "./hotkey.ts";

const key = (
  code: string,
  mods: Partial<Record<"ctrl" | "alt" | "shift" | "meta", boolean>> = {},
  keyValue = "",
) => ({
  code,
  key: keyValue,
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

test("falls back to the key value when the device reports no code", () => {
  assert.equal(accelerator(key("", { ctrl: true, shift: true }, "k")), "Ctrl+Shift+K");
  assert.equal(accelerator(key("Unidentified", { alt: true }, "5")), "Alt+5");
  assert.equal(accelerator(key("", { ctrl: true }, "")), null);
  assert.equal(accelerator(key("", { ctrl: true }, "Control")), null);
});

test("held modifiers preview the combination being built", () => {
  assert.equal(heldModifiers(key("ControlLeft", { ctrl: true })), "Ctrl");
  assert.equal(heldModifiers(key("AltLeft", { ctrl: true, alt: true })), "Ctrl+Alt");
  assert.equal(heldModifiers(key("MetaLeft", { meta: true, shift: true })), "Shift+Super");
  assert.equal(heldModifiers(key("KeyA")), "");
});
