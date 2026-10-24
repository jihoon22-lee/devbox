/** 도구 섹션: 목록(`ToolsList`) + 본문(API 작업 공간·실행기·가져오기). 실제 데몬 연결은 `ToolsPage`가 넣는다. */
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { useRpc } from "../../rpc/RpcProvider";
import { SectionLayout } from "../../shell/SectionLayout";
import { EmptyState } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { ApiWorkspace } from "./api/ApiWorkspace";
import { ApiBackendProvider, useApiBackend } from "./api/backend";
import { GrpcPanel } from "./api/grpc/GrpcPanel";
import { ImportDialog } from "./api/ImportDialog";
import { RunnerView } from "./api/RunnerView";
import { SsePanel } from "./api/realtime/SsePanel";
import { WsPanel } from "./api/realtime/WsPanel";
import { rpcBackend } from "./api/rpcBackend";
import { useApiTabs } from "./api/store";
import { bumpCollections } from "./api/useCollections";
import { type ToolsSearch, toolsSearch } from "./search";
import { rootOf, ToolsList } from "./ToolsList";

const LATER: Record<string, string> = {
  webhooks: "웹훅",
  transforms: "변환 도구",
  env: "개발 환경 점검",
};

export function ToolsView({
  search,
  projectId,
  navigate,
}: {
  search: ToolsSearch;
  projectId: string | undefined;
  navigate: (search: ToolsSearch) => void;
}) {
  const backend = useApiBackend();
  const view = search.view ?? "api";
  const roots = [
    ...(projectId ? [{ id: projectId, label: "프로젝트 컬렉션", root: { kind: "project" as const, projectId } }] : []),
    { id: "personal", label: "개인 컬렉션", root: { kind: "personal" as const } },
  ];
  const { root, path } = search;
  // 검색값의 경로는 그 파일을 탭으로 연다(명령 팔레트·알림에서 온 이동).
  useEffect(() => {
    if (!path) return;
    let live = true;
    const target = rootOf(root);
    backend.read(target, path).then(
      (file) => live && useApiTabs.getState().openFile(target, path, file.text, file.digest, file.reviewed),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [backend, root, path]);
  const body =
    view !== "api" ? (
      <EmptyState title={LATER[view] ?? "도구"} body="이 도구는 다음 단계에서 이 자리에 연결됩니다." />
    ) : search.tool === "runner" ? (
      <RunnerView root={rootOf(search.root)} />
    ) : search.tool === "websocket" ? (
      <WsPanel />
    ) : search.tool === "sse" ? (
      <SsePanel />
    ) : search.tool === "grpc" ? (
      <GrpcPanel roots={roots} />
    ) : (
      <ApiWorkspace />
    );
  return (
    <SectionLayout list={<ToolsList projectId={projectId} navigate={navigate} />}>
      {body}
      <ImportDialog
        open={view === "api" && search.tool === "import"}
        onOpenChange={(open) => !open && navigate({ view: "api" })}
        roots={roots}
      />
    </SectionLayout>
  );
}

export function ToolsPage() {
  const search = toolsSearch(useSearch({ strict: false }));
  const projectId = useCurrentProject((state) => state.id) ?? undefined;
  const client = useRpc();
  const backend = useMemo(() => rpcBackend(client), [client]);
  const navigate = useNavigate();
  useEffect(() => client.subscribe("api.collections_changed", bumpCollections, bumpCollections), [client]);
  return (
    <ApiBackendProvider backend={backend}>
      <ToolsView
        search={search}
        projectId={projectId}
        navigate={(next) => void navigate({ to: "/tools", search: next })}
      />
    </ApiBackendProvider>
  );
}
