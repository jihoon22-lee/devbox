import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import type { DocKey } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Banner, Button, ConfirmDialog, Dialog } from "../../ui";
import { useDocuments } from "../documents/store";
import { validCodeKey } from "./editor/location";
import s from "./source.module.css";
import { useSource } from "./store";
export type FileOperation = { kind: "create" | "rename" | "delete"; key: DocKey };
export function FileOperationDialog({ operation, onClose }: { operation: FileOperation; onClose(): void }) {
  return operation.kind === "delete" ? (
    <DeleteDialog docKey={operation.key} onClose={onClose} />
  ) : (
    <NameDialog operation={operation} onClose={onClose} />
  );
}
function DeleteDialog({ docKey, onClose }: { docKey: DocKey; onClose(): void }) {
  const query = useRpcQuery("files.stat", { key: docKey }, { refetchOnWindowFocus: false });
  const remove = useRpcMutation("files.delete");
  const cache = useQueryClient();
  return (
    <ConfirmDialog
      open
      title="파일 삭제"
      body={`${docKey.path}을(를) 삭제합니다. 휴지통으로 옮기지 않습니다. 폴더는 비어 있어야 합니다.`}
      confirmLabel="삭제"
      tone="danger"
      disabled={!query.data || query.isFetching || remove.isPending || remove.isError}
      onCancel={() => !remove.isPending && onClose()}
      onConfirm={() => {
        if (!query.data) return;
        remove.mutate(
          { key: query.data.key, expect: query.data.rev },
          {
            onSuccess: () => {
              void cache.invalidateQueries({ queryKey: ["files.tree"] });
              onClose();
            },
          },
        );
      }}
    >
      {query.data && <p>확인한 크기: {query.data.rev.size.toLocaleString("ko")} 바이트</p>}
      {(query.error || remove.error) && (
        <Banner tone="danger">
          {messageFor((query.error ?? remove.error)!.code, (query.error ?? remove.error)!.detail).title}
          <Button
            onClick={() => {
              remove.reset();
              void query.refetch();
            }}
          >
            새 상태 확인
          </Button>
        </Banner>
      )}
    </ConfirmDialog>
  );
}
function NameDialog({ operation, onClose }: { operation: FileOperation; onClose(): void }) {
  const [name, setName] = useState(operation.key.path);
  const [busy, setBusy] = useState(false);
  const create = useRpcMutation("files.create");
  const rename = useRpcMutation("documents.rename");
  const cache = useQueryClient();
  const key = { root: operation.key.root, path: name };
  const valid = validCodeKey(key);
  const error = create.error ?? rename.error;
  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    try {
      if (operation.kind === "create") {
        const file = await create.mutateAsync({ key });
        await useSource.getState().open(file.key, { pinned: true });
      } else {
        const doc = useDocuments.getState().find(operation.key);
        await rename.mutateAsync({ key: operation.key, to: key, leaseId: doc?.leaseId ?? null });
      }
      void cache.invalidateQueries({ queryKey: ["files.tree"] });
      onClose();
    } catch {
      /* The mutation retains its typed error and idempotent retry identity. */
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      title={operation.kind === "create" ? "새 파일" : "이름 바꾸기"}
      description="프로젝트 안의 상대 경로를 입력하세요."
      onOpenChange={(open) => !open && !busy && onClose()}
      footer={
        <>
          <Button disabled={busy} onClick={onClose}>
            취소
          </Button>
          <Button variant="primary" disabled={busy || !valid} onClick={() => void submit()}>
            {operation.kind === "create" ? "만들기" : "이름 바꾸기"}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <label className={s.field}>
          파일 경로
          <input aria-label="파일 경로" value={name} onChange={(e) => setName(e.currentTarget.value)} disabled={busy} />
        </label>
      </form>
      {error && <Banner tone="danger">{messageFor(error.code, error.detail).title}</Banner>}
    </Dialog>
  );
}
