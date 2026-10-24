import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { FileEntry, Project } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpc } from "../../rpc/RpcProvider";
import { Button, EmptyState, useToasts } from "../../ui";
import { chooseDocumentClose } from "../documents/closePrompt";
import { needsCloseChoice, useDocuments } from "../documents/store";
import { useProjectData } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { openTab, projectKey, useTerminalLayout } from "../terminal/store";
import { useCreateSession } from "../terminal/useSessions";
import { CodeTabs } from "./editor/CodeTabs";
import { sourceSearch } from "./editor/location";
import { openExternalEditor } from "./externalEditor";
import { type FileOperation, FileOperationDialog } from "./FileOperationDialog";
import { type FileAction, FileTree } from "./FileTree";
import s from "./source.module.css";
import { useSource } from "./store";
export function SourceRoute() {
  const search = sourceSearch(useSearch({ strict: false }) as Record<string, unknown>);
  useEffect(() => {
    if (!search.root || !search.path) return;
    useCurrentProject.getState().set(search.root.slice("project:".length));
    void useSource
      .getState()
      .open(
        { root: search.root, path: search.path },
        { pinned: true, location: { line: search.line, column: search.column } },
      );
  }, [search.root, search.path, search.line, search.column]);
  return <SourcePage />;
}
export function SourcePage() {
  const id = useCurrentProject((s) => s.id);
  const projects = useProjectData();
  const project = projects.data?.items.find((p) => p.id === id);
  return project ? (
    <ProjectSource key={project.id} project={project} />
  ) : (
    <EmptyState title="프로젝트를 선택하세요" body="프로젝트의 파일을 열어 편집할 수 있습니다." />
  );
}
function ProjectSource({ project }: { project: Project }) {
  const root = `project:${project.id}`;
  const [hidden, setHidden] = useState(false);
  const [operation, setOperation] = useState<FileOperation>();
  const client = useRpc();
  const cache = useQueryClient();
  const navigate = useNavigate();
  const createTerminal = useCreateSession();
  const [busy, setBusy] = useState(false);
  const fail = (error: unknown) =>
    useToasts.getState().push({
      tone: "danger",
      title:
        error instanceof RpcError
          ? messageFor(error.code, error.detail).title
          : "파일 작업을 마치지 못했습니다. 경로와 앱 설치를 확인하세요.",
    });
  const action = async (kind: FileAction, entry: FileEntry) => {
    if (busy || useDocuments.getState().windowClosing) return;
    setBusy(true);
    try {
      if (kind === "code" || kind === "cursor" || kind === "zed") {
        await openExternalEditor(client, entry.key, kind);
        return;
      }
      if (kind === "terminal") {
        const dir = entry.kind === "directory" ? entry.key.path : entry.key.path.split("/").slice(0, -1).join("/");
        const session = await createTerminal({
          projectId: project.id,
          cwd: `${project.path}${dir ? `/${dir}` : ""}`,
          title: dir || project.name,
          command: null,
          attachZellij: null,
        });
        useTerminalLayout.getState().update(projectKey(project.id), (layout) => openTab(layout, session.id));
        if (useCurrentProject.getState().id === project.id) void navigate({ to: "/terminal" });
        return;
      }
      if (kind === "delete") {
        const doc = useDocuments.getState().find(entry.key);
        if (doc && needsCloseChoice(doc)) {
          const choices = await chooseDocumentClose([doc], new AbortController().signal);
          if (!choices) return;
          if (!(await useSource.getState().close(entry.key, choices[doc.id]))) throw new Error("close failed");
        }
      }
      const key =
        kind === "create"
          ? {
              root,
              path:
                entry.kind === "directory"
                  ? `${entry.key.path}/`
                  : entry.key.path.split("/").slice(0, -1).join("/") + "/",
            }
          : entry.key;
      if (key.path === "/") key.path = "";
      setOperation({ kind, key });
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <main className={s.page} aria-label="소스 파일">
      <aside className={s.sidebar} aria-label="파일 목록">
        <div className={s.toolbar}>
          <strong>파일</strong>
          <Button size="sm" disabled={busy} onClick={() => setOperation({ kind: "create", key: { root, path: "" } })}>
            새 파일
          </Button>
          <Button size="sm" onClick={() => void cache.invalidateQueries({ queryKey: ["files.tree"] })}>
            새로고침
          </Button>
        </div>
        <label>
          <input type="checkbox" checked={hidden} onChange={(e) => setHidden(e.currentTarget.checked)} />
          숨김 파일
        </label>
        <FileTree projectId={project.id} showHidden={hidden} onAction={(kind, entry) => void action(kind, entry)} />
      </aside>
      <div className={s.workspace}>
        <div className={s.toolbar}>
          <Button size="sm" onClick={() => useSource.setState({ quickOpen: true })}>
            빠른 열기 · Ctrl+P
          </Button>
        </div>
        <CodeTabs root={root} />
      </div>
      {operation && (
        <FileOperationDialog
          key={JSON.stringify(operation)}
          operation={operation}
          onClose={() => setOperation(undefined)}
        />
      )}
    </main>
  );
}
