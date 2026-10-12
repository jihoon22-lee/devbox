import s from "./StatusDot.module.css";

const SHAPE = { ok: "circle", progress: "diamond", idle: "ring", warn: "triangle", danger: "square" } as const;

export function StatusDot({ tone, label }: { tone: keyof typeof SHAPE; label: string }) {
  return <span role="img" aria-label={label} className={`${s.dot} ${s[tone]}`} data-shape={SHAPE[tone]} />;
}
