import { useEffect, useRef } from "react";
import { useCommands } from "../../shell/commands";
import { useToasts } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { useTerminalFocus } from "./focus";
import { openPopout } from "./popout";
import { openTab, projectKey, sessionsIn, useTerminalLayout } from "./store";
import { useCreateSession } from "./useSessions";

export function TerminalCommands({ navigate }: { navigate: () => void }) {
  const create = useCreateSession();
  const actions = useRef({ create, navigate });
  actions.current = { create, navigate };
  const busy = useRef(false);
  const register = useCommands((state) => state.register);
  useEffect(() => {
    const newTerminal = async () => {
      if (busy.current) return;
      busy.current = true;
      const projectId = useCurrentProject.getState().id ?? null,
        project = projectKey(projectId);
      const focus = useTerminalFocus.getState();
      const layout = useTerminalLayout.getState().byProject[project];
      const active = layout?.tabs.find((tab) => tab.id === layout.active);
      const visible = active ? sessionsIn(active.root) : [];
      const previous = focus.byProject[project];
      const target = previous && visible.includes(previous) ? previous : visible[0];
      try {
        const session = await actions.current.create({
          projectId,
          cwd: target ? (focus.cwd[target] ?? null) : null,
          title: null,
          command: null,
          attachZellij: null,
        });
        useTerminalLayout.getState().update(project, (layout) => openTab(layout, session.id));
        useTerminalFocus.getState().focus(project, session.id);
        actions.current.navigate();
      } catch {
        useToasts.getState().push({ tone: "danger", title: "터미널을 만들지 못했습니다. 다시 시도하세요" });
      } finally {
        busy.current = false;
      }
    };
    const cleanups = [
      register({ id: "term.new", label: "새 터미널", keys: ["Ctrl", "Shift", "T"], run: () => void newTerminal() }),
      register({
        id: "term.popout",
        label: "터미널을 새 창으로",
        run: () => {
          const project = projectKey(useCurrentProject.getState().id);
          const layout = useTerminalLayout.getState().byProject[project];
          const active = layout?.tabs.find((tab) => tab.id === layout.active);
          const visible = active ? sessionsIn(active.root) : [];
          const focused = useTerminalFocus.getState().byProject[project];
          const id = focused && visible.includes(focused) ? focused : visible[0];
          if (!id) {
            useToasts.getState().push({ tone: "info", title: "먼저 터미널을 여세요" });
            return;
          }
          void openPopout("terminal", id).catch(() =>
            useToasts.getState().push({ tone: "danger", title: "새 창을 열지 못했습니다" }),
          );
        },
      }),
    ];
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [register]);
  return null;
}
