import { useEffect, useState } from "react";
import { RpcError } from "../../../rpc/client";
import { useRpcQuery } from "../../../rpc/query";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { Button, EmptyState } from "../../../ui";
import type { DiffRange } from "../../../ui/diff";
import { ChangesList } from "../../source/ChangesList";
import { SourceDiff } from "../../source/SourceDiff";
import s from "./Detail.module.css";
import { useDrafts } from "./drafts";
import { QueryError } from "./QueryError";
import { quoteForPrompt } from "./quote";
import { freshReviewPatch } from "./reviewPatch";
import { useReviewSignature, useReviewSnapshot } from "./reviewSignature";
import { useSeen } from "./seen";

export function ChangesTab({ agentId, closed, onQuote }: { agentId: string; closed?: boolean; onQuote?: () => void }) {
  const changes = useRpcQuery("agents.changes", { id: agentId });
  const [selected, setSelected] = useState<string>();
  const entries = useSeen((state) => state.entries);
  const [verified, setVerified] = useState<Record<string, { signature: string; snapshot: string }>>({});
  const [refreshed, setRefreshed] = useState<{ path: string; snapshot: string; version: number }>();
  const [verificationError, setVerificationError] = useState<{
    path: string;
    snapshot: string;
    version: number;
    error: RpcError;
  }>();
  const [readVersion, setReadVersion] = useState(0);
  const [selection, setSelection] = useState<{ path: string; patch: string; range: DiffRange }>();
  const files = changes.data?.files ?? [];
  const current = files.find((file) => file.path === selected) ?? files[0];
  const path = current?.path ?? "";
  const patch = useRpcQuery("agents.patch", { id: agentId, path }, { enabled: !!current });
  const snapshot = useReviewSnapshot(agentId);
  const { qc } = useRpcSession();
  useEffect(() => {
    if (!path) return;
    let current = true;
    setVerificationError(undefined);
    // Only the selected patch is queried. A refresh makes off-selection marks
    // unverified until those files are selected again, avoiding query fan-out.
    void freshReviewPatch(qc, agentId, path, snapshot)
      .then(() => {
        if (current) setRefreshed({ path, snapshot, version: readVersion });
      })
      .catch((error) => {
        if (current)
          setVerificationError({
            path,
            snapshot,
            version: readVersion,
            error: error instanceof RpcError ? error : new RpcError("internal"),
          });
      });
    return () => {
      current = false;
    };
  }, [agentId, path, snapshot, qc, readVersion]);
  const retryPatch = async () => {
    setRefreshed(undefined);
    setVerificationError(undefined);
    setReadVersion((version) => version + 1);
  };
  const patchError =
    patch.error ??
    (verificationError?.path === path &&
    verificationError.snapshot === snapshot &&
    verificationError.version === readVersion
      ? verificationError.error
      : null);
  const reviewing = changes.isFetching || patch.isFetching;
  const stale =
    reviewing ||
    !!changes.error ||
    !!patchError ||
    refreshed?.path !== path ||
    refreshed?.snapshot !== snapshot ||
    refreshed?.version !== readVersion;
  const signature = useReviewSignature(changes.data, current, patch.data);
  useEffect(() => {
    if (!signature || stale) return;
    setVerified((previous) =>
      previous[path]?.signature === signature && previous[path]?.snapshot === snapshot
        ? previous
        : { ...previous, [path]: { signature, snapshot } },
    );
  }, [signature, stale, path, snapshot]);
  const seen = (file: (typeof files)[number]) => {
    const trusted =
      file.path === path
        ? !stale
          ? signature
          : undefined
        : !changes.isFetching && !changes.error && verified[file.path]?.snapshot === snapshot
          ? verified[file.path]?.signature
          : undefined;
    return !!trusted && entries[JSON.stringify([agentId, file.path])] === trusted;
  };
  const range = selection?.path === path && selection.patch === patch.data?.patch ? selection.range : undefined;
  return (
    <>
      <QueryError query={changes} />
      {changes.data && (
        <p>
          {closed && (
            <>
              종료할 때 확인한 기록 · 검토 HEAD {changes.data.headOid} · 기준 {changes.data.baseOid}
              <br />
            </>
          )}
          기준 {changes.data.base} 대비 · 앞섬 {changes.data.ahead} · 뒤처짐 {changes.data.behind}
        </p>
      )}
      {!changes.data && !changes.error && <p role="status">변경 내용을 불러오는 중…</p>}
      {changes.data && !files.length && <EmptyState title="변경한 파일이 없습니다" />}
      {!!files.length && (
        <div className={s.changes}>
          <ChangesList
            mode="review"
            files={files}
            selectedPath={path}
            onSelectPath={setSelected}
            isSeen={seen}
            className={s.files}
            itemClassName={s.file}
          />
          <div className={s.patch}>
            {current && (
              <Button
                size="sm"
                disabled={stale || !signature}
                onClick={() => {
                  if (!signature) return;
                  useSeen.getState().mark(agentId, path, signature);
                  const next = files.find((file) => file.path !== path && !seen(file));
                  if (next) setSelected(next.path);
                }}
              >
                봤음
              </Button>
            )}
            {range && !closed && (
              <Button
                size="sm"
                disabled={stale}
                onClick={() => {
                  const quotePath = range.side === "old" ? (current?.oldPath ?? path) : path;
                  useDrafts.getState().append(agentId, quoteForPrompt(quotePath, range));
                  onQuote?.();
                }}
              >
                지시에 인용
              </Button>
            )}
            <QueryError query={{ error: patchError, refetch: retryPatch }} />
            {stale && patch.data && (
              <p role="status">
                {reviewing
                  ? "최신 변경 내용을 확인하는 중입니다. 표시된 내용은 이전 조회 결과입니다."
                  : "최신 변경 내용을 확인하지 못했습니다. 표시된 내용은 이전 조회 결과입니다."}
              </p>
            )}
            {!patch.data && !patch.error && <p role="status">파일 변경 내용을 불러오는 중…</p>}
            {patch.data?.truncated && <p role="status">파일 변경 내용이 너무 길어 일부만 가져왔습니다.</p>}
            {patch.data && (
              <SourceDiff
                mode="review"
                path={path}
                snapshot={patch.data}
                onSelectLines={(range) => setSelection({ path, patch: patch.data!.patch, range })}
              />
            )}
          </div>
        </div>
      )}
    </>
  );
}
