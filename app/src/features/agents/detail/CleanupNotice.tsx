import { useState } from "react";
import { CLEANUP_UNKNOWN } from "../../../rpc/agentMessages";
import { cleanupMessage } from "../../../rpc/cleanupMessages";
import type { ErrorCode } from "../../../rpc/client";
import { RPC_ERROR_CODES } from "../../../rpc/gen/rpc";
import type { Message } from "../../../rpc/messages";
import { Button } from "../../../ui";
import s from "./Detail.module.css";

const KNOWN: readonly string[] = RPC_ERROR_CODES;

/**
 * The daemon keeps why a closed task's cleanup stopped as the JSON of the error that stopped it,
 * `{"code":"agents.worktree_dirty","detail":{...}}`, or a sentence of its own for a fault it does
 * not describe. What the code says (or a general message, when the code is missing or unknown) is
 * the main text; the stored text, with any files it names, is only the details behind [자세히].
 * The next action is the notice's own (`cleanupMessage`): the table's action belongs to the
 * operation that failed.
 */
function messageOf(raw: string): Message {
  try {
    const error: unknown = JSON.parse(raw);
    if (error && typeof error === "object" && "code" in error && typeof error.code === "string") {
      if (KNOWN.includes(error.code)) {
        const message = cleanupMessage(error.code as ErrorCode, "detail" in error ? error.detail : undefined, "notice");
        if (message?.title) return message;
      }
    }
  } catch {
    /* Not an error's JSON. */
  }
  return CLEANUP_UNKNOWN;
}

/** What a closed task left behind and why, with the stored text one [자세히] away. */
export function CleanupNotice({ error }: { error: string }) {
  const [open, setOpen] = useState(false);
  const message = messageOf(error);
  return (
    <div className={s.cleanup}>
      <p role="alert">
        정리하지 못한 항목이 있습니다. {message.title} {message.action}
      </p>
      <Button size="sm" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
        자세히
      </Button>
      {open && <pre className={s.cleanupRaw}>{error}</pre>}
    </div>
  );
}
