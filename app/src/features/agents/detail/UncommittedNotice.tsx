import { useRpcQuery } from "../../../rpc/query";
import { usePageVisible } from "../../../shell/visibility";
import { Banner, Button } from "../../../ui";
import type { AgentTask } from "../model";
import { useDrafts } from "./drafts";

type Props = { task: AgentTask; onCompose?: () => void };

/**
 * Merge and PR refuse a task folder with uncommitted changes; this says so before their confirmation
 * does. Changes in the middle of a turn are normal, so a running or preparing task shows nothing.
 * A folder in a merge or rebase stopped on conflicts shows nothing either: finishing it comes first,
 * and AgentActions leaves the task's own rebase to its conflict banner.
 */
export function UncommittedNotice({ task, onCompose }: Props) {
  const shown = !!task.worktreePath && !["closed", "running", "preparing"].includes(task.state);
  // A turn that ends, or any other change to the task, starts a new reader: it reads the folder at
  // once and shows nothing until that read answers, since the earlier count may no longer be true.
  return shown ? <FolderChanges key={`${task.id}:${task.activitySeq}`} task={task} onCompose={onCompose} /> : null;
}

function FolderChanges({ task, onCompose }: Props) {
  const visible = usePageVisible();
  // The task's own folder, which the daemon finds from the task, read like the status bar's Git chip.
  const status = useRpcQuery(
    "git.status",
    { target: { projectId: task.projectId, worktreeId: task.id } },
    { enabled: visible, staleTime: 0, refetchInterval: visible ? 15000 : false },
  );
  if (!status.isFetchedAfterMount) return null;
  if (status.error) return <Banner tone="warn">변경을 확인하지 못했습니다</Banner>;
  if (status.data?.summary.conflicted.length) return null;
  const count = status.data?.files.length ?? 0;
  if (!count) return null;
  return (
    <Banner
      tone="warn"
      actions={
        <Button
          onClick={() => {
            // Only a draft: the user reads it and sends it.
            useDrafts
              .getState()
              .appendOnce(
                task.id,
                `작업 폴더에 커밋하지 않은 변경이 ${count}개 있습니다. 변경을 확인하고 알맞은 메시지로 모두 커밋해 주세요.`,
              );
            onCompose?.();
          }}
        >
          커밋 지시 작성
        </Button>
      }
    >
      {`커밋하지 않은 변경 ${count}개가 있어 병합·PR을 진행할 수 없습니다.`}
    </Banner>
  );
}
