import { useRef, useState } from "react";
import { RpcError } from "../../../rpc/client";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { Banner, Button, Menu, Tooltip } from "../../../ui";
import { useProjectTrust } from "../../projects/useProjectTrust";
import { openPopout } from "../../terminal/popout";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";
import { useAgentRefresh, useAgentResult } from "./data";
import { QueryError } from "./QueryError";
import { type EditIntent, ReviewDialogs } from "./ReviewDialogs";
import { useReviewAction } from "./useReviewAction";
export function AgentActions({
  task,
  sessionAlive,
  sessionKnown = true,
  onInstruct,
  onResume,
  onTest,
}: {
  task: AgentTask;
  sessionAlive: boolean;
  sessionKnown?: boolean;
  onInstruct?: () => void;
  onResume?: () => void;
  onTest?: (id: string) => void;
}) {
  const resume = useRpcMutation("agents.resume"),
    review = useRpcMutation("agents.review"),
    rebase = useRpcMutation("agents.rebase"),
    abort = useRpcMutation("agents.rebase_abort"),
    send = useRpcMutation("agents.send"),
    test = useRpcMutation("agents.test");
  const trust = useProjectTrust(`${task.id}:${task.projectId}:${task.state}`);
  const config = useRpcQuery("projects.config", { id: task.projectId }, { enabled: task.state !== "closed" });
  const commits = useRpcQuery("agents.commits", { id: task.id }, { enabled: false });
  const rebaseStatus = useRpcQuery(
    "agents.rebase_status",
    { id: task.id },
    {
      enabled: !!task.worktreePath && task.state !== "closed",
      staleTime: 0,
      refetchOnWindowFocus: "always",
    },
  );
  const apply = useAgentResult(),
    refresh = useAgentRefresh();
  const { state } = useRpcSession();
  const [preflightConflict, setConflict] = useState<string[]>();
  const rebaseConflict =
    rebaseStatus.isFetchedAfterMount && !rebaseStatus.isFetching && !rebaseStatus.error && rebaseStatus.data?.owned;
  const conflict = rebaseConflict ? rebaseStatus.data?.files : preflightConflict;
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState("");
  const [edit, setEdit] = useState<EditIntent>();
  const guard = useRef(false);
  const destructive = useReviewAction(task, (files) => {
    setConflict(files);
  });
  const busy =
    resume.isPending ||
    review.isPending ||
    rebase.isPending ||
    abort.isPending ||
    send.isPending ||
    test.isPending ||
    destructive.busy;
  const blocked = preparing || busy || trust.pending || state !== "ready";
  const run = async (operation: () => Promise<void>) => {
    if (guard.current || blocked) return;
    guard.current = true;
    setPreparing(true);
    setError("");
    try {
      await operation();
    } catch (error) {
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = messageFor(failure.code ?? "internal", failure.detail);
      setError(
        `${message.title} ${message.action ?? ""}${failure.diagnosticId ? ` · 진단 ${failure.diagnosticId}` : ""}`,
      );
    } finally {
      guard.current = false;
      setPreparing(false);
    }
  };
  const reflect = () =>
    run(async () => {
      await rebase.mutateAsync({ id: task.id });
      setConflict(undefined);
      refresh();
    });
  const runTest = () =>
    run(async () => {
      if (
        !(await trust.ensure({
          id: task.projectId,
          operation: "test",
          agentId: task.id,
          worktrees: [],
          worktree: null,
        }))
      )
        return;
      const result = await test.mutateAsync({ id: task.id });
      onTest?.(result.runId);
    });
  const createPr = () =>
    run(async () => {
      const result = await commits.refetch();
      if (result.error) throw result.error;
      setEdit({
        kind: "pr",
        body: `${task.prompt}\n\n${(result.data?.items ?? []).map((commit) => `- ${commit.sha.slice(0, 8)} ${commit.subject}`).join("\n")}`,
      });
    });
  const hasTest = !!config.data?.config.agent.test && !config.data.error;
  if (task.state === "closed") return null;
  return (
    <>
      <div className={s.actions}>
        {sessionAlive && !task.toolExited ? (
          <Button disabled={blocked} onClick={onInstruct}>
            이어서 지시
          </Button>
        ) : (
          <Button
            disabled={blocked || !sessionKnown || task.state === "preparing"}
            onClick={() =>
              void run(async () => {
                apply(await resume.mutateAsync({ id: task.id }));
                onResume?.();
              })
            }
          >
            {sessionKnown ? "재개" : "세션 확인 중…"}
          </Button>
        )}
        {task.worktreePath && (
          <>
            <Button disabled={blocked} onClick={() => void reflect()}>
              기준 반영
            </Button>
            <Menu
              trigger={<Button disabled={blocked}>병합</Button>}
              items={[
                {
                  label: "병합 커밋",
                  afterClose: true,
                  onSelect: () => void destructive.open({ kind: "merge", strategy: "merge", message: null }),
                },
                { label: "커밋 하나로 합쳐 병합", afterClose: true, onSelect: () => setEdit({ kind: "squash" }) },
                {
                  label: "기준 위로 옮긴 뒤 병합",
                  afterClose: true,
                  onSelect: () => void destructive.open({ kind: "merge", strategy: "rebase", message: null }),
                },
              ]}
            />
            <Button disabled={blocked} onClick={() => void createPr()}>
              PR
            </Button>
          </>
        )}
        <Menu
          trigger={<Button disabled={blocked}>더 보기</Button>}
          items={[
            { label: "테스트 실행", disabled: !hasTest, onSelect: () => void runTest() },
            {
              label: "검토 시작",
              disabled: !["running", "waiting", "idle"].includes(task.state),
              onSelect: () =>
                void run(async () => {
                  apply(await review.mutateAsync({ id: task.id }));
                }),
            },
            {
              label: "새 창으로",
              onSelect: () =>
                void run(async () => {
                  await openPopout("agent", task.id);
                }),
            },
            {
              label: task.worktreePath ? "폐기" : "세션 끝내기",
              danger: true,
              afterClose: true,
              onSelect: () => void destructive.open({ kind: "discard" }),
            },
          ]}
        />
        {!hasTest && (
          <Tooltip content=".devbox/devbox.toml의 [agent] test를 설정하세요">
            <span>테스트 설정 필요</span>
          </Tooltip>
        )}
      </div>
      {trust.dialog}
      {trust.notice && <Banner tone="danger">{trust.notice}</Banner>}
      {error && <Banner tone="danger">{error}</Banner>}
      <QueryError query={rebaseStatus} />
      {conflict && (
        <Banner
          tone="warn"
          actions={
            rebaseConflict ? (
              <>
                <Button
                  disabled={blocked || !sessionAlive || task.toolExited}
                  onClick={() =>
                    void run(async () => {
                      apply(
                        await send.mutateAsync({
                          id: task.id,
                          text: `기준 브랜치 위로 rebase하다 충돌이 났습니다. 충돌 파일: ${conflict.join(", ")}. 충돌을 해결하고 git rebase --continue까지 마쳐 주세요.`,
                        }),
                      );
                      setConflict(undefined);
                    })
                  }
                >
                  해결 지시 보내기
                </Button>
                <Button
                  disabled={blocked}
                  onClick={() =>
                    void run(async () => {
                      apply(await abort.mutateAsync({ id: task.id }));
                      setConflict(undefined);
                    })
                  }
                >
                  되돌리기
                </Button>
              </>
            ) : (
              <Button disabled={blocked} onClick={() => void reflect()}>
                기준 반영 실행
              </Button>
            )
          }
        >
          충돌 파일 {conflict.length}개 · {conflict.join(", ")} ·{" "}
          {rebaseConflict && (!sessionAlive || task.toolExited)
            ? "에이전트를 정지했습니다. 작업 폴더에서 충돌을 해결하고 git rebase --continue를 마친 뒤 재개하거나, 되돌리기를 선택하세요."
            : "변경 내용을 확인해 충돌을 해결하세요."}
        </Banner>
      )}
      <ReviewDialogs task={task} edit={edit} setEdit={setEdit} review={destructive} />
    </>
  );
}
