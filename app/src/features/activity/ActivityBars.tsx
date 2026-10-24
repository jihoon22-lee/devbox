import { duration } from "./dates";
export function ActivityBars({ items, label }: { items: { label: string; ms: number }[]; label: string }) {
  const maximum = Math.max(1, ...items.map((item) => item.ms));
  return (
    <ol aria-label={label} style={{ listStyle: "none", padding: 0 }}>
      {items.map((item) => (
        <li key={item.label} style={{ display: "grid", gridTemplateColumns: "6em 1fr 7em", gap: 8 }}>
          <span>{item.label}</span>
          <meter
            min={0}
            max={maximum}
            value={Math.max(0, item.ms)}
            aria-label={`${item.label} ${duration(item.ms)}`}
            style={{ width: "100%" }}
          />
          <span>{duration(item.ms)}</span>
        </li>
      ))}
    </ol>
  );
}
