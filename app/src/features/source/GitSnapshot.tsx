import type { RemoteExpect, SourceExpect } from "../../rpc/gen/rpc";
import type { GitAction } from "./gitState";

function RemoteSnapshot({ value }: { value: RemoteExpect }) {
  return (
    <dl>
      <dt>브랜치</dt>
      <dd>{value.branch ?? "분리된 HEAD"}</dd>
      <dt>현재 커밋</dt>
      <dd>
        <code>{value.headOid ?? "커밋 없음"}</code>
      </dd>
      <dt>원격 대상</dt>
      <dd>{value.upstream ? `${value.upstream.remote} · ${value.upstream.destinationRef}` : "upstream 없음"}</dd>
      <dt>확인한 원격 커밋</dt>
      <dd>
        <code>{value.upstreamOid ?? "없음"}</code>
      </dd>
    </dl>
  );
}
function RepositorySnapshot({ value }: { value: SourceExpect }) {
  return (
    <>
      <RemoteSnapshot value={value.state} />
      <details>
        <summary>확인한 파일 상태</summary>
        <p>
          스테이징: <code>{value.indexHash}</code>
        </p>
        <p>
          작업 파일: <code>{value.worktreeHash}</code>
        </p>
      </details>
    </>
  );
}
export function GitSnapshot({ action }: { action: GitAction }) {
  switch (action.method) {
    case "git.push":
    case "git.pull":
      return <RemoteSnapshot value={action.params.expect} />;
    case "git.branch_delete":
      return (
        <>
          <p>
            삭제할 브랜치: {action.params.branch} · <code>{action.params.oid}</code>
          </p>
          <p>{action.params.force ? "병합되지 않은 커밋도 삭제합니다." : "병합한 브랜치만 삭제합니다."}</p>
          <RepositorySnapshot value={action.params.expect} />
        </>
      );
    case "git.cleanup":
      return (
        <ul>
          {action.params.candidates.map((item) => (
            <li key={JSON.stringify([item.branch, item.path])}>
              {item.branch} · {item.path ?? "브랜치만"} · <code>{item.oid}</code>
            </li>
          ))}
        </ul>
      );
    case "git.discard":
    case "git.stage":
    case "git.unstage":
      return (
        <>
          <p>{action.params.path}</p>
          {action.method === "git.discard" && <p>작업 파일의 변경을 버립니다. 되돌릴 수 없습니다.</p>}
          <details>
            <summary>확인한 파일 상태</summary>
            <code>{action.params.expect.fingerprint}</code>
          </details>
        </>
      );
    case "git.stash_drop":
      return (
        <>
          <p>
            삭제할 stash: <code>{action.params.oid}</code>
          </p>
          <RepositorySnapshot value={action.params.expect} />
        </>
      );
    case "git.checkout":
    case "git.stash":
      return <RepositorySnapshot value={action.params.expect} />;
    case "git.fetch":
      return <p>원격 변경 정보를 가져옵니다.</p>;
  }
}
