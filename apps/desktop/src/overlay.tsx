import { type ComponentChildren, render } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useBusy } from "./controls";
import { type Bubble, type Mic, type Phrase, type Running, type View, bubble } from "./bubble.ts";
import "./style.css";

const svg = { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" } as const;

const MicIcon = () => (
  <svg class="icon" {...svg}>
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
  </svg>
);

const MicOffIcon = () => (
  <svg class="icon" {...svg}>
    <path d="M9 9v2a3 3 0 0 0 5.1 2.1M15 9.3V6a3 3 0 0 0-5.7-1.3M5 11a7 7 0 0 0 11.9 5M19 11a7 7 0 0 1-.4 2.3M12 18v3M3 3l18 18" />
  </svg>
);

// The pencil fills its square diagonally; a padded view box makes it read the same size as the microphone.
const PencilIcon = () => (
  <svg class="icon" {...svg} viewBox="-3 -3 30 30">
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
  </svg>
);

const TerminalIcon = () => (
  <svg class="icon" {...svg}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="m7 10 3 2-3 2" />
    <path d="M13 15h4" />
  </svg>
);

const Dot = ({ kind }: { kind: "ok" | "err" }) => (
  <svg class={`dot ${kind}`} viewBox="0 0 13 13">
    <circle cx="6.5" cy="6.5" r="3.5" fill="currentColor" />
  </svg>
);

/** A row that shows a tooltip when it carries an outcome or when its text is cut off. */
function Tip(props: {
  text: string;
  outcome?: string | null;
  kind?: "ok" | "err";
  clickHint?: string | null;
  class: string;
  children: ComponentChildren;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [cut, setCut] = useState(false);
  useLayoutEffect(() => {
    const text = ref.current?.querySelector<HTMLElement>(".t");
    if (text) setCut(text.scrollWidth > text.clientWidth + 1 || text.scrollHeight > text.clientHeight + 1);
  }, [props.text, props.class]);
  const show = Boolean(props.outcome) || cut;
  return (
    <div ref={ref} class={`${props.class} ${show ? "has-tip" : ""}`}>
      {props.children}
      {show && (
        <div class="tip">
          {props.outcome && props.kind && (
            <div class={`why ${props.kind}`}>
              <Dot kind={props.kind} />
              {props.outcome}
            </div>
          )}
          <div class="full">{props.text}</div>
          {props.clickHint && <div class="hint">{props.clickHint}</div>}
        </div>
      )}
    </div>
  );
}

const live = new Set(["run", "speak", "decode"]);

function PhraseRow({ phrase }: { phrase: Phrase }) {
  const kind = phrase.rail === "ok" ? "ok" : phrase.rail === "err" ? "err" : undefined;
  return (
    <Tip
      text={phrase.text}
      outcome={phrase.outcome}
      kind={kind}
      clickHint={phrase.clickHint}
      class={`rail r-${phrase.rail} ${live.has(phrase.rail) ? "live" : "muted"}`}
    >
      <span class={`t ${phrase.rail === "run" ? "two" : ""}`}>{phrase.text}</span>
    </Tip>
  );
}

const micLabel: Record<Mic, string> = { waiting: "Waiting", listening: "Listening", off: "Mic off", error: "Mic unavailable" };

function MicSlot({ mic }: { mic: Mic }) {
  return (
    <div class="mic-slot">
      {mic === "off" || mic === "error" ? <MicOffIcon /> : <MicIcon />}
      <span class={`label ${mic === "error" ? "light" : ""}`}>
        {micLabel[mic]}
        {mic === "listening" && <span class="ell" />}
      </span>
    </div>
  );
}

function RunningSlot({ running }: { running: NonNullable<Running> }) {
  return (
    <Tip text={running.text} class="run-slot">
      {running.icon === "pencil" ? <PencilIcon /> : <TerminalIcon />}
      <span class="t label">
        {running.text}
        {running.dots && <span class="ell" />}
      </span>
    </Tip>
  );
}

function BottomRow({ b }: { b: Bubble }) {
  const alone = b.phrases.length === 0 ? "alone" : "";
  if (b.globalError) {
    return (
      <div class={`foot global ${alone}`}>
        <MicOffIcon />
        <span class="label light">{b.globalError}</span>
        <span />
      </div>
    );
  }
  return (
    <div class={`foot ${alone}`}>
      <MicSlot mic={b.mic} />
      {b.running && <RunningSlot running={b.running} />}
    </div>
  );
}

function Overlay() {
  const [view, setView] = useState<View | null>(null);
  const openSession = useBusy().run(() => invoke("open_session"));

  useEffect(() => {
    const off = listen<View>("view", (e) => setView(e.payload));
    return () => {
      off.then((f) => f());
    };
  }, []);

  // Rust takes the mouse only over this box, so tooltips and clicks work on the bubble alone.
  const bubbleRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = bubbleRef.current;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      invoke("set_bubble_rect", { rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom } });
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [view]);

  if (!view) return null;
  const b = bubble(view);
  return (
    <div class="fixed inset-0 flex items-end justify-center pb-3 select-none">
      <div
        id="bubble"
        ref={bubbleRef}
        class={`bubble ${b.clickable ? "clickable" : ""}`}
        onClick={b.clickable ? openSession : undefined}
      >
        <div class="body">
          {b.phrases.map((phrase) => (
            <PhraseRow phrase={phrase} />
          ))}
          <BottomRow b={b} />
        </div>
        <div class="stripwrap">
          <div class={`strip ${b.strip}`} />
        </div>
      </div>
    </div>
  );
}

render(<Overlay />, document.getElementById("app")!);
