import { useState } from "react";
import { CLEANUP_UNKNOWN } from "../../../rpc/agentMessages";
import type { ErrorCode } from "../../../rpc/client";
import { RPC_ERROR_CODES } from "../../../rpc/gen/rpc";
import { type Message, messageFor } from "../../../rpc/messages";
import { Button } from "../../../ui";
import s from "./Detail.module.css";

const KNOWN: readonly string[] = RPC_ERROR_CODES;

/**
 * The daemon keeps why a closed task's cleanup stopped as the JSON of the error that stopped it,
 * `{"code":"agents.worktree_dirty","detail":{...}}`, or a sentence of its own for a fault it does
 * not describe. A code has its message in the table, which says all there is to say; the stored
 * text is shown, behind [자세히], only when there is no code to explain it.
 */
function messageOf(raw: string): { message: Message; coded: boolean } {
  try {
    const error: unknown = JSON.parse(raw);
    if (error && typeof error === "object" && "code" in error && typeof error.code === "string") {
      if (KNOWN.includes(error.code)) {
        const message = messageFor(error.code as ErrorCode, "detail" in error ? error.detail : undefined);
        if (message?.title) return { message, coded: true };
      }
    }
  } catch {
    /* Not an error's JSON. */
  }
  return { message: CLEANUP_UNKNOWN, coded: false };
}

/** What a closed task left behind and why. Only an error without a code offers its stored text. */
export function CleanupNotice({ error }: { error: string }) {
  const [open, setOpen] = useState(false);
  const { message, coded } = messageOf(error);
  return (
    <div className={s.cleanup}>
      <p role="alert">
        정리하지 못한 항목이 있습니다. {message.title} {message.action}
      </p>
      {!coded && (
        <>
          <Button size="sm" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
            자세히
          </Button>
          {open && <pre className={s.cleanupRaw}>{error}</pre>}
        </>
      )}
    </div>
  );
}
