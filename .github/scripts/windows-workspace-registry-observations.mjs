import path from "node:path";
function sameRoot(root, value) {
  return root.startsWith("/")
    ? value === root
    : typeof value === "string" &&
        path.win32.normalize(value).toLowerCase() === path.win32.normalize(root).toLowerCase();
}
export async function waitForSelectedWorkspaceRoot(context, registry, wait, root) {
  await wait(async () => {
    const selected = await context();
    const snapshot = await registry();
    return snapshot.worktrees.some(
      (tree) =>
        sameRoot(root, tree.binding.root) &&
        tree.projectId === selected?.projectId &&
        tree.id === selected?.worktreeId &&
        tree.revision === selected?.revision,
    );
  }, "exact owned root native context selected");
}
export async function selectRegisteredWorkspaceRoot(ui, registry, wait, root) {
  let project;
  await wait(async () => {
    const snapshot = await registry();
    const tree = snapshot.worktrees.find((item) => sameRoot(root, item.binding.root));
    project = snapshot.projects.find((item) => item.id === tree?.projectId);
    return !!project;
  }, "owned root registered");
  const target = { role: "button", name: "프로젝트 선택", scope: { role: "region", name: project.name } };
  await ui.waitForTarget(target);
  await ui.click(target);
  return project;
}
