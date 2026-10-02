import { test } from "node:test";
import assert from "node:assert/strict";
import { markdown } from "./markdown.ts";

test("a model's Markdown turns into HTML", () => {
  assert.equal(markdown("## Paris\n\n- **big**\n- `old`"), "<h2>Paris</h2>\n<ul>\n<li><strong>big</strong></li>\n<li><code>old</code></li>\n</ul>\n");
});

test("HTML in a reply shows as text, never runs", () => {
  assert.equal(markdown('<img src=x onerror="alert(1)">'), "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
  assert.equal(markdown("a <b>b</b>"), "<p>a &lt;b&gt;b&lt;/b&gt;</p>\n");
  assert.ok(!markdown("[x](javascript:alert(1))").includes("javascript:"));
});
