import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import { EmptyState } from "../../ui";
import s from "./TerminalPage.module.css";
import { TerminalView } from "./TerminalView";
export function PopoutTerminal() {
  const { id } = useParams({ strict: false }) as { id?: string };
  const [ended, setEnded] = useState(false);
  return (
    <div className={s.popout}>
      {id && !ended ? (
        <TerminalView id={id} autoFocus onClose={() => setEnded(true)} />
      ) : (
        <EmptyState title="터미널 세션이 끝났습니다" />
      )}
    </div>
  );
}
