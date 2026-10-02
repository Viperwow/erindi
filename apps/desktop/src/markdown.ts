import { Marked } from "marked";

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/**
 * A model's reply is untrusted, and the page it lands on can call the app's commands. So HTML in it
 * shows as text, links keep only web and mail addresses, and images show their alt text.
 */
const marked = new Marked({
  gfm: true,
  breaks: true,
  renderer: {
    html: ({ text }) => escape(text),
    link({ href, tokens }) {
      const text = this.parser.parseInline(tokens);
      return /^(https?:|mailto:)/i.test(href) ? `<a href="${escape(href)}" target="_blank" rel="noreferrer">${text}</a>` : text;
    },
    image: ({ text }) => escape(text),
  },
});

export const markdown = (text: string) => marked.parse(text, { async: false });
