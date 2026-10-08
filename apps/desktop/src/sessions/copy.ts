import { type Prompt, replyOf, textOf } from "../model.ts";

export const answerMarkdown = (p: Prompt) => `${replyOf(p) ?? ""}\n`;

export function qaMarkdown(p: Prompt): string {
  const reply = replyOf(p);
  return `**You:** ${textOf(p)}\n${reply === null ? "" : `\n${reply}\n`}`;
}

export const sessionMarkdown = (title: string, prompts: Prompt[]) =>
  `# ${title}\n\n${prompts.map(qaMarkdown).join("\n---\n\n")}`;
