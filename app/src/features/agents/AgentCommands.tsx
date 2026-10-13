import { useEffect, useMemo, useRef } from "react";
import { useCommands } from "../../shell/commands";
import { type Binding, useKeymap } from "../../shell/keymap";
import { useAgentData } from "./AgentData";
import { nextWaiting } from "./model";
import { NewTaskDialog } from "./NewTaskDialog";
import { useAgentsUi } from "./ui";
export function AgentCommands({
  navigate,
  currentId,
}: {
  navigate: (id?: string) => void;
  currentId: () => string | undefined;
}) {
  const data = useAgentData();
  const { creating, setCreating } = useAgentsUi();
  const actions = useRef({ navigate, currentId, data });
  actions.current = { navigate, currentId, data };
  const bindings = useMemo<Binding[]>(
    () => [
      {
        id: "agents.new",
        code: "KeyN",
        ctrl: true,
        shift: true,
        scope: "app",
        label: "새 에이전트 작업",
        run: () => {
          setCreating(true);
          actions.current.navigate();
        },
      },
      {
        id: "agents.nextWaiting",
        code: "KeyJ",
        ctrl: true,
        shift: true,
        scope: "app",
        label: "다음 입력 대기",
        run: () => {
          const id = nextWaiting(actions.current.data.groups, actions.current.currentId());
          if (id) actions.current.navigate(id);
        },
      },
    ],
    [setCreating],
  );
  useKeymap(bindings);
  const register = useCommands((state) => state.register);
  useEffect(() => {
    const cleanups = bindings.map((binding) =>
      register({
        id: binding.id,
        label: binding.label,
        keys: ["Ctrl", "Shift", binding.code.slice(3)],
        run: binding.run,
      }),
    );
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [bindings, register]);
  return <NewTaskDialog open={creating} onOpenChange={setCreating} onCreated={(id) => actions.current.navigate(id)} />;
}
