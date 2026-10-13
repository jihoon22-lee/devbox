import { RouterProvider } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useMemo, useState } from "react";
import { AgentCommands } from "../features/agents/AgentCommands";
import { AgentDataProvider } from "../features/agents/AgentData";
import { PopoutAgentGrid } from "../features/agents/AgentGrid";
import { AgentsPage, agentsSearch } from "../features/agents/AgentsPage";
import { PopoutAgent } from "../features/agents/detail/AgentDetail";
import { AttentionBridge } from "../features/notifications/AttentionBridge";
import { subscribeNativeNavigation } from "../features/notifications/native";
import { ProjectDataProvider } from "../features/projects/ProjectData";
import { ProjectSwitcher, ProjectTitle } from "../features/projects/ProjectSwitcher";
import { openSettings, registerSettingsNavigation } from "../features/settings/open";
import { restorePreferences } from "../features/settings/preferences";
import { SettingsPage } from "../features/settings/SettingsPage";
import { BottomTerminal } from "../features/terminal/BottomTerminal";
import { PopoutTerminal } from "../features/terminal/PopoutTerminal";
import { TerminalCommands } from "../features/terminal/TerminalCommands";
import { TerminalPage } from "../features/terminal/TerminalPage";
import { ToastHost } from "../ui/Toast";
import { registerBottomTab } from "./BottomPanel";
import { CommandPalette } from "./CommandPalette";
import { useCommands } from "./commands";
import { Frame } from "./Frame";
import { type Binding, useKeymap } from "./keymap";
import { buildRouter } from "./router";
import { SECTIONS } from "./sections";
import { uiBindings } from "./uiShortcuts";
export function Shell() {
  // Window identity survives navigation; browser popouts also have a stable target name.
  const [ownsAttention] = useState(() =>
    "__TAURI_INTERNALS__" in window
      ? getCurrentWindow().label === "main"
      : !window.name.startsWith("popout-") && !window.location.pathname.startsWith("/popout/"),
  );
  const [palette, setPalette] = useState(false);
  const [projects, setProjects] = useState(false);
  const router = useMemo(
    () =>
      buildRouter({
        frame: () => (
          <Frame project={<ProjectTitle onOpen={() => setProjects(true)} />} onOpenPalette={() => setPalette(true)} />
        ),
        pages: {
          "/terminal": { component: TerminalPage },
          "/agents": { component: AgentsPage, validateSearch: agentsSearch },
        },
        settings: SettingsPage,
        popouts: [
          { path: "/popout/terminal/$id", component: PopoutTerminal },
          { path: "/popout/agent/$id", component: PopoutAgent },
          { path: "/popout/agents-grid", component: PopoutAgentGrid },
        ],
      }),
    [],
  );
  const attentionLocation = useMemo(() => () => router.state.location, [router]);
  useEffect(() => {
    if (ownsAttention) return subscribeNativeNavigation((target) => router.navigate(target));
  }, [ownsAttention, router]);
  useEffect(() => registerSettingsNavigation(() => void router.navigate({ to: "/settings" })), [router]);
  const bindings = useMemo<Binding[]>(
    () => [
      ...uiBindings(),
      ...SECTIONS.map((section) => ({
        id: `go.${section.path}`,
        code: `Digit${section.digit}`,
        ctrl: true,
        scope: "app" as const,
        label: `${section.label} 열기`,
        run: () => void router.navigate({ to: section.path }),
      })),
      {
        id: "palette",
        code: "KeyP",
        ctrl: true,
        shift: true,
        scope: "app",
        label: "명령 팔레트",
        run: () => setPalette(true),
      },
      {
        id: "projects.open",
        code: "KeyO",
        ctrl: true,
        shift: true,
        scope: "app",
        label: "프로젝트 전환",
        run: () => setProjects(true),
      },
      { id: "palette.f1", code: "F1", scope: "app", label: "명령 팔레트", run: () => setPalette(true) },
      { id: "palette.k", code: "KeyK", ctrl: true, scope: "app", label: "명령 팔레트", run: () => setPalette(true) },
      { id: "nav.back", code: "ArrowLeft", alt: true, scope: "app", label: "뒤로", run: () => router.history.back() },
      {
        id: "nav.forward",
        code: "ArrowRight",
        alt: true,
        scope: "app",
        label: "앞으로",
        run: () => router.history.forward(),
      },
    ],
    [router],
  );
  useKeymap(bindings);
  useEffect(() => registerBottomTab({ id: "terminal", label: "터미널", render: () => <BottomTerminal /> }), []);
  useEffect(() => {
    const cleanupPreferences = restorePreferences();
    const onMouse = (event: MouseEvent) => {
      if (
        event.defaultPrevented ||
        document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]')
      )
        return;
      if (event.button !== 3 && event.button !== 4) return;
      event.preventDefault();
      if (event.button === 3) router.history.back();
      else router.history.forward();
    };
    window.addEventListener("mouseup", onMouse);
    return () => {
      window.removeEventListener("mouseup", onMouse);
      cleanupPreferences();
    };
  }, [router]);
  const register = useCommands((state) => state.register);
  useEffect(() => {
    const cleanups = bindings
      .filter((binding) => !binding.id.startsWith("palette."))
      .map((binding) =>
        register({
          id: binding.id,
          label: binding.label,
          keys: [
            ...(binding.ctrl ? ["Ctrl"] : []),
            ...(binding.shift ? ["Shift"] : []),
            ...(binding.alt ? ["Alt"] : []),
            ({ Backquote: "`", Equal: "=", Minus: "-" } as Record<string, string>)[binding.code] ??
              binding.code.replace(/^(Key|Digit)/, ""),
          ],
          run: binding.run,
        }),
      );
    cleanups.push(register({ id: "settings.open", label: "설정 열기", run: () => openSettings("general") }));
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [bindings, register]);
  return (
    <ProjectDataProvider>
      <AgentDataProvider>
        {ownsAttention && <AttentionBridge location={attentionLocation} />}
        <RouterProvider router={router} />
        <AgentCommands
          navigate={(id) =>
            void router.navigate({
              to: "/agents",
              search: (previous: Record<string, unknown>) => ({ ...previous, id, view: "list" }),
            })
          }
          currentId={() => {
            const search = router.state.location.search as Record<string, unknown>;
            return router.state.location.pathname === "/agents" && typeof search.id === "string"
              ? search.id
              : undefined;
          }}
        />
        <TerminalCommands navigate={() => void router.navigate({ to: "/terminal" })} />
        <ToastHost />
        <CommandPalette open={palette} onOpenChange={setPalette} />
        <ProjectSwitcher
          open={projects}
          onOpenChange={setProjects}
          onLaunched={() => void router.navigate({ to: "/terminal" })}
        />
      </AgentDataProvider>
    </ProjectDataProvider>
  );
}
