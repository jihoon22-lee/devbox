export function Kbd({ keys }: { keys: string[] }) {
  return (
    <kbd
      style={{
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-xs)",
        color: "var(--fg-muted)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
        padding: "0 4px",
      }}
    >
      {keys.join("+")}
    </kbd>
  );
}
