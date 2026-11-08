export function Kbd({ keys }: { keys: string[] }) {
  return (
    <kbd
      style={{
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-xs)",
        // A filled container (primary button) sets these; elsewhere the muted defaults apply.
        color: "var(--kbd-color, var(--fg-muted))",
        border: "1px solid var(--kbd-border, var(--border))",
        borderRadius: "var(--radius-sm)",
        padding: "0 4px",
      }}
    >
      {keys.join("+")}
    </kbd>
  );
}
