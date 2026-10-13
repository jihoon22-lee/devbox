import { RouterProvider } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { ProjectDataProvider } from "../features/projects/ProjectData";
import { ProjectSwitcher, ProjectTitle } from "../features/projects/ProjectSwitcher";
import { SettingsPage } from "../features/settings/SettingsPage";
import { ToastHost } from "../ui/Toast";
import { CommandPalette } from "./CommandPalette";
import { useCommands } from "./commands";
import { Frame } from "./Frame";
import { type Binding, useKeymap } from "./keymap";
import { buildRouter } from "./router";
import { SECTIONS } from "./sections";
import { restoreUiScale, uiBindings } from "./uiShortcuts";
export function Shell() {
  const [palette, setPalette] = useState(false);
  const [projects, setProjects] = useState(false);
  const router = useMemo(
    () =>
      buildRouter({
        frame: () => (
          <Frame project={<ProjectTitle onOpen={() => setProjects(true)} />} onOpenPalette={() => setPalette(true)} />
        ),
        pages: {},
        settings: SettingsPage,
        popouts: [],
      }),
    [],
  );
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
  useEffect(() => {
    restoreUiScale();
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
    return () => window.removeEventListener("mouseup", onMouse);
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
    cleanups.push(
      register({ id: "settings.open", label: "설정 열기", run: () => void router.navigate({ to: "/settings" }) }),
    );
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [bindings, register, router]);
  return (
    <ProjectDataProvider>
      <RouterProvider router={router} />
      <ToastHost />
      <CommandPalette open={palette} onOpenChange={setPalette} />
      <ProjectSwitcher open={projects} onOpenChange={setProjects} />
    </ProjectDataProvider>
  );
}
