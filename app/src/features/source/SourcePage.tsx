import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { FileEntry, Project } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpc } from "../../rpc/RpcProvider";
import { Button, Dialog, EmptyState, useToasts } from "../../ui";
import { chooseDocumentClose } from "../documents/closePrompt";
import { keyId } from "../documents/offlineJournal";
import { needsCloseChoice, useDocuments } from "../documents/store";
import { useProjectData } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { openTab, projectKey, useTerminalLayout } from "../terminal/store";
import { useCreateSession } from "../terminal/useSessions";
import { BlameGutter } from "./BlameGutter";
import { BranchList } from "./BranchList";
import { ChangesList, type SourceSelection } from "./ChangesList";
import { CleanupPanel } from "./CleanupPanel";
import { CommitBox } from "./CommitBox";
import { DependencyLens } from "./deps/DependencyLens";
import { CodeTabs } from "./editor/CodeTabs";
import { sourceSearch } from "./editor/location";
import { openExternalEditor } from "./externalEditor";
import { type FileOperation, FileOperationDialog } from "./FileOperationDialog";
import { type FileAction, FileTree } from "./FileTree";
import { GitControls } from "./GitControls";
import { LogView } from "./LogView";
import { MergePanel } from "./merge/MergePanel";
import { SourceDiff } from "./SourceDiff";
import { SourceNavigation, type SourceSection } from "./SourceNavigation";
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
  const target = { projectId: project.id };
  const [selected, setSelected] = useState<SourceSelection>();
  const [panel, setPanel] = useState<"editor" | "diff" | "log" | "blame" | "cleanup" | "deps">("editor");
  const active = useSource((state) =>
    state.layout.tabs.find((tab) => tab.key.root === root && tab.id === state.layout.active[state.layout.group]),
  );
  const locationRequest = useSource((state) => (active ? state.locations[keyId(active.key)]?.request : undefined));
  useEffect(() => {
    if (locationRequest !== undefined) setPanel("editor");
  }, [locationRequest]);
  const [section, setSection] = useState<SourceSection>("files");
  const [mergeOpen, setMergeOpen] = useState(false);
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
          : error instanceof Error
            ? error.message
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
      <SourceNavigation section={section} onChange={setSection}>
        {section === "files" ? (
          <>
            <div className={s.toolbar}>
              <strong>파일</strong>
              <Button
                size="sm"
                disabled={busy}
                onClick={() => setOperation({ kind: "create", key: { root, path: "" } })}
              >
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
            <FileTree
              projectId={project.id}
              showHidden={hidden}
              onAction={(kind, entry) => void action(kind, entry)}
              onOpen={() => setPanel("editor")}
            />
          </>
        ) : section === "changes" ? (
          <>
            <ChangesList
              target={target}
              selected={selected}
              onSelect={(value) => {
                setSelected(value);
                setPanel("diff");
              }}
            />
            <CommitBox target={target} />
          </>
        ) : (
          <BranchList target={target} />
        )}
      </SourceNavigation>
      <div className={s.workspace}>
        <div className={s.toolbar}>
          <Button size="sm" onClick={() => useSource.setState({ quickOpen: true })}>
            빠른 열기 · Ctrl+P
          </Button>
          <Button size="sm" onClick={() => setMergeOpen(true)}>
            충돌 해결
          </Button>
        </div>
        <GitControls target={target} />
        <div className={s.toolbar}>
          <Button onClick={() => setPanel("editor")}>편집기</Button>
          {selected && <Button onClick={() => setPanel("diff")}>선택한 변경 · {selected.path}</Button>}
          <Button onClick={() => setPanel("log")}>커밋 기록</Button>
          <Button disabled={!selected?.path && !active} onClick={() => setPanel("blame")}>
            줄별 변경 기록
          </Button>
          <Button onClick={() => setPanel("cleanup")}>작업 폴더 정리</Button>
          <Button onClick={() => setPanel("deps")}>의존성 점검</Button>
        </div>
        <div className={s.editorWorkspace} hidden={panel !== "editor"}>
          <CodeTabs root={root} />
        </div>
        {panel !== "editor" && (
          <div className={s.gitPanel}>
            {panel === "diff" &&
              selected &&
              (selected.conflicted ? (
                <MergePanel target={target} initialPath={selected.path} />
              ) : (
                <SourceDiff target={target} path={selected.path} staged={selected.staged} />
              ))}
            {panel === "log" && <LogView target={target} />}
            {panel === "blame" && (selected?.path ?? active?.key.path) && (
              <BlameGutter
                target={target}
                path={(selected?.path ?? active?.key.path)!}
                onOpen={(line) => {
                  void useSource
                    .getState()
                    .open(
                      { root, path: (selected?.path ?? active?.key.path)! },
                      { pinned: true, location: { line, column: 1 } },
                    );
                  setPanel("editor");
                }}
              />
            )}
            {panel === "cleanup" && <CleanupPanel target={target} />}
            {panel === "deps" && <DependencyLens projectId={project.id} />}
          </div>
        )}
      </div>
      <Dialog open={mergeOpen} onOpenChange={setMergeOpen} title="충돌 해결" width={1100}>
        {mergeOpen && <MergePanel target={{ projectId: project.id }} />}
      </Dialog>
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
