// Writes one version into every file that carries it: `node scripts/set-version.mjs 1.2.3`.
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error(`not a version: ${version}`);

function edit(path, pattern, replacement) {
  const text = readFileSync(path, "utf8");
  if (!pattern.test(text)) throw new Error(`no version found in ${path}`);
  pattern.lastIndex = 0;
  writeFileSync(path, text.replace(pattern, replacement));
}

edit("Cargo.toml", /(\[workspace\.package\][^[]*?\nversion = )"[^"]*"/, `$1"${version}"`);
edit("Cargo.lock", /(name = "erindi-[^"]+"\r?\nversion = )"[^"]*"/g, `$1"${version}"`);
edit("apps/desktop/package.json", /("version": )"[^"]*"/, `$1"${version}"`);
edit("apps/desktop/src-tauri/tauri.conf.json", /("version": )"[^"]*"/, `$1"${version}"`);
