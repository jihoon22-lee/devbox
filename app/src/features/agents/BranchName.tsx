import { useId, useState } from "react";
import s from "./Agents.module.css";
import { BRANCH_PREFIX, branchNameProblem, titleSlug } from "./model";

const NEEDS_NAME = "제목에 영문이나 숫자가 없어 이름을 만들 수 없습니다. 브랜치 이름을 입력해 주세요.";
const NOTE = `${BRANCH_PREFIX} 뒤에 붙는 이름입니다. 이 이름 그대로 만들고, 이미 있으면 만들지 않습니다. 비우면 제목에서 정하고 겹치면 번호를 붙입니다.`;

/**
 * The name of a new task's branch, which the dialog shows after the fixed `agent/`. It follows
 * the title until it is edited, and whatever it holds is the name the daemon is asked for: used
 * as it is, or refused when taken. Only an empty field sends nothing, so the daemon makes the
 * name from the title and numbers it when taken. A title without an ASCII letter or digit has no
 * name to follow, so the dialog asks for one.
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
  return { value, problem, send: enabled && name !== "" ? name : undefined, edit: setEdited };
}

export function BranchNameField({ branch }: { branch: ReturnType<typeof useBranchName> }) {
  const id = useId();
  return (
    <>
      <label htmlFor={`${id}-branch`}>브랜치 이름</label>
      <div className={s.branch}>
        <span aria-hidden="true">{BRANCH_PREFIX}</span>
        <input
          id={`${id}-branch`}
          value={branch.value}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={branch.problem ? true : undefined}
          aria-describedby={`${id}-branch-note`}
          onChange={(event) => branch.edit(event.target.value)}
        />
      </div>
      <p id={`${id}-branch-note`} className={branch.problem ? s.problem : s.muted}>
        {branch.problem ?? NOTE}
      </p>
    </>
  );
}
