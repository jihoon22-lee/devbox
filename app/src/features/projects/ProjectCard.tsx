import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, MoreHorizontal, Star, Trash2 } from "lucide-react";
import type { Project, ProjectSummary } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { Button, Menu, Tooltip, TooltipProvider, useToasts } from "../../ui";
import { ProjectError } from "./ProjectError";
import s from "./Projects.module.css";

export function ProjectCard({
  project,
  summary,
  rowId,
  selected,
  column,
  pending,
  pick,
  remove,
  favorite,
  launch,
  activate,
  restoreFocus,
}: {
  project: Project;
  summary?: ProjectSummary;
  rowId: string;
  selected: boolean;
  column: number;
  pending: boolean;
  pick: () => void;
  remove: () => void;
  favorite: () => void;
  launch: () => void;
  activate: () => void;
  restoreFocus: () => void;
}) {
  const config = useRpcQuery("projects.config", { id: project.id }, { enabled: true, staleTime: 30_000 });
  const profile = config.data?.config.launch;
  // The daemon returns an empty default launch object for an absent [launch].
  const launchable =
    !config.data?.error &&
    profile &&
    (profile.terminals.length > 0 || profile.agents.length > 0 || profile.services.length > 0);
  const native = "__TAURI_INTERNALS__" in window;
  const external = async (kind: "vscode" | "explorer" | "terminal") => {
    try {
      await invoke("open_external", { kind, path: project.path });
    } catch {
      useToasts
        .getState()
        .push({ tone: "danger", title: "외부 앱을 열지 못했습니다", body: "연결 상태와 앱 설치를 확인해 주세요." });
    }
  };
  const cell = (index: number) => ({
    role: "gridcell",
    id: `${rowId}-${index}`,
    "data-active": selected && column === index,
  });
  return (
    <div id={rowId} className={s.row} role="row" aria-selected={selected} onMouseEnter={activate}>
      <div
        {...cell(0)}
        className={s.nameCell}
        aria-description={config.data?.error?.message ?? summary?.error?.message}
      >
        <button type="button" tabIndex={-1} className={s.pick} disabled={pending} onClick={pick}>
          <span>{project.name}</span>
          <span className={s.path}>{project.path}</span>
          {summary && (
            <span className={s.summary}>
              {summary.branch && <span>{summary.branch}</span>}
              {summary.dirty !== null && <span>{summary.dirty ? "변경 있음" : "변경 없음"}</span>}
              {summary.ahead !== null && <span>앞섬 {summary.ahead}</span>}
              {summary.behind !== null && <span>뒤처짐 {summary.behind}</span>}
              <span>터미널 {summary.terminals}개</span>
              <span>
                에이전트 {summary.agents.open} · 입력 대기 {summary.agents.attention}
              </span>
            </span>
          )}
        </button>
        {(config.data?.error || summary?.error) && (
          <TooltipProvider>
            <Tooltip content={config.data?.error?.message ?? summary?.error?.message}>
              <span
                className={s.warning}
                tabIndex={-1}
                aria-label={`${project.name} ${config.data?.error ? "설정" : "Git 상태"} 오류`}
              >
                <AlertTriangle size={14} />
              </span>
            </Tooltip>
          </TooltipProvider>
        )}
        {config.error && <ProjectError error={config.error} retry={() => void config.refetch()} />}
      </div>
      <div {...cell(1)} aria-label={`${project.name} 제거`} className={s.actionCell}>
        <Button
          tabIndex={-1}
          variant="ghost"
          size="sm"
          disabled={pending}
          aria-label={`${project.name} 제거`}
          onClick={remove}
        >
          <Trash2 size={14} />
        </Button>
      </div>
      <div {...cell(2)} aria-label={`${project.name} 즐겨찾기`} className={s.actionCell}>
        <Button
          tabIndex={-1}
          variant="ghost"
          size="sm"
          disabled={pending}
          aria-pressed={project.favorite}
          aria-label={`${project.name} 즐겨찾기`}
          onClick={favorite}
        >
          <Star size={14} fill={project.favorite ? "currentColor" : "none"} />
        </Button>
      </div>
      {launchable && (
        <div {...cell(3)} className={s.actionCell}>
          <Button
            tabIndex={-1}
            size="sm"
            disabled={pending}
            aria-label={`${project.name} 시작 구성 실행`}
            onClick={launch}
          >
            시작 구성 실행
          </Button>
        </div>
      )}
      {native && (
        <div {...cell(launchable ? 4 : 3)} className={s.actionCell}>
          <Menu
            onCloseFocus={restoreFocus}
            trigger={
              <Button
                tabIndex={-1}
                variant="ghost"
                size="sm"
                disabled={pending}
                aria-label={`${project.name} 외부 앱으로 열기`}
              >
                <MoreHorizontal size={14} />
              </Button>
            }
            items={[
              { label: "VS Code로 열기", onSelect: () => void external("vscode") },
              { label: "탐색기로 열기", onSelect: () => void external("explorer") },
              { label: "Windows Terminal로 열기", onSelect: () => void external("terminal") },
            ]}
          />
        </div>
      )}
    </div>
  );
}
