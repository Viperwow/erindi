import { test } from "node:test";
import assert from "node:assert/strict";
import { type Options, search } from "./search.ts";

const base: Options = { query: "", matchCase: false, word: false, regex: false, questions: true, answers: true, names: false };
const one = (prompts: unknown[]) => [{ id: "s", prompts: prompts as never }];
const hits = (found: ReturnType<typeof search>) => ("groups" in found ? found.groups.flatMap((g) => g.hits) : []);

test("substring search escapes regex characters", () => {
  assert.equal(hits(search(one(["a.b"]), { ...base, query: "a.b" })).length, 1);
  assert.equal(hits(search(one(["axb"]), { ...base, query: "a.b" })).length, 0);
});

test("regex mode reads patterns", () => {
  const found = search(one(["rent", "rental", "parent"]), { ...base, query: "^re?nt(al)?$", regex: true });
  assert.deepEqual(
    hits(found).map((h) => h.match),
    ["rent", "rental"],
  );
});

test("case and word toggles", () => {
  assert.equal(hits(search(one(["rent"]), { ...base, query: "Rent", matchCase: true })).length, 0);
  assert.equal(hits(search(one(["parent"]), { ...base, query: "rent", word: true })).length, 0);
  assert.equal(hits(search(one(["the rent"]), { ...base, query: "rent", word: true })).length, 1);
});

test("an invalid regex is reported", () => {
  assert.deepEqual(search(one(["a"]), { ...base, query: "(", regex: true }), { error: "regex" });
});

test("no filter is reported", () => {
  assert.deepEqual(search(one(["a"]), { ...base, query: "a", questions: false, answers: false, names: false }), {
    error: "nothing",
  });
});

test("answers and questions are told apart", () => {
  const found = search(one([{ text: "rent?", reply: "rent is high" }]), { ...base, query: "rent" });
  assert.deepEqual(
    hits(found).map((h) => [h.turn, h.kind]),
    [
      [1, "q"],
      [1, "a"],
    ],
  );
  assert.equal("total" in found && found.total, 2);
});

test("names only marks the title", () => {
  const found = search(one(["Plan a move to Paris", "rent"]), { ...base, query: "paris", questions: false, answers: false, names: true });
  assert.ok("groups" in found);
  assert.deepEqual(found.groups.map((g) => [g.title, g.titleHit, g.hits.length]), [["Plan a move to Paris", true, 0]]);
});

test("an empty query finds nothing", () => {
  assert.deepEqual(search(one(["a"]), { ...base, query: "" }), { groups: [], total: 0 });
});

test("a snippet keeps the text around the match", () => {
  const long = `${"word ".repeat(30)}rent${" more".repeat(40)}`;
  const [hit] = hits(search(one([long]), { ...base, query: "rent" }));
  assert.equal(hit.match, "rent");
  assert.ok(hit.before.startsWith("…") && hit.before.length <= 42);
  assert.ok(hit.after.endsWith("…") && hit.after.length <= 82);
});

test("answer snippets read as plain text", () => {
  const found = search(one([{ text: "q", reply: "### Rent caps\n- **Centre**: `€36`\n| a | b |" }]), { ...base, query: "centre" });
  const [hit] = hits(found);
  assert.equal(`${hit.before}${hit.match}${hit.after}`, "Rent caps Centre: €36 a b");
});
