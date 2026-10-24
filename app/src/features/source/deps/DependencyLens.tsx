import { useState } from "react";
import { messageFor } from "../../../rpc/messages";
import { openExternal } from "../../../shell/external";
import { Banner, Button } from "../../../ui";
import s from "./deps.module.css";
import { audited, CACHE_MS, useDependencyLens } from "./model";
export function DependencyLens({ projectId }: { projectId: string }) {
  return <ProjectDependencies key={projectId} projectId={projectId} />;
}
function ProjectDependencies({ projectId }: { projectId: string }) {
  const model = useDependencyLens(projectId),
    { inventory, job } = model;
  const [limit, setLimit] = useState(100);
  const report = job?.result ?? job?.lastSuccess ?? undefined;
  const fresh =
    !!report &&
    !inventory.error &&
    !inventory.isFetching &&
    !model.notice &&
    job?.state === "succeeded" &&
    report.fingerprint === inventory.data?.fingerprint &&
    model.now >= report.completedAtMs &&
    model.now - report.completedAtMs < CACHE_MS;
  const failed = job?.state === "failed" || !!model.notice;
  const rows =
    inventory.data?.dependencies.flatMap((dependency) =>
      dependency.versions.map((version) => ({ dependency, version })),
    ) ?? [];
  const inventoryError = inventory.error ? messageFor(inventory.error.code, inventory.error.detail) : undefined;
  return (
    <section className={s.panel} aria-label="의존성">
      <div className={s.toolbar}>
        <Button onClick={() => void inventory.refetch()}>의존성 새로고침</Button>
        <Button
          onClick={() => void model.start()}
          disabled={
            !model.connected || model.starting || job?.state === "running" || !inventory.data || !!inventory.error
          }
        >
          {failed ? "다시 시도" : "취약점 조회"}
        </Button>
        {inventory.data && (
          <span>
            {rows.length}개 버전 · {inventory.data.locks.join(", ")}
          </span>
        )}
      </div>
      <p>조회하면 레지스트리 패키지의 이름과 버전을 OSV에 보냅니다. Git·로컬·출처 불명 의존성은 조회에서 제외합니다.</p>
      {!model.connected && <Banner tone="warn">연결이 끊겼습니다. 다시 연결되면 조회 상태를 확인합니다.</Banner>}
      {inventory.isLoading && <p>의존성을 불러오는 중…</p>}
      {inventoryError && (
        <Banner tone="danger">
          {inventoryError.title} {inventoryError.action}
        </Banner>
      )}
      {job?.state === "running" && (
        <p role="status">
          조회 중 {job.completed} / {job.total}
        </p>
      )}
      {model.starting && <p role="status">조회 준비 중…</p>}
      {failed && (
        <Banner tone="danger">
          {job?.error === "inputChanged"
            ? "조회 중 의존성 파일이 바뀌었습니다. 다시 시도하세요."
            : "조회 실패 · 다시 시도"}{" "}
          {model.notice}
        </Banner>
      )}
      {report && (
        <p>
          마지막 성공:{" "}
          <time dateTime={new Date(report.completedAtMs).toISOString()}>
            {new Date(report.completedAtMs).toLocaleString("ko-KR")}
          </time>
          {job?.cached ? " · 30분 이내 캐시" : ""}
          {!fresh ? " · 이전 조회 결과(오래됨 또는 현재 파일과 다름)" : ""}
        </p>
      )}
      {report && report.skipped > 0 && (
        <p>조회 제외: {report.skipped}개 버전. 전체 의존성의 안전 여부를 뜻하지 않습니다.</p>
      )}
      {!inventory.isLoading && inventory.data && rows.length === 0 && <p>잠금 파일에 의존성이 없습니다.</p>}
      {rows.length > 0 && (
        <div className={s.table}>
          <table>
            <caption>프로젝트 의존성 버전과 취약점 조회 결과</caption>
            <thead>
              <tr>
                <th>패키지</th>
                <th>버전</th>
                <th>관계</th>
                <th>취약점</th>
              </tr>
            </thead>
            <tbody>
              {rows.slice(0, limit).map(({ dependency, version }) => {
                const result = audited(report, dependency, version.version);
                return (
                  <tr key={`${dependency.ecosystem}:${dependency.name}:${version.version}:${version.source}`}>
                    <td>
                      {dependency.name} <small>({dependency.ecosystem})</small>
                    </td>
                    <td>{version.version}</td>
                    <td>{version.direct ? "직접 의존성" : "간접 의존성"}</td>
                    <td>
                      {version.source !== "registry" ? (
                        `조회 제외 · ${version.source === "git" ? "Git" : version.source === "local" ? "로컬" : "출처 불명"}`
                      ) : !result ? (
                        "미조회"
                      ) : result.advisories.length === 0 ? (
                        fresh ? (
                          "취약점 없음"
                        ) : (
                          "이전 조회: 발견 없음"
                        )
                      ) : (
                        <span className={s.links}>
                          {result.advisories.map((advisory) => (
                            <a
                              key={advisory.id}
                              href={`https://osv.dev/vulnerability/${encodeURIComponent(advisory.id)}`}
                              onClick={(event) => {
                                event.preventDefault();
                                void openExternal(`https://osv.dev/vulnerability/${encodeURIComponent(advisory.id)}`);
                              }}
                            >
                              {advisory.id}
                            </a>
                          ))}
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {rows.length > limit && <Button onClick={() => setLimit((n) => n + 100)}>100개 더 보기</Button>}
    </section>
  );
}
