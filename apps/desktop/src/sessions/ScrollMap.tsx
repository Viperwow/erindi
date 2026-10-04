import { useEffect, useRef, useState } from "preact/hooks";

export type Tick = { turn: number; label: string; kind: "current" | "failed" | "match" };

type Geometry = { top: number; view: number; total: number; offsets: number[] };

/**
 * The conversation's scrollbar: a thumb for the visible part and ticks for the turns worth finding.
 * Ticks come from where the turns really are, so a long answer pushes the next tick down with it.
 */
export function ScrollMap(props: {
  scroller: { current: HTMLDivElement | null };
  ticks: Tick[];
  onJump: (turn: number) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const [g, setG] = useState<Geometry>({ top: 0, view: 1, total: 1, offsets: [] });

  useEffect(() => {
    const el = props.scroller.current;
    if (!el) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      const turns = [...el.querySelectorAll<HTMLElement>("[data-turn]")];
      setG({ top: el.scrollTop, view: el.clientHeight, total: el.scrollHeight, offsets: turns.map((t) => t.offsetTop) });
    };
    const later = () => {
      if (!frame) frame = requestAnimationFrame(measure);
    };
    measure();
    el.addEventListener("scroll", later, { passive: true });
    // Turns change height as they render and as answers stream in.
    const sizes = new ResizeObserver(later);
    sizes.observe(el);
    if (el.firstElementChild) sizes.observe(el.firstElementChild);
    return () => {
      el.removeEventListener("scroll", later);
      sizes.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [props.scroller]);

  if (g.total <= g.view + 1) return null;
  const pct = (px: number) => `${(px / g.total) * 100}%`;

  const drag = (e: PointerEvent) => {
    const el = props.scroller.current;
    const height = track.current?.clientHeight;
    if (!el || !height) return;
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const start = { y: e.clientY, top: el.scrollTop };
    const move = (m: PointerEvent) => {
      el.scrollTop = start.top + ((m.clientY - start.y) * g.total) / height;
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  const page = (e: MouseEvent) => {
    const el = props.scroller.current;
    const box = track.current?.getBoundingClientRect();
    if (!el || !box || e.target !== track.current) return;
    el.scrollTop = ((e.clientY - box.top) / box.height) * g.total - g.view / 2;
  };

  return (
    <div ref={track} class="ses-map" aria-hidden="true" onClick={page}>
      {props.ticks.map((t) => (
        <button
          key={t.turn}
          type="button"
          tabIndex={-1}
          title={t.label}
          class={`ses-tick ${t.kind}`}
          style={{ top: pct(g.offsets[t.turn - 1] ?? 0) }}
          onClick={() => props.onJump(t.turn)}
        />
      ))}
      <div class="ses-thumb" style={{ top: pct(g.top), height: pct(g.view) }} onPointerDown={drag} />
    </div>
  );
}
