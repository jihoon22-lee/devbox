import { useEffect } from "react";
export type Scope = "global" | "app" | "section" | "terminal" | "editor";
export interface Binding {
  id: string;
  code: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  scope: Scope;
  label: string;
  run: () => void;
  enabled?: () => boolean;
}
export function matches(e: KeyboardEvent, b: Binding): boolean {
  return (
    !e.isComposing &&
    e.keyCode !== 229 &&
    !e.metaKey &&
    e.code === b.code &&
    e.ctrlKey === !!b.ctrl &&
    e.shiftKey === !!b.shift &&
    e.altKey === !!b.alt
  );
}
export function allowedWhileFocused(b: Binding, focus: "terminal" | "editor" | null): boolean {
  if (!focus) return true;
  if (/^F(?:[1-9]|1\d|2[0-4])$/.test(b.code)) return true;
  if (b.ctrl && /^Digit\d$/.test(b.code)) return true;
  return !!b.ctrl && (!!b.shift || !!b.alt) && /^Key[A-Z]$/.test(b.code);
}
export function useKeymap(bindings: Binding[]): void {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]')
      )
        return;
      const focused = document.activeElement?.closest("[data-keyscope]")?.getAttribute("data-keyscope");
      const scope = focused === "terminal" || focused === "editor" ? focused : null;
      for (const binding of bindings)
        if (matches(event, binding) && allowedWhileFocused(binding, scope) && (binding.enabled?.() ?? true)) {
          event.preventDefault();
          event.stopPropagation();
          binding.run();
          return;
        }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [bindings]);
}
