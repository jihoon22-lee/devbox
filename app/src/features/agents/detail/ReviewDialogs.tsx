import { useState } from "react";
import type { MergeKind } from "../../../rpc/gen/rpc";
import { Button, ConfirmDialog, Dialog, TextArea } from "../../../ui";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";
import type { useReviewAction } from "./useReviewAction";
export type EditIntent = { kind: "squash" } | { kind: "pr"; body: string };
export function ReviewDialogs({
  task,
  edit,
  setEdit,
  review,
}: {
  task: AgentTask;
  edit?: EditIntent;
  setEdit: (edit?: EditIntent) => void;
  review: ReturnType<typeof useReviewAction>;
}) {
  return (
    <>
      {edit && (
        <EditReview
          key={`${task.id}:${edit.kind}`}
          task={task}
          edit={edit}
          onCancel={() => setEdit(undefined)}
          onDone={(title, body) => {
            setEdit(undefined);
            void review.open(
              edit.kind === "squash"
                ? { kind: "merge", strategy: "squash" as MergeKind, message: title }
                : { kind: "pr", title, body },
            );
          }}
        />
      )}
      <ConfirmDialog
        open={!!review.snapshot}
        title={`${review.label} · ${task.title}`}
        body={review.body}
        confirmLabel={review.label}
        tone={review.snapshot?.intent.kind === "discard" ? "danger" : "primary"}
        disabled={review.busy || review.blocked}
        onConfirm={() => void review.confirm()}
        onCancel={review.cancel}
      >
        {review.snapshot && review.notice && review.blocked && !review.loading && (
          <Button onClick={() => void review.retry()}>다시 확인</Button>
        )}
      </ConfirmDialog>
    </>
  );
}
function EditReview({
  task,
  edit,
  onCancel,
  onDone,
}: {
  task: AgentTask;
  edit: EditIntent;
  onCancel: () => void;
  onDone: (title: string, body: string) => void;
}) {
  const [title, setTitle] = useState(edit.kind === "squash" ? `${task.title} (agent)` : task.title);
  const [body, setBody] = useState(edit.kind === "pr" ? edit.body : "");
  return (
    <Dialog
      open
      title={edit.kind === "squash" ? "병합 커밋 메시지" : "PR 작성"}
      onOpenChange={(open) => !open && onCancel()}
      footer={
        <>
          <Button onClick={onCancel}>취소</Button>
          <Button variant="primary" disabled={!title.trim()} onClick={() => onDone(title.trim(), body)}>
            내용 확인
          </Button>
        </>
      }
    >
      <label className={s.field}>
        {edit.kind === "squash" ? "커밋 메시지" : "PR 제목"}
        <TextArea value={title} onChange={(event) => setTitle(event.target.value)} />
      </label>
      {edit.kind === "pr" && (
        <label className={s.field}>
          PR 본문
          <TextArea value={body} onChange={(event) => setBody(event.target.value)} />
        </label>
      )}
    </Dialog>
  );
}
