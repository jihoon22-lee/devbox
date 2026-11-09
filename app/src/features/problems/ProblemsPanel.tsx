import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { DocKey } from "../../rpc/gen/rpc";
import { messageText } from "../../rpc/messages";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { openFileLocation } from "../../shell/fileLocation";
import { Banner, Button } from "../../ui";
import { NoProjectState } from "../projects/AddProjectAction";
import { ProjectError } from "../projects/ProjectError";
import { useCurrentProject } from "../projects/store";
import type { CodeLocation } from "../source/editor/location";
import { resolveSourceLocation } from "../source/openLocation";

const OPEN_FAILED = "파일을 열지 못했습니다. 원본 파일과 외부 편집기 설치를 확인하세요.";
/** Why a language server problem did not open: the daemon's meaning, a helper's own sentence, or the generic text. */
function lspReason(error: unknown): string {
  if (error instanceof RpcError) return messageText(error.code, error.detail, error.diagnosticId);
  return error instanceof Error ? error.message : OPEN_FAILED;
}
type OpenCode = (key: DocKey, location: CodeLocation) => void;
export function ProblemsPanel({ onOpenCode }: { onOpenCode?: OpenCode } = {}) {
  const projectId = useCurrentProject((state) => state.id);
  return projectId ? (
    <ProjectProblems key={projectId} projectId={projectId} onOpenCode={onOpenCode} />
  ) : (
    <NoProjectState body="현재 프로젝트의 실행에서 발견된 문제를 보여 줍니다." />
  );
}
function ProjectProblems({ projectId, onOpenCode }: { projectId: string; onOpenCode?: OpenCode }) {
  const client = useRpc();
  const problems = useRpcQuery("observe.problems", { projectId, limit: 100 });
  useTopicInvalidation("observe.problems.changed", [["observe.problems", { projectId, limit: 100 }]]);
  const [editor, setEditor] = useState<"source" | "vscode" | "cursor">(onOpenCode ? "source" : "vscode");
  const [error, setError] = useState<string>();
  const request = useRef(0);
  useEffect(
    () => () => {
      request.current += 1;
    },
    [],
  );
  return (
    <section aria-label="실행 문제" style={{ overflow: "auto", height: "100%", padding: "var(--space-3)" }}>
      <label>
        문제 파일 편집기{" "}
        <select
          aria-label="문제 파일 편집기"
          value={editor}
          onChange={(event) => setEditor(event.target.value as "source" | "vscode" | "cursor")}
        >
          {onOpenCode && <option value="source">Devbox 코드 편집기</option>}
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
          const isLsp = problem.source.startsWith("lsp:");
          return (
            <li key={JSON.stringify(problem)}>
              <span>{problem.severity === "error" ? "오류" : problem.severity === "warning" ? "경고" : "정보"} </span>
              <a
                href="#"
                onClick={(event) => {
                  event.preventDefault();
                  setError(undefined);
                  const own = ++request.current;
                  if (isLsp) {
                    const target = {
                      path: problem.file,
                      line: problem.line,
                      column: problem.col ?? undefined,
                      source: { kind: "file", path: problem.file } as const,
                    };
                    const fail = (reason: unknown) => {
                      if (request.current === own) setError(lspReason(reason));
                    };
                    if (editor === "source" && onOpenCode) {
                      void resolveSourceLocation(client, projectId, target)
                        .then(({ key, location }) => {
                          if (request.current === own) onOpenCode(key, location);
                        })
                        .catch(fail);
                    } else {
                      void openFileLocation(client, projectId, target, editor === "source" ? "vscode" : editor).catch(
                        fail,
                      );
                    }
                    return;
                  }
                  if (!runId) {
                    setError("문제의 원본 실행을 찾을 수 없습니다.");
                    return;
                  }
                  if (editor === "source" && onOpenCode) {
                    void resolveSourceLocation(client, projectId, {
                      path: problem.file,
                      line: problem.line,
                      column: problem.col ?? undefined,
                      source: { kind: "run", runId },
                    })
                      .then(({ key, location }) => {
                        if (request.current === own) onOpenCode(key, location);
                      })
                      .catch(() => {
                        if (request.current === own)
                          setError("프로젝트 파일을 열지 못했습니다. 작업 폴더의 파일은 외부 편집기를 선택해 주세요.");
                      });
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
                    editor === "source" ? "vscode" : editor,
                  ).catch(() => setError(OPEN_FAILED));
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
