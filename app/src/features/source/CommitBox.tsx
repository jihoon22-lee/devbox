import { useState } from "react";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import { commitSource, gitError, prepareRemote, sourceTargetKey, useSourceGit } from "./gitState";
export function CommitBox({ target }: { target: SourceTarget }) {
  const client = useRpc();
  const status = useRpcQuery("git.status", { target }, { staleTime: 0 });
  const entry = useSourceGit((state) => state.entries[sourceTargetKey(target)]);
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false),
    [push, setPush] = useState(false);
  const disabled =
    !message.trim() || !status.data || status.isFetching || !!status.error || entry?.busy || !!entry?.uncertain;
  const submit = async () => {
    if (disabled || !status.data) return;
    await commitSource(client, { target, message, amend, expect: status.data.expect }, push);
    if (useSourceGit.getState().entries[sourceTargetKey(target)]?.committed) {
      setMessage("");
      setAmend(false);
    }
  };
  return (
    <section aria-label="커밋">
      <label>
        커밋 메시지
        <textarea
          aria-label="커밋 메시지"
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={(event) => {
            if (event.ctrlKey && event.key === "Enter") {
              event.preventDefault();
              void submit();
            }
          }}
        />
      </label>
      <p>{[...(message.split("\n")[0] ?? "")].length}/72자 · 제목은 72자 이내를 권장합니다. Ctrl+Enter로 커밋합니다.</p>
      <label>
        <input type="checkbox" checked={amend} onChange={(event) => setAmend(event.target.checked)} />
        마지막 커밋 수정(amend)
      </label>
      {amend && <p>마지막 커밋의 내용과 메시지를 바꿉니다.</p>}
      <label>
        <input type="checkbox" checked={push} onChange={(event) => setPush(event.target.checked)} />
        커밋 후 push 확인
      </label>
      <Button disabled={disabled} onClick={() => void submit()}>
        커밋
      </Button>
      {status.error && (
        <Banner tone="danger">
          {gitError(status.error)}
          <Button onClick={() => void status.refetch()}>커밋 상태 다시 읽기</Button>
        </Banner>
      )}
      {entry?.committed && <p role="status">커밋을 완료했습니다.</p>}
      {entry?.error && <Banner tone="danger">{entry.error}</Banner>}
      {entry?.committed && entry.needsPush && (
        <Button disabled={entry.busy} onClick={() => void prepareRemote(client, target, "git.push", false)}>
          완료한 커밋의 push 상태 다시 읽기
        </Button>
      )}
    </section>
  );
}
