import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { Project } from "../../rpc/gen/rpc";
import { confirmPolicy, invalidateIfNewer, useRpcMutation } from "../../rpc/query";
import { Button, ConfirmDialog, Dialog } from "../../ui";
import { AddProjectDialog } from "./AddProjectDialog";
import { PROJECT_KEYS, ProjectDataBoundary, recordProjectRemoval, useProjectData } from "./ProjectData";
import { ProjectError } from "./ProjectError";
import { useCurrentProject } from "./store";
import s from "./Projects.module.css";
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
export function ProjectSwitcher({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return open ? (
    <ProjectDataBoundary>
      <Switcher onOpenChange={onOpenChange} />
    </ProjectDataBoundary>
  ) : null;
}
function Switcher({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  const list = useProjectData();
  const qc = useQueryClient();
  const remove = useRpcMutation("projects.remove");
  const locked = useRef(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [column, setColumn] = useState(0);
  const [adding, setAdding] = useState(false);
  const [confirm, setConfirm] = useState<Project | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const normalized = query.trim().toLocaleLowerCase();
  const items = (list.data?.items ?? []).filter(
    (project) => !normalized || `${project.name}\n${project.path}`.toLocaleLowerCase().includes(normalized),
  );
  const selected = Math.max(0, Math.min(active, items.length - 1));
  const activeId = items.length ? `${listId}-${selected}-${column}` : undefined;
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
          <Button variant="primary" disabled={remove.isPending} onClick={() => setAdding(true)}>
            프로젝트 추가
          </Button>
        }
      >
        {remove.error && <ProjectError error={remove.error} />}
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
              setColumn(event.key === "ArrowRight" ? 1 : 0);
            }
            if (event.key === "Enter") {
              event.preventDefault();
              if (column === 0) pick(selected);
              else if (items[selected] && !locked.current && confirmPolicy("projects.remove") === "always")
                setConfirm(items[selected]);
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
            <div
              id={`${listId}-${index}`}
              className={s.row}
              key={project.id}
              role="row"
              aria-selected={index === selected}
              onMouseEnter={() => setActive(index)}
            >
              <div
                role="gridcell"
                id={`${listId}-${index}-0`}
                className={s.nameCell}
                data-active={index === selected && column === 0}
              >
                <button
                  type="button"
                  tabIndex={-1}
                  className={s.pick}
                  disabled={remove.isPending}
                  onClick={() => pick(index)}
                >
                  <span>{project.name}</span>
                  <span className={s.path}>{project.path}</span>
                </button>
              </div>
              <div
                role="gridcell"
                id={`${listId}-${index}-1`}
                aria-label={`${project.name} 제거`}
                data-active={index === selected && column === 1}
                className={s.actionCell}
              >
                <Button
                  tabIndex={-1}
                  variant="ghost"
                  size="sm"
                  disabled={remove.isPending}
                  aria-label={`${project.name} 제거`}
                  onClick={() => {
                    if (confirmPolicy("projects.remove") === "always") setConfirm(project);
                  }}
                >
                  <Trash2 size={14} />
                </Button>
              </div>
            </div>
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
        onCancel={() => setConfirm(null)}
        onConfirm={removeConfirmed}
      />
    </>
  );
}
