import { writeText } from "@tauri-apps/plugin-clipboard-manager";

/** Copies `text`; false when the system clipboard refused it. */
export const copyText = (text: string) =>
  writeText(text).then(
    () => true,
    () => false,
  );
