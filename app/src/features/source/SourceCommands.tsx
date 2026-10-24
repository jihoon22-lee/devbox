import { useEffect, useMemo } from "react";
import { useCommands } from "../../shell/commands";
import { useKeymap } from "../../shell/keymap";
import { useCurrentProject } from "../projects/store";
import { QuickOpen } from "./editor/QuickOpen";
import { useSource } from "./store";
export function SourceCommands({ navigate }: { navigate: () => void }) {
  const projectId = useCurrentProject((s) => s.id);
  const open = useSource((s) => s.quickOpen);
  const action = useMemo(
    () => () => {
      if (!projectId) return;
      navigate();
      useSource.setState({ quickOpen: true });
    },
    [navigate, projectId],
  );
  useKeymap(
    useMemo(
      () => [
        {
          id: "source.quickOpen",
          code: "KeyP",
          ctrl: true,
          scope: "app" as const,
          label: "파일 빠른 열기",
          run: action,
          enabled: () => !!projectId,
        },
      ],
      [action, projectId],
    ),
  );
  useEffect(
    () =>
      useCommands
        .getState()
        .register({ id: "source.quickOpen", label: "파일 빠른 열기", keys: ["Ctrl", "P"], run: action }),
    [action],
  );
  return open && projectId ? (
    <QuickOpen key={projectId} projectId={projectId} onClose={() => useSource.setState({ quickOpen: false })} />
  ) : null;
}
