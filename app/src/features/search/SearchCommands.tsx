import { useEffect, useMemo } from "react";
import { useCommands } from "../../shell/commands";
import { useKeymap } from "../../shell/keymap";
export const terminalFocused = () => !!document.activeElement?.closest('[data-keyscope="terminal"]');
export function SearchCommands({ navigate }: { navigate(): void }) {
  const register = useCommands((state) => state.register);
  const bindings = useMemo(
    () => [
      {
        id: "search.open",
        code: "KeyF",
        ctrl: true,
        shift: true,
        scope: "app" as const,
        label: "통합 검색",
        enabled: () => !terminalFocused(),
        run: navigate,
      },
    ],
    [navigate],
  );
  useKeymap(bindings);
  useEffect(
    () => register({ id: "search.open", label: "통합 검색", keys: ["Ctrl", "Shift", "F"], run: navigate }),
    [navigate, register],
  );
  return null;
}
