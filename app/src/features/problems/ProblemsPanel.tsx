import { useState } from "react";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { openFileLocation } from "../../shell/fileLocation";
import { Banner, Button, EmptyState } from "../../ui";
import { ProjectError } from "../projects/ProjectError";
import { useCurrentProject } from "../projects/store";

export function ProblemsPanel() {
  const projectId = useCurrentProject((state) => state.id);
  return projectId ? (
    <ProjectProblems key={projectId} projectId={projectId} />
  ) : (
    <EmptyState title="프로젝트를 선택하세요" body="현재 프로젝트의 실행에서 발견된 문제를 보여 줍니다." />
  );
}
function ProjectProblems({ projectId }: { projectId: string }) {
  const client = useRpc();
  const problems = useRpcQuery("observe.problems", { projectId, limit: 100 });
  useTopicInvalidation("observe.problems.changed", [["observe.problems", { projectId, limit: 100 }]]);
  const [editor, setEditor] = useState<"vscode" | "cursor">("vscode");
  const [error, setError] = useState<string>();
  return (
    <section aria-label="실행 문제" style={{ overflow: "auto", height: "100%", padding: "var(--space-3)" }}>
      <label>
        문제 파일 편집기{" "}
        <select
          aria-label="문제 파일 편집기"
          value={editor}
          onChange={(event) => setEditor(event.target.value as "vscode" | "cursor")}
        >
          <option value="vscode">VS Code</option>
          <option value="cursor">Cursor</option>
        </select>
      </label>
      {problems.isError && <ProjectError error={problems.error} retry={() => void problems.refetch()} />}
      {error && <Banner tone="danger">{error}</Banner>}
      {problems.isPending && <p role="status">문제 불러오는 중…</p>}
      {problems.data?.truncated && <Banner tone="warn">보관 한도를 넘어 일부 문제가 누락되었습니다.</Banner>}
      {problems.data?.items.length === 0 && <p>발견된 문제가 없습니다.</p>}
      <ul>
        {problems.data?.items.map((problem) => {
          const runId = problem.source.startsWith("run:") ? problem.source.slice(4) : undefined;
          return (
            <li key={JSON.stringify(problem)}>
              <span>{problem.severity === "error" ? "오류" : problem.severity === "warning" ? "경고" : "정보"} </span>
              <a
                href="#"
                onClick={(event) => {
                  event.preventDefault();
                  setError(undefined);
                  if (!runId) {
                    setError("문제의 원본 실행을 찾을 수 없습니다.");
                    return;
                  }
                  void openFileLocation(
                    client,
                    projectId,
                    {
                      path: problem.file,
                      line: problem.line,
                      column: problem.col ?? undefined,
                      source: { kind: "run", runId },
                    },
                    editor,
                  ).catch(() => setError("파일을 열지 못했습니다. 원본 파일과 외부 편집기 설치를 확인하세요."));
                }}
              >
                {problem.file}:{problem.line}
                {problem.col ? `:${problem.col}` : ""}
              </a>{" "}
              <span>{problem.message}</span>
            </li>
          );
        })}
      </ul>
      {problems.data && (
        <Button size="sm" onClick={() => void problems.refetch()}>
          새로고침
        </Button>
      )}
    </section>
  );
}
