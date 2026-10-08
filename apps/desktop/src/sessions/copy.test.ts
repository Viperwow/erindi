import { test } from "node:test";
import assert from "node:assert/strict";
import { answerMarkdown, qaMarkdown, sessionMarkdown } from "./copy.ts";

test("a question and its answer copy as Markdown", () => {
  assert.equal(qaMarkdown({ text: "Q", reply: "A" }), "**You:** Q\n\nA\n");
  assert.equal(qaMarkdown("Q"), "**You:** Q\n");
  assert.equal(answerMarkdown({ text: "Q", reply: "A" }), "A\n");
});

test("a session copies with its title and every turn", () => {
  assert.equal(
    sessionMarkdown("T", [{ text: "Q1", reply: "A1" }, "Q2"]),
    "# T\n\n**You:** Q1\n\nA1\n\n---\n\n**You:** Q2\n",
  );
});
