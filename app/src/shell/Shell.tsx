import { RouterProvider } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { ProjectDataProvider } from "../features/projects/ProjectData";
import { ProjectTitle, ProjectSwitcher } from "../features/projects/ProjectSwitcher";
import { SettingsPage } from "../features/settings/SettingsPage";
import { Frame } from "./Frame";
import { CommandPalette } from "./CommandPalette";
import { useCommands } from "./commands";
import { type Binding, useKeymap } from "./keymap";
import { buildRouter } from "./router";
import { SECTIONS } from "./sections";
export function Shell() {
  const [palette, setPalette] = useState(false);
  const [projects, setProjects] = useState(false);
  const router = useMemo(
    () =>
      buildRouter(
        () => (
          <Frame project={<ProjectTitle onOpen={() => setProjects(true)} />} onOpenPalette={() => setPalette(true)} />
        ),
        SettingsPage,
      ),
    [],
  );
  const bindings = useMemo<Binding[]>(
    () => [
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
      <CommandPalette open={palette} onOpenChange={setPalette} />
      <ProjectSwitcher open={projects} onOpenChange={setProjects} />
    </ProjectDataProvider>
  );
}
