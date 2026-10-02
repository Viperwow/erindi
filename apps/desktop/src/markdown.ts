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
    code: ({ text, lang }) =>
      lang === "mermaid" ? `<pre class="mermaid-source"><code>${escape(text)}</code></pre>
` : false,
  },
});

// A private-use character marks where the caret goes; the parser leaves it in the last line's text.
const CARET = "";

/** `caret` ends a reply that is still streaming with a blinking caret. */
export const markdown = (text: string, caret = false) => {
  const html = marked.parse(caret ? text + CARET : text, { async: false });
  return caret ? html.replace(CARET, '<span class="caret" aria-hidden="true"></span>') : html;
};
