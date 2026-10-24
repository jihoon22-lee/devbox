import { useState } from "react";
import { Button, Dialog } from "../../ui";
import { folderOf, joinPath, stem } from "./notePaths";
import s from "./notes.module.css";

/** Asks for a new note name in the same folder; links in other notes follow (`rewriteLinks`). */
export function RenameDialog({
  path,
  onClose,
  onRename,
}: {
  path: string | null;
  onClose(): void;
  onRename(from: string, to: string): void;
}) {
  const [name, setName] = useState("");
  const [shown, setShown] = useState<string | null>(null);
  if (path !== shown) {
    setShown(path);
    setName(path ? stem(path) : "");
  }
  const trimmed = name.trim().replace(/\.md$/i, "");
  const valid = !!trimmed && !/[\\]|^\.|\/\.|\.\.|\/$|^\//.test(trimmed);
  const submit = () => {
    if (!path || !valid) return;
    const to = joinPath(folderOf(path), `${trimmed}.md`);
    onClose();
    if (to !== path) onRename(path, to);
  };
  return (
    <Dialog
      open={path !== null}
      onOpenChange={(open) => !open && onClose()}
      title="노트 이름 바꾸기"
      description="다른 노트에서 이 노트를 가리키는 링크도 새 이름으로 고칩니다."
      footer={
        <>
          <Button onClick={onClose}>취소</Button>
          <Button variant="primary" disabled={!valid} onClick={submit}>
            이름 바꾸기
          </Button>
        </>
      }
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className={s.field}>
          새 이름
          <input value={name} onChange={(event) => setName(event.currentTarget.value)} spellCheck={false} />
        </label>
      </form>
    </Dialog>
  );
}
