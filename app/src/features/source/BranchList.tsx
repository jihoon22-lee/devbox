import { useState } from "react";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import { gitError, requestGit, runGit, sourceTargetKey, useSourceGit } from "./gitState";
import { StashPanel } from "./StashPanel";
export function BranchList({ target }: { target: SourceTarget }) {
  const client = useRpc();
  const branches = useRpcQuery("git.branches", { target }, { staleTime: 0 });
  const status = useRpcQuery("git.status", { target }, { staleTime: 0 });
  const trees = useRpcQuery("git.worktrees", { target });
  const [name, setName] = useState("");
  const [force, setForce] = useState(false);
  const busy = useSourceGit(
    (state) => state.entries[sourceTargetKey(target)]?.busy || !!state.entries[sourceTargetKey(target)]?.uncertain,
  );
  const disabled =
    busy || !status.data || status.isFetching || !!status.error || !!branches.error || branches.isFetching;
  const checkout = (branch: string, create = false) => {
    if (!disabled && status.data)
      void runGit(client, { method: "git.checkout", params: { target, branch, create, expect: status.data.expect } });
  };
  return (
    <section aria-label="브랜치 목록">
      <Button
        onClick={() => {
          void branches.refetch();
          void status.refetch();
          void trees.refetch();
        }}
      >
        브랜치 새로고침
      </Button>
      {(branches.error || status.error || trees.error) && (
        <Banner tone="danger">{gitError(branches.error ?? status.error ?? trees.error)}</Banner>
      )}
      <label>
        새 브랜치 이름
        <input aria-label="새 브랜치 이름" value={name} onChange={(event) => setName(event.target.value)} />
      </label>
      <Button disabled={disabled || !name.trim()} onClick={() => checkout(name, true)}>
        만들고 전환
      </Button>
      <label>
        <input type="checkbox" checked={force} onChange={(event) => setForce(event.target.checked)} />
        병합되지 않은 브랜치도 삭제
      </label>
      {[false, true].map((remote) => (
        <section key={String(remote)} aria-label={remote ? "원격 브랜치" : "로컬 브랜치"}>
          <h3>{remote ? "원격" : "로컬"}</h3>
          <ul>
            {branches.data
              ?.filter((branch) => branch.remote === remote)
              .map((branch) => (
                <li key={branch.name}>
                  <span>{branch.name}</span>
                  {branch.current && " · 현재"}
                  {branch.upstream && ` · ${branch.upstream}`}
                  {!remote && (
                    <>
                      <Button
                        disabled={disabled || branch.current}
                        aria-label={`${branch.name} 전환`}
                        onClick={() => checkout(branch.name)}
                      >
                        전환
                      </Button>
                      <Button
                        disabled={disabled || branch.current || trees.data?.some((tree) => tree.branch === branch.name)}
                        aria-label={`${branch.name} 삭제`}
                        onClick={() => {
                          if (status.data)
                            requestGit({
                              method: "git.branch_delete",
                              params: {
                                target,
                                branch: branch.name,
                                oid: branch.oid,
                                force,
                                expect: status.data.expect,
                              },
                            });
                        }}
                      >
                        삭제
                      </Button>
                    </>
                  )}
                </li>
              ))}
          </ul>
        </section>
      ))}
      <details>
        <summary>작업 폴더</summary>
        <ul>
          {trees.data?.map((tree) => (
            <li key={tree.path}>
              {tree.path} · {tree.branch ?? "분리된 HEAD"}
              {tree.main ? " · 기본 폴더" : ""}
              {tree.locked ? " · 잠김" : ""}
            </li>
          ))}
        </ul>
      </details>
      <StashPanel target={target} />
    </section>
  );
}
