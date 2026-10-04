import type { Mark } from "./status";

const labels: Record<Mark, string> = {
  waiting: "Waiting for your voice",
  speak: "Listening",
  decode: "Transcribing",
  run: "Answering",
  idle: "Idle",
};

/** The session's state left of its title. Shapes differ as well as colours, for colour-blind people. */
export function SessionMark(props: { mark: Mark }) {
  const { mark } = props;
  return (
    <svg class={`ses-mark ${mark}`} viewBox="0 0 12 12" role="img" aria-label={labels[mark]}>
      <title>{labels[mark]}</title>
      {mark === "waiting" && <circle cx="6" cy="6" r="3.4" fill="none" stroke="#22508a" stroke-width="1.4" />}
      {mark === "speak" && <circle cx="6" cy="6" r="3.5" fill="#38bdf8" />}
      {mark === "decode" && <polygon points="6,9.45 2.02,2.55 9.98,2.55" fill="#a855f7" stroke="#a855f7" stroke-width="0.6" />}
      {mark === "run" && (
        <polygon points="6,1.6 10.18,4.64 8.59,9.56 3.41,9.56 1.82,4.64" fill="#f59e0b" stroke="#f59e0b" stroke-width="0.6" />
      )}
      {mark === "idle" && <circle cx="6" cy="6" r="3.5" fill="#737373" />}
    </svg>
  );
}
