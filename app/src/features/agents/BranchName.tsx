import { useId, useLayoutEffect, useRef, useState } from "react";
import s from "./Agents.module.css";
import { BRANCH_PREFIX, branchNameProblem, titleSlug } from "./model";

const NEEDS_NAME = "제목에 영문이나 숫자가 없어 이름을 만들 수 없습니다. 브랜치 이름을 입력해 주세요.";
const FROM_TITLE = `${BRANCH_PREFIX} 뒤에 붙는 이름입니다. 직접 고치지 않으면 제목에서 만들고, 겹치면 번호를 붙입니다.`;
const AS_EDITED = `${BRANCH_PREFIX} 뒤에 붙는 이름입니다. 고친 이름 그대로 만들고, 이미 있으면 만들지 않습니다.`;

/**
 * Capital ASCII letters only. The daemon's rule is lowercase ASCII, so a capital is typed as its
 * lowercase instead of refused; any other letter is left for the rule to name.
 */
const lowerAscii = (text: string) => text.replace(/[A-Z]/g, (letter) => letter.toLowerCase());

/**
 * The name of a new task's branch, which the dialog shows after the fixed `agent/`. It follows
 * the title until it is edited. Only a name that was edited and is not empty is sent: the daemon
 * uses it exactly or refuses it when taken. A name nobody edited is only the title's own, so it is
 * not sent, and the daemon makes it from the title as ever and numbers it when taken, which is why
 * an emptied field shows that name as its placeholder. A title without an ASCII letter or digit has
 * no name to follow, so the dialog asks for one.
 */
export function useBranchName(title: string, enabled: boolean) {
  const [edited, setEdited] = useState<string | null>(null);
  const suggested = titleSlug(title.trim());
  const value = edited ?? suggested;
  const name = value.trim();
  const problem = !enabled
    ? null
    : name !== ""
      ? branchNameProblem(name)
      : title.trim() !== "" && suggested === ""
        ? NEEDS_NAME
        : null;
  const typed = edited !== null && name !== "";
  return {
    value,
    suggested,
    typed,
    problem,
    send: enabled && typed ? name : undefined,
    edit: (next: string) => setEdited(lowerAscii(next)),
  };
}

export function BranchNameField({ branch }: { branch: ReturnType<typeof useBranchName> }) {
  const id = useId();
  const field = useRef<HTMLInputElement>(null);
  const caret = useRef<[number, number] | null>(null);
  // A capital typed in the middle is rewritten to its lowercase, which sends the caret of a
  // controlled field to its end. The length is the same, so the caret goes back where it was.
  useLayoutEffect(() => {
    const [start, end] = caret.current ?? [];
    caret.current = null;
    if (start !== undefined && end !== undefined && document.activeElement === field.current)
      field.current?.setSelectionRange(start, end);
  });
  return (
    <>
      <label htmlFor={`${id}-branch`}>브랜치 이름</label>
      <div className={s.branch}>
        <span aria-hidden="true">{BRANCH_PREFIX}</span>
        <input
          ref={field}
          id={`${id}-branch`}
          value={branch.value}
          placeholder={branch.suggested}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={branch.problem ? true : undefined}
          aria-describedby={`${id}-branch-note`}
          onChange={(event) => {
            const { value, selectionStart, selectionEnd } = event.target;
            caret.current =
              value !== lowerAscii(value) && selectionStart !== null && selectionEnd !== null
                ? [selectionStart, selectionEnd]
                : null;
            branch.edit(value);
          }}
        />
      </div>
      <p id={`${id}-branch-note`} className={branch.problem ? s.problem : s.muted}>
        {branch.problem ?? (branch.typed ? AS_EDITED : FROM_TITLE)}
      </p>
    </>
  );
}
