import { invoke, type InvokeArgs } from "@tauri-apps/api/core";

const pending = new Map<string, Promise<unknown>>();

/** A read-only command; identical calls in flight share the first one's answer. */
export function query<T>(command: string, args?: InvokeArgs): Promise<T> {
  const key = `${command} ${JSON.stringify(args ?? null)}`;
  let p = pending.get(key) as Promise<T> | undefined;
  if (!p) {
    p = invoke<T>(command, args).finally(() => pending.delete(key));
    pending.set(key, p);
  }
  return p;
}
