import type { LogSource } from "../../rpc/gen/rpc";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { openFileLocation } from "../../shell/fileLocation";
import { Button, EmptyState, ErrorState } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { LogView } from "./LogView";
import { usePinnedLogSources } from "./pinnedSources";

export function BottomLogs() {
  const projectId = useCurrentProject((state) => state.id);
  return projectId ? (
    <ProjectBottomLogs key={projectId} projectId={projectId} />
  ) : (
    <EmptyState title="프로젝트를 선택하세요" />
  );
}
function ProjectBottomLogs({ projectId }: { projectId: string }) {
  const client = useRpc();
  const pinned = usePinnedLogSources((state) =>
    Object.hasOwn(state.byProject, projectId) ? state.byProject[projectId] : undefined,
  );
  const recent = useRpcQuery("runtime.runs", { projectId, limit: 1 }, { enabled: !pinned?.length });
  useTopicInvalidation("runtime.changed", [["runtime.runs", { projectId, limit: 1 }]]);
  const latest = recent.data?.items[0];
  const sources: LogSource[] = pinned?.length ? pinned : latest ? [{ kind: "run", runId: latest.id }] : [];
  if (!pinned?.length && recent.isError)
    return (
      <ErrorState
        title="최근 실행을 불러오지 못했습니다"
        action={<Button onClick={() => void recent.refetch()}>다시 시도</Button>}
      />
    );
  if (!pinned?.length && recent.isPending) return <EmptyState title="최근 실행을 불러오는 중입니다" />;
  return (
    <div style={{ height: "100%", display: "flex", flexDirection: "column", minHeight: 0, overflow: "auto" }}>
      {pinned?.length ? (
        <Button size="sm" onClick={() => usePinnedLogSources.getState().clear(projectId)}>
          고정 해제
        </Button>
      ) : null}
      <LogView
        sources={sources}
        projectId={projectId}
        onPinSources={(sources) => usePinnedLogSources.getState().set(projectId, sources)}
        onOpenLocation={(location) => openFileLocation(client, projectId, location, "vscode")}
      />
    </div>
  );
}
