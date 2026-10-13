import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { Project } from "../../rpc/gen/rpc";
import { confirmPolicy, invalidateIfNewer, useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Button, ConfirmDialog, Dialog, useToasts } from "../../ui";
import { projectKey, useTerminalLayout } from "../terminal/store";
import { AddProjectDialog } from "./AddProjectDialog";
import { ProjectCard } from "./ProjectCard";
import { PROJECT_KEYS, ProjectDataBoundary, recordProjectRemoval, useProjectData } from "./ProjectData";
import { ProjectError } from "./ProjectError";
import s from "./Projects.module.css";
import { useCurrentProject } from "./store";
export function ProjectTitle({ onOpen }: { onOpen: () => void }) {
  return (
    <ProjectDataBoundary>
      <Title onOpen={onOpen} />
    </ProjectDataBoundary>
  );
}
function Title({ onOpen }: { onOpen: () => void }) {
  const id = useCurrentProject((state) => state.id);
  const list = useProjectData();
  const project = list.data?.items.find((item) => item.id === id);
  return (
    <Button variant="ghost" size="sm" onClick={onOpen} aria-label="프로젝트 전환">
      {project?.name ?? "프로젝트 선택"} <ChevronDown size={14} />
    </Button>
  );
}
export function ProjectSwitcher({
  open,
  onOpenChange,
  onLaunched,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onLaunched?: () => void;
}) {
  return open ? (
    <ProjectDataBoundary>
      <Switcher onOpenChange={onOpenChange} onLaunched={onLaunched} />
    </ProjectDataBoundary>
  ) : null;
}
function Switcher({ onOpenChange, onLaunched }: { onOpenChange: (open: boolean) => void; onLaunched?: () => void }) {
  const list = useProjectData();
  const qc = useQueryClient();
  const remove = useRpcMutation("projects.remove");
  const favorite = useRpcMutation("projects.set_favorite");
  const launch = useRpcMutation("projects.launch");
  const summary = useRpcQuery("projects.summary", {}, { enabled: true, staleTime: 0 });
  const pending = remove.isPending || favorite.isPending || launch.isPending;
  const locked = useRef(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [column, setColumn] = useState(0);
  const [adding, setAdding] = useState(false);
  const [confirm, setConfirm] = useState<Project | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const normalized = query.trim().toLocaleLowerCase();
  const items = (list.data?.items ?? [])
    .filter((project) => !normalized || `${project.name}\n${project.path}`.toLocaleLowerCase().includes(normalized))
    .toSorted((a, b) => Number(b.favorite) - Number(a.favorite));
  const selected = Math.max(0, Math.min(active, items.length - 1));
  const [cellColumn, setCellColumn] = useState(0);
  const activeId = items.length ? `${listId}-${selected}-${cellColumn}` : undefined;
  useEffect(() => {
    const sync = () => {
      const count = document.getElementById(`${listId}-${selected}`)?.querySelectorAll('[role="gridcell"]').length ?? 1;
      setCellColumn(Math.min(column, count - 1));
    };
    sync();
    // Config responses can add/remove actions independently of the parent query.
    const grid = document.getElementById(listId);
    const observer = new MutationObserver(sync);
    if (grid) observer.observe(grid, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [column, selected, listId]);
  useEffect(() => {
    setActive(selected);
  }, [selected]);
  useEffect(() => {
    if (activeId) document.getElementById(activeId)?.scrollIntoView?.({ block: "nearest" });
  }, [activeId]);
  const pick = (index: number) => {
    const project = items[index];
    if (!project || locked.current) return;
    useCurrentProject.getState().set(project.id);
    onOpenChange(false);
  };
  const runLaunch = (project: Project) => {
    if (locked.current) return;
    locked.current = true;
    launch.mutate(
      { id: project.id },
      {
        onSuccess: (result) => {
          useTerminalLayout.getState().openLaunched(projectKey(project.id), result.terminalIds, result.rev);
          useCurrentProject.getState().set(project.id);
          useToasts
            .getState()
            .push({ tone: "ok", title: `터미널 ${result.terminals}개 · 에이전트 ${result.agents}개를 시작했습니다` });
          // Refresh errors cannot turn an acknowledged launch into a retry.
          void qc.invalidateQueries({ queryKey: ["terminal.list"] });
          void qc.invalidateQueries({ queryKey: ["agents.list"] });
          void qc.invalidateQueries({ queryKey: ["projects.summary"] });
          onOpenChange(false);
          onLaunched?.();
        },
        onSettled: () => {
          locked.current = false;
        },
      },
    );
  };
  const toggleFavorite = (project: Project) => {
    if (locked.current) return;
    locked.current = true;
    favorite.mutate(
      { id: project.id, favorite: !project.favorite },
      {
        onSuccess: ({ rev }) => {
          void invalidateIfNewer(qc, PROJECT_KEYS, rev);
        },
        onSettled: () => {
          locked.current = false;
        },
      },
    );
  };
  const removeConfirmed = () => {
    if (!confirm || locked.current) return;
    const id = confirm.id;
    locked.current = true;
    setConfirm(null);
    remove.mutate(
      { id },
      {
        onSuccess: ({ rev }) => {
          recordProjectRemoval(qc, id, rev);
          if (useCurrentProject.getState().id === id) useCurrentProject.getState().set(undefined);
          void invalidateIfNewer(qc, PROJECT_KEYS, rev);
        },
        onSettled: () => {
          locked.current = false;
        },
      },
    );
  };
  return (
    <>
      <Dialog
        open
        onOpenChange={(open) => {
          if (!locked.current) onOpenChange(open);
        }}
        title="프로젝트"
        width={620}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          input.current?.focus();
        }}
        footer={
          <Button variant="primary" disabled={pending} onClick={() => setAdding(true)}>
            프로젝트 추가
          </Button>
        }
      >
        {favorite.error && <ProjectError error={favorite.error} />}
        {launch.error && <ProjectError error={launch.error} />}
        {summary.error && <ProjectError error={summary.error} retry={() => void summary.refetch()} />}
        {remove.error && <ProjectError error={remove.error} />}
        {launch.isPending && <p role="status">시작 구성을 실행하는 중…</p>}
        {remove.isPending && <p role="status">프로젝트 제거 중…</p>}
        {list.isPending && <p role="status">프로젝트를 불러오는 중…</p>}
        {list.error && <ProjectError error={list.error} retry={() => void list.refetch()} />}
        <input
          ref={input}
          className={s.input}
          role="combobox"
          aria-expanded="true"
          aria-autocomplete="list"
          aria-haspopup="grid"
          aria-controls={listId}
          aria-activedescendant={activeId}
          aria-label="프로젝트 검색"
          value={query}
          placeholder="이름이나 경로"
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
            setColumn(0);
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActive(Math.max(0, Math.min(selected + (event.key === "ArrowDown" ? 1 : -1), items.length - 1)));
            }
            if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
              event.preventDefault();
              const count =
                document.getElementById(`${listId}-${selected}`)?.querySelectorAll('[role="gridcell"]').length ?? 1;
              setColumn(Math.max(0, Math.min(cellColumn + (event.key === "ArrowRight" ? 1 : -1), count - 1)));
            }
            if (event.key === "Enter") {
              event.preventDefault();
              if (cellColumn === 0) pick(selected);
              else if (!locked.current) {
                const button = document.getElementById(activeId ?? "")?.querySelector<HTMLButtonElement>("button");
                if (button?.getAttribute("aria-haspopup") === "menu")
                  button.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
                else button?.click();
              }
            }
          }}
        />
        <div
          id={listId}
          role="grid"
          aria-label="프로젝트 목록"
          className={s.list}
          onMouseDown={(event) => {
            // Composite popup focus stays in the combobox, including after a
            // mouse-opened confirmation closes, so arrow navigation continues.
            event.preventDefault();
            input.current?.focus();
          }}
        >
          {items.map((project, index) => (
            <ProjectCard
              key={project.id}
              project={project}
              summary={summary.data?.items.find((item) => item.id === project.id)}
              rowId={`${listId}-${index}`}
              selected={index === selected}
              column={cellColumn}
              pending={pending}
              pick={() => pick(index)}
              activate={() => setActive(index)}
              restoreFocus={() => {
                if (input.current?.isConnected) input.current.focus();
              }}
              remove={() => {
                if (!locked.current && confirmPolicy("projects.remove") === "always") setConfirm(project);
              }}
              favorite={() => toggleFavorite(project)}
              launch={() => runLaunch(project)}
            />
          ))}
        </div>
        {list.isSuccess && items.length === 0 && (
          <p>
            {query.trim() ? "일치하는 프로젝트가 없습니다." : "프로젝트가 없습니다. 프로젝트 추가로 폴더를 고르세요."}
          </p>
        )}
      </Dialog>
      <AddProjectDialog open={adding} onOpenChange={setAdding} />
      <ConfirmDialog
        open={confirm !== null}
        title="프로젝트 제거"
        body={`${confirm?.name ?? ""}을(를) devbox 목록에서만 지웁니다. 폴더와 파일은 그대로 둡니다.`}
        confirmLabel="프로젝트 제거"
        onCancel={() => {
          remove.reset();
          setConfirm(null);
        }}
        onConfirm={removeConfirmed}
      />
    </>
  );
}
