import path from "node:path";
export async function selectRegisteredWorkspaceRoot(ui, registry, wait, root) {
  let project;
  const sameRoot = (value) =>
    root.startsWith("/")
      ? value === root
      : typeof value === "string" &&
        path.win32.normalize(value).toLowerCase() === path.win32.normalize(root).toLowerCase();
  await wait(async () => {
    const snapshot = await registry();
    const tree = snapshot.worktrees.find((item) => sameRoot(item.binding.root));
    project = snapshot.projects.find((item) => item.id === tree?.projectId);
    return !!project;
  }, "owned root registered");
  const target = { role: "button", name: "프로젝트 선택", scope: { role: "region", name: project.name } };
  await ui.waitForTarget(target);
  await ui.click(target);
  return project;
}
