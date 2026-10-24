import { useState } from "react";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import { gitError, requestGit, runGit, sourceTargetKey, useSourceGit } from "./gitState";
export function StashPanel({ target }: { target: SourceTarget }) {
  const client = useRpc();
  const list = useRpcQuery("git.stash_list", { target });
  const status = useRpcQuery("git.status", { target });
  const busy = useSourceGit(
    (state) => state.entries[sourceTargetKey(target)]?.busy || !!state.entries[sourceTargetKey(target)]?.uncertain,
  );
  const [message, setMessage] = useState("");
  const [untracked, setUntracked] = useState(false);
  const disabled = busy || !status.data || status.isFetching || !!status.error || !!list.error || list.isFetching;
  return (
    <section aria-label="stash">
      <h3>stash · 변경 임시 보관</h3>
      <Button
        onClick={() => {
          void list.refetch();
          void status.refetch();
        }}
      >
        stash 새로고침
      </Button>
      {(list.error || status.error) && <Banner tone="danger">{gitError(list.error ?? status.error)}</Banner>}
      <label>
        stash 설명
        <input aria-label="stash 설명" value={message} onChange={(event) => setMessage(event.target.value)} />
      </label>
      <label>
        <input type="checkbox" checked={untracked} onChange={(event) => setUntracked(event.target.checked)} />
        추적하지 않는 파일 포함
      </label>
      <Button
        disabled={disabled}
        onClick={() => {
          if (status.data)
            void runGit(client, {
              method: "git.stash",
              params: {
                target,
                expect: status.data.expect,
                payload: { action: "push", message, includeUntracked: untracked },
              },
            });
        }}
      >
        stash 만들기
      </Button>
      <ul>
        {list.data?.map((item) => (
          <li key={item.oid}>
            {item.reference} · {item.subject}
            {(["apply", "pop"] as const).map((action) => (
              <Button
                key={action}
                disabled={disabled}
                onClick={() => {
                  if (status.data)
                    void runGit(client, {
                      method: "git.stash",
                      params: { target, expect: status.data.expect, payload: { action, oid: item.oid } },
                    });
                }}
              >
                {action === "apply" ? "적용" : "적용 후 제거"}
              </Button>
            ))}
            <Button
              disabled={disabled}
              onClick={() => {
                if (status.data)
                  requestGit({
                    method: "git.stash_drop",
                    params: { target, oid: item.oid, expect: status.data.expect },
                  });
              }}
            >
              stash 삭제
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}
