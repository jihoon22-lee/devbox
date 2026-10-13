import { useQueryClient } from "@tanstack/react-query";
import { useId, useRef, useState } from "react";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button, Dialog, Select, TextArea } from "../../ui";
import { ProjectDataBoundary, useProjectData } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { AGENT_KEYS } from "./AgentData";
import s from "./Agents.module.css";
import { fillTemplate, previewBranch } from "./model";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (id: string) => void;
}
export function NewTaskDialog(props: Props) {
  return props.open ? (
    <ProjectDataBoundary>
      <NewTaskForm {...props} />
    </ProjectDataBoundary>
  ) : null;
}
function lastProfile() {
  try {
    return localStorage.getItem("devbox.lastProfile") ?? "";
  } catch {
    return "";
  }
}
function NewTaskForm({ open, onOpenChange, onCreated }: Props) {
  const uid = useId();
  const session = useRpcSession();
  const projects = useProjectData();
  const profiles = useRpcQuery("agents.profiles", {});
  const current = useCurrentProject((state) => state.id);
  const [projectId, setProjectId] = useState(current ?? "");
  const [chosenProfile, setProfile] = useState(lastProfile);
  const [title, setTitle] = useState(""),
    [baseBranch, setBaseBranch] = useState("");
  const [worktree, setWorktree] = useState(true),
    [prompt, setPrompt] = useState("");
  const project = projects.data?.items.find((p) => p.id === projectId);
  const options = profiles.data?.items ?? [];
  const profile = options.find((p) => p.id === chosenProfile) ?? options[0];
  const prompts = useRpcQuery("projects.prompts", { id: project?.id ?? "" }, { enabled: !!project });
  const create = useRpcMutation("agents.create");
  const qc = useQueryClient();
  const inFlight = useRef(false);
  const validTitle = title.trim().length > 0 && [...title.trim()].length <= 120 && !/[\x00-\x1f\x7f]/.test(title);
  const ready =
    session.state === "ready" && !!project && !!profile && validTitle && !projects.isError && !profiles.isError;
  const submit = async () => {
    if (inFlight.current || !ready || !project || !profile) return;
    inFlight.current = true;
    const params = {
      projectId: project.id,
      title: title.trim(),
      profile: profile.id,
      baseBranch: baseBranch.trim() || null,
      worktree,
      prompt,
    };
    try {
      const result = await create.mutateAsync(params);
      try {
        localStorage.setItem("devbox.lastProfile", params.profile);
      } catch {
        /* Preference storage is optional. */
      }
      // A mutation result has no list revision. Cache only its own detail, then
      // refresh the authoritative list without treating refresh failure as create failure.
      qc.setQueryData(["agents.get", { id: result.id }], result);
      for (const queryKey of AGENT_KEYS) void qc.invalidateQueries({ queryKey }).catch(() => {});
      onCreated(result.id);
      onOpenChange(false);
    } catch {
      /* The typed mutation error is rendered below; identical retries keep RPC identity. */
    } finally {
      inFlight.current = false;
    }
  };
  const queryError = projects.error ?? profiles.error ?? prompts.error;
  const error = create.error ?? queryError;
  const message = error && messageFor(error.code, error.detail);
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!inFlight.current) onOpenChange(value);
      }}
      title="새 에이전트 작업"
      width={560}
      footer={
        <>
          <Button
            disabled={create.isPending}
            onClick={() => {
              if (!inFlight.current) onOpenChange(false);
            }}
          >
            취소
          </Button>
          <Button variant="primary" disabled={!ready || create.isPending} onClick={() => void submit()}>
            {create.isPending ? "시작 중…" : "시작"}
          </Button>
        </>
      }
    >
      <form
        className={s.form}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
        onKeyDown={(event) => {
          if (
            event.key === "Enter" &&
            event.ctrlKey &&
            !event.defaultPrevented &&
            !event.nativeEvent.isComposing &&
            event.keyCode !== 229 &&
            !(
              event.target instanceof Element &&
              event.target.closest('[role="option"], [role="listbox"], [role="menu"], [role="combobox"]')
            )
          ) {
            event.preventDefault();
            if (!event.repeat) void submit();
          }
        }}
      >
        <label htmlFor={`${uid}-title`}>제목</label>
        <input id={`${uid}-title`} value={title} onChange={(event) => setTitle(event.target.value)} required />
        <Select
          label="프로젝트"
          value={project?.id ?? ""}
          onChange={setProjectId}
          disabled={create.isPending}
          options={(projects.data?.items ?? []).map((p) => ({ value: p.id, label: p.name }))}
        />
        {projects.data?.items.length === 0 && <p>에이전트 작업을 만들 프로젝트를 등록해 주세요.</p>}
        {projects.data && !project && projects.data.items.length > 0 && (
          <p>에이전트 작업을 만들 프로젝트를 선택해 주세요.</p>
        )}
        <Select
          label="도구"
          value={profile?.id ?? ""}
          onChange={setProfile}
          disabled={create.isPending}
          options={options.map((p) => ({ value: p.id, label: p.label }))}
        />
        {profiles.data?.items.length === 0 && <p>사용할 도구가 없습니다. 설정에서 에이전트 도구를 확인해 주세요.</p>}
        <label htmlFor={`${uid}-base`}>기준 브랜치</label>
        <input
          id={`${uid}-base`}
          placeholder="비우면 현재 브랜치"
          value={baseBranch}
          onChange={(event) => setBaseBranch(event.target.value)}
        />
        <label className={s.check}>
          <input type="checkbox" checked={worktree} onChange={(event) => setWorktree(event.target.checked)} />
          작업 폴더 만들기
        </label>
        {!worktree && <p>현재 체크아웃에서 실행</p>}
        <div className={s.templates}>
          {(prompts.data?.items ?? []).map((template) => (
            <Button
              key={template.name}
              size="sm"
              disabled={!project || create.isPending}
              onClick={() =>
                setPrompt(
                  fillTemplate(template.body, {
                    title: title.trim(),
                    branch: baseBranch.trim() || previewBranch(title.trim(), Date.now()),
                    project: project?.name ?? "",
                  }),
                )
              }
            >
              템플릿: {template.name}
            </Button>
          ))}
        </div>
        <label htmlFor={`${uid}-prompt`}>지시문</label>
        <TextArea
          id={`${uid}-prompt`}
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          maxRows={10}
        />
        <p className={s.muted}>템플릿의 예상 브랜치는 이름이 겹치면 바뀔 수 있습니다. Ctrl+Enter로 시작합니다.</p>
        {(!projects.data || !profiles.data) && !queryError && <p role="status">프로젝트와 도구를 불러오는 중…</p>}
        {message && (
          <p role="alert">
            {message.title} {message.action}
          </p>
        )}
      </form>
    </Dialog>
  );
}
