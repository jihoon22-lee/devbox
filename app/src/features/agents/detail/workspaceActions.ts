import { invoke } from "@tauri-apps/api/core";
import { useToasts } from "../../../ui";
import { openPopout } from "../../terminal/popout";
import { useCreateSession } from "../../terminal/useSessions";
import type { AgentTask } from "../model";

const toast = (title: string, body?: string) => useToasts.getState().push({ tone: "danger", title, body });

/** [더 보기] items that open a task's worktree folder; empty for a closed task or one without a folder. */
export function useWorkspaceItems(task: AgentTask) {
  const create = useCreateSession();
  const path = task.worktreePath;
  if (!path || task.state === "closed") return [];
  const terminal = async () => {
    // The task's own project and folder: the global current project stays as it is.
    const session = await create({
      projectId: task.projectId,
      cwd: path,
      title: null,
      command: null,
      attachZellij: null,
    }).catch(() => null);
    if (!session) return toast("터미널을 만들지 못했습니다. 다시 시도하세요");
    await openPopout("terminal", session.id).catch(() => toast("새 창을 열지 못했습니다"));
  };
  const code = async () => {
    try {
      await invoke("open_external", { kind: "vscode", path });
    } catch {
      toast("외부 앱을 열지 못했습니다", "연결 상태와 앱 설치를 확인해 주세요.");
    }
  };
  return [
    ...("__TAURI_INTERNALS__" in window ? [{ label: "VS Code로 작업 폴더 열기", onSelect: () => void code() }] : []),
    { label: "작업 폴더 터미널", onSelect: () => void terminal() },
  ];
}
