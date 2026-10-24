import { useCallback, useEffect, useRef, useState } from "react";
import { RpcError } from "../../../rpc/client";
import type { SourceTarget } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { useRpc, useRpcSession } from "../../../rpc/RpcProvider";
import { Banner, Button } from "../../../ui";
import { MergeView } from "./MergeView";
import s from "./merge.module.css";
import { choose, mergeKey, useMergeDrafts } from "./model";

function errorText(error: unknown) {
  if (!(error instanceof RpcError)) return "충돌 정보를 불러오지 못했습니다. 다시 시도하세요.";
  const message = messageFor(error.code, error.detail);
  return `${message.title} ${message.action ?? ""}`;
}
export function MergePanel({ target, initialPath }: { target: SourceTarget; initialPath?: string }) {
  return <TargetMergePanel key={mergeKey(target, "")} target={target} initialPath={initialPath} />;
}
function TargetMergePanel({ target, initialPath }: { target: SourceTarget; initialPath?: string }) {
  const files = useRpcQuery("git.conflicts", { target });
  const [selected, setSelected] = useState(initialPath);
  const path = selected ?? files.data?.[0]?.path;
  useEffect(() => {
    if (!selected && files.data?.[0]) setSelected(files.data[0].path);
  }, [selected, files.data]);
  return (
    <section className={s.panel} aria-label="충돌 해결">
      <div className={s.toolbar}>
        <Button onClick={() => void files.refetch()}>충돌 목록 새로고침</Button>
        {(files.data ?? []).map((file) => (
          <Button key={file.path} onClick={() => setSelected(file.path)}>
            {file.path}
          </Button>
        ))}
      </div>
      {files.error && <Banner tone="danger">{errorText(files.error)}</Banner>}
      {files.isLoading && <p>충돌 목록을 불러오는 중…</p>}
      {!files.isLoading && files.data?.length === 0 && <p>현재 충돌 파일이 없습니다.</p>}
      {path && (
        <MergeFile key={mergeKey(target, path)} target={target} path={path} onApplied={() => void files.refetch()} />
      )}
    </section>
  );
}
function MergeFile({ target, path, onApplied }: { target: SourceTarget; path: string; onApplied: () => void }) {
  const client = useRpc(),
    { state } = useRpcSession();
  const apply = useRpcMutation("git.conflict_apply");
  const key = mergeKey(target, path);
  const draft = useMergeDrafts((s) => s.entries[key]);
  const [notice, setNotice] = useState("");
  const applying = useRef(false);
  const projectId = target.projectId,
    worktreeId = target.worktreeId;
  const reload = useCallback(async () => {
    const store = useMergeDrafts.getState(),
      request = store.begin(key);
    try {
      const preview = await client.call("git.conflict_preview", {
        target: { projectId, ...(worktreeId ? { worktreeId } : {}) },
        path,
      });
      store.receive(key, request, preview);
    } catch (error) {
      store.fail(key, request, errorText(error));
    }
  }, [client, key, projectId, worktreeId, path]);
  useEffect(() => {
    if (state === "ready") void reload();
  }, [reload, state]);
  const submit = async () => {
    const current = useMergeDrafts.getState().entries[key];
    if (
      applying.current ||
      current?.loading ||
      current?.error ||
      !current?.preview?.textSupported ||
      current.resolution === undefined ||
      state !== "ready"
    )
      return;
    applying.current = true;
    setNotice("");
    try {
      await apply.mutateAsync({ target, path, resolution: current.resolution, token: current.preview.token });
      setNotice("해결안을 파일과 Git에 반영했습니다. 작성한 내용은 이 화면에 보존합니다.");
      onApplied();
    } catch (error) {
      setNotice(errorText(error));
      await reload();
    } finally {
      applying.current = false;
    }
  };
  return (
    <div className={s.file}>
      <h3>{path}</h3>
      {notice && <Banner tone="warn">{notice}</Banner>}
      {draft?.error && <Banner tone="warn">{draft.error}</Banner>}
      <div className={s.toolbar}>
        <Button disabled={draft?.loading || apply.isPending || state !== "ready"} onClick={() => void reload()}>
          현재 파일 다시 읽기 · 해결안 유지
        </Button>
        {(["ours", "theirs", "both"] as const).map((side, index) => (
          <Button
            key={side}
            disabled={!draft?.preview?.textSupported || draft.loading}
            onClick={() => {
              if (draft?.preview) useMergeDrafts.getState().edit(key, choose(draft.preview, side));
            }}
          >
            {["내 것", "그쪽", "둘 다"][index]}
          </Button>
        ))}
        <Button
          disabled={
            !draft?.preview?.textSupported || draft.loading || !!draft.error || apply.isPending || state !== "ready"
          }
          onClick={() => void submit()}
        >
          해결안 적용
        </Button>
      </div>
      {draft?.resolution === null && (
        <Banner tone="warn">
          선택한 쪽에는 파일이 없습니다. 적용하면 이 파일을 삭제하고 Git에 반영합니다. 결과를 직접 편집하면 파일
          내용으로 해결합니다.
        </Banner>
      )}
      {draft?.loading && <p>현재 파일과 Git 상태를 다시 읽는 중…</p>}
      {draft?.preview && (
        <>
          {draft.preview.textSupported ? (
            <MergeView
              preview={draft.preview}
              resolution={draft.resolution ?? ""}
              onChange={(text) => useMergeDrafts.getState().edit(key, text)}
            />
          ) : (
            <Banner tone="warn">바이너리·큰 파일 또는 지원하지 않는 파일 형식은 이 화면에서 해결할 수 없습니다.</Banner>
          )}
          <details>
            <summary>현재 작업 파일 · 해결안과 비교</summary>
            <pre className={s.current}>{draft.preview.merged ?? "파일 없음 또는 텍스트 표시 불가"}</pre>
          </details>
        </>
      )}
    </div>
  );
}
