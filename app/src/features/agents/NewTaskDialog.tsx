import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button, Dialog, Select, TextArea } from "../../ui";
import { AddProjectAction, useNoRegisteredProjects } from "../projects/AddProjectAction";
import { isRegisteredProject, ProjectDataBoundary, useProjectData } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { useProjectTrust } from "../projects/useProjectTrust";
import { AGENT_KEYS } from "./AgentData";
import s from "./Agents.module.css";
import { BranchNameField, useBranchName } from "./BranchName";
import { rememberedProfile, rememberProfile } from "./lastProfile";
import { TemplateButtons } from "./TemplateButtons";
import { useAgentsUi } from "./ui";

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
function NewTaskForm({ open, onOpenChange, onCreated }: Props) {
  const uid = useId();
  const session = useRpcSession();
  const projects = useProjectData();
  const noProjects = useNoRegisteredProjects();
  const profiles = useRpcQuery("agents.profiles", {});
  const current = useCurrentProject((state) => state.id);
  // Read once: from here on the form is the user's.
  const [draft] = useState(() => useAgentsUi.getState().draft);
  const [projectId, setProjectId] = useState(draft?.projectId ?? current ?? "");
  // A draft's project that was removed since gives way to the usual default, once, as soon as the list is known.
  // A project removed after that is never replaced: the form asks for one, as for any project removed meanwhile.
  const [draftChecked, setDraftChecked] = useState(!draft?.projectId);
  if (!draftChecked && projects.data) {
    setDraftChecked(true);
    if (!projects.data.items.some((p) => p.id === projectId)) setProjectId(current ?? "");
  }
  // The tool picked in this dialog; until then the project's last tool is the default.
  const [picked, setPicked] = useState<string | null>(null);
  const [title, setTitle] = useState(draft?.title ?? ""),
    [baseBranch, setBaseBranch] = useState(draft?.baseBranch ?? "");
  const [worktree, setWorktree] = useState(draft?.worktree ?? true),
    [prompt, setPrompt] = useState(draft?.prompt ?? "");
  const project = projects.data?.items.find((p) => p.id === projectId);
  const options = profiles.data?.items ?? [];
  // A remembered tool that was removed since is ignored.
  const profile =
    options.find((p) => p.id === picked) ?? options.find((p) => p.id === rememberedProfile(project?.id)) ?? options[0];
  const prompts = useRpcQuery("projects.prompts", { id: project?.id ?? "" }, { enabled: !!project });
  const templates = prompts.data?.items ?? [];
  const create = useRpcMutation("agents.create");
  const trust = useProjectTrust(project?.id ?? "");
  const mounted = useRef(true);
  const target = useRef(project?.id);
  target.current = project?.id;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const qc = useQueryClient();
  const inFlight = useRef(false);
  const validTitle = title.trim().length > 0 && [...title.trim()].length <= 120 && !/[\x00-\x1f\x7f]/.test(title);
  const branch = useBranchName(title, worktree);
  const ready =
    session.state === "ready" &&
    !!project &&
    !!profile &&
    validTitle &&
    !branch.problem &&
    !projects.isError &&
    !profiles.isError;
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
      ...(branch.send === undefined ? {} : { branch: branch.send }),
    };
    try {
      if (
        !(await trust.ensure({
          id: params.projectId,
          operation: "create",
          agentId: null,
          worktrees: [],
          worktree: params.worktree,
        }))
      )
        return;
      if (!mounted.current || target.current !== params.projectId || !isRegisteredProject(qc, params.projectId)) return;
      const result = await create.mutateAsync(params);
      rememberProfile(params.projectId, params.profile);
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
  // [취소], Esc and a click outside: ignored while the create request is in flight, else they also drop a trust check.
  const close = (value: boolean) => {
    if (!create.isPending) {
      trust.cancel();
      onOpenChange(value);
    }
  };
  return (
    <>
      <Dialog
        open={open}
        onOpenChange={close}
        title="새 에이전트 작업"
        width={560}
        footer={
          <>
            <Button disabled={create.isPending} onClick={() => close(false)}>
              취소
            </Button>
            <Button
              variant="primary"
              disabled={!ready || create.isPending || trust.pending}
              onClick={() => void submit()}
            >
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
          <div className={s.field}>
            <label htmlFor={`${uid}-title`}>제목</label>
            <input id={`${uid}-title`} value={title} onChange={(event) => setTitle(event.target.value)} required />
          </div>
          <div className={s.pair}>
            <div className={s.field}>
              <label htmlFor={`${uid}-project`}>프로젝트</label>
              <Select
                id={`${uid}-project`}
                className={s.control}
                label="프로젝트"
                value={project?.id ?? ""}
                onChange={setProjectId}
                disabled={create.isPending}
                options={(projects.data?.items ?? []).map((p) => ({ value: p.id, label: p.name }))}
              />
            </div>
            <div className={s.field}>
              <label htmlFor={`${uid}-tool`}>도구</label>
              <Select
                id={`${uid}-tool`}
                className={s.control}
                label="도구"
                value={profile?.id ?? ""}
                onChange={setPicked}
                disabled={create.isPending}
                options={options.map((p) => ({ value: p.id, label: p.label }))}
              />
            </div>
          </div>
          {noProjects && (
            <>
              <p>에이전트 작업을 만들 프로젝트를 등록해 주세요.</p>
              <AddProjectAction onAdded={setProjectId} />
            </>
          )}
          {projects.data && !project && projects.data.items.length > 0 && (
            <p>에이전트 작업을 만들 프로젝트를 선택해 주세요.</p>
          )}
          {profiles.data?.items.length === 0 && <p>사용할 도구가 없습니다. 설정에서 에이전트 도구를 확인해 주세요.</p>}
          <div className={s.field}>
            <label htmlFor={`${uid}-base`}>기준 브랜치</label>
            <input
              id={`${uid}-base`}
              placeholder="비우면 현재 브랜치"
              value={baseBranch}
              onChange={(event) => setBaseBranch(event.target.value)}
            />
          </div>
          <label className={s.check}>
            <input type="checkbox" checked={worktree} onChange={(event) => setWorktree(event.target.checked)} />
            작업 폴더 만들기
          </label>
          {worktree ? <BranchNameField branch={branch} /> : <p>현재 체크아웃에서 실행</p>}
          <TemplateButtons
            templates={templates}
            disabled={!project || create.isPending}
            title={title}
            baseBranch={baseBranch}
            branch={branch.send}
            project={project?.name ?? ""}
            onFill={setPrompt}
          />
          {draft?.truncated && <p role="status">노트 내용이 길어 처음 64 KiB만 지시문에 넣었습니다.</p>}
          <div className={s.field}>
            <label htmlFor={`${uid}-prompt`}>지시문</label>
            <TextArea
              id={`${uid}-prompt`}
              className={s.control}
              aria-describedby={`${uid}-prompt-help`}
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              maxRows={10}
            />
            <p id={`${uid}-prompt-help`} className={s.muted}>
              Ctrl+Enter로 시작
            </p>
          </div>
          {(!projects.data || !profiles.data) && !queryError && <p role="status">프로젝트와 도구를 불러오는 중…</p>}
          {trust.notice && <p role="alert">{trust.notice}</p>}
          {message && (
            <p role="alert">
              {message.title} {message.action}
            </p>
          )}
        </form>
      </Dialog>
      {trust.dialog}
    </>
  );
}
