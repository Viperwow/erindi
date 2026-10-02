/** Drawn diagrams by source, so a re-render does not draw them again. */
const drawn = new Map<string, string>();
let next = 0;

/**
 * Replaces each mermaid block under `root` with its diagram. Mermaid loads only when a reply has
 * one. A block that does not parse stays as code.
 */
export async function drawDiagrams(root: HTMLElement) {
  const blocks = [...root.querySelectorAll<HTMLElement>("pre.mermaid-source")];
  if (blocks.length === 0) return;
  const { default: mermaid } = await import("mermaid");
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  // "strict" sanitizes labels and turns off click handlers: the diagram comes from a model.
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default" });
  for (const block of blocks) {
    const source = block.textContent ?? "";
    let svg = drawn.get(source);
    if (svg === undefined) {
      // A failed render leaves mermaid's error graphic in the page, so a block that does not parse is never rendered.
      if (!(await mermaid.parse(source, { suppressErrors: true }))) continue;
      try {
        svg = (await mermaid.render(`diagram-${next++}`, source)).svg;
        drawn.set(source, svg);
      } catch {
        continue;
      }
    }
    if (!block.isConnected) continue;
    const figure = document.createElement("div");
    figure.className = "diagram";
    figure.setAttribute("role", "img");
    figure.setAttribute("aria-label", `Diagram: ${source.trim().split("\n")[0]}`);
    figure.innerHTML = svg;
    block.replaceWith(figure);
  }
}
