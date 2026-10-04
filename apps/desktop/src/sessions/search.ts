import { type Prompt, replyOf, textOf } from "../model.ts";

export type Options = {
  query: string;
  matchCase: boolean;
  word: boolean;
  regex: boolean;
  questions: boolean;
  answers: boolean;
  names: boolean;
};

/** One match: `turn` counts from 1, `kind` says whether it is in the question or the answer. */
export type Hit = { turn: number; kind: "q" | "a"; before: string; match: string; after: string };
export type Group = { id: string; title: string; titleHit: boolean; hits: Hit[] };
export type Found = { groups: Group[]; total: number } | { error: "regex" } | { error: "nothing" };

const BEFORE = 40;
const AFTER = 80;

/** The query as a pattern; `null` when the person's regular expression does not compile. */
export function pattern(o: Options): RegExp | null {
  let source = o.regex ? o.query : o.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // `\b` only knows Latin letters, so a whole word is bounded by any letter or digit.
  if (o.word) source = `(?<![\\p{L}\\p{N}_])(?:${source})(?![\\p{L}\\p{N}_])`;
  try {
    return new RegExp(source, o.matchCase ? "u" : "iu");
  } catch {
    return null;
  }
}

function snippet(text: string, at: number, length: number): Omit<Hit, "turn" | "kind"> {
  let before = text.slice(0, at);
  if (before.length > BEFORE) before = `…${before.slice(-BEFORE).replace(/^\S*\s/, "")}`;
  let after = text.slice(at + length);
  if (after.length > AFTER) after = `${after.slice(0, AFTER).replace(/\s\S*$/, "")}…`;
  return { before, match: text.slice(at, at + length), after };
}

function find(re: RegExp, text: string): Omit<Hit, "turn" | "kind"> | null {
  const m = re.exec(text);
  // An empty match, such as `a*` on "b", finds nothing to show.
  return m && m[0] ? snippet(text, m.index, m[0].length) : null;
}

export function search(sessions: { id: string; prompts: Prompt[] }[], o: Options): Found {
  if (!o.questions && !o.answers && !o.names) return { error: "nothing" };
  if (!o.query) return { groups: [], total: 0 };
  const re = pattern(o);
  if (!re) return { error: "regex" };
  const groups: Group[] = [];
  let total = 0;
  for (const s of sessions) {
    const title = s.prompts.length ? textOf(s.prompts[0]) : "";
    const titleHit = o.names && find(re, title) !== null;
    const hits: Hit[] = [];
    s.prompts.forEach((p, i) => {
      const q = o.questions ? find(re, textOf(p)) : null;
      if (q) hits.push({ turn: i + 1, kind: "q", ...q });
      const reply = replyOf(p);
      const a = o.answers && reply ? find(re, reply) : null;
      if (a) hits.push({ turn: i + 1, kind: "a", ...a });
    });
    if (titleHit || hits.length) {
      groups.push({ id: s.id, title, titleHit, hits });
      total += hits.length + (titleHit && !hits.length ? 1 : 0);
    }
  }
  return { groups, total };
}
