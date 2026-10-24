import { useNavigate } from "@tanstack/react-router";
import { useRpcQuery } from "../../rpc/query";
import { usePageVisible } from "../../shell/visibility";
import { useCurrentProject } from "../projects/store";
export function GitStatus() {
  const id = useCurrentProject((state) => state.id);
  return id ? <ProjectGitStatus key={id} projectId={id} /> : null;
}
function ProjectGitStatus({ projectId }: { projectId: string }) {
  const navigate = useNavigate();
  const visible = usePageVisible();
  const status = useRpcQuery(
    "git.status",
    { target: { projectId } },
    { enabled: visible, staleTime: 0, refetchInterval: visible ? 15000 : false },
  );
  const summary = status.data?.summary;
  return (
    <button
      type="button"
      style={{ background: "transparent", border: 0, color: "inherit", cursor: "pointer" }}
      onClick={() => void navigate({ to: "/source" })}
      aria-label="Git 상태 · 소스로 이동"
    >
      Git{" "}
      {status.error
        ? "조회 실패"
        : summary
          ? `${summary.branch ?? "분리된 HEAD"} ↑${summary.ahead} ↓${summary.behind} · 변경 ${status.data!.files.length}`
          : "불러오는 중…"}
    </button>
  );
}
