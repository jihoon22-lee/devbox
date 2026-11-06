import type { DocumentsStore } from "../features/documents/store";

/**
 * One renderer participates in a native exit round; cancellation cannot race a durable save.
 * `ask` (main window only) first asks about unsaved changes that live only in this renderer. It returns
 * nothing when there is nothing to ask, so the documents check starts in the same turn as without it.
 */
export function createQuitPreparation(
  documents: Pick<DocumentsStore, "prepareWindowClose" | "cancelWindowClose"> &
    Partial<Pick<DocumentsStore, "abortWindowClose">>,
  ack: (round: number, safe: boolean) => Promise<unknown>,
  blocked: () => void,
  ask?: (signal: AbortSignal) => Promise<boolean> | undefined,
) {
  let active:
    | { round: number; cancelled: boolean; settled: boolean; declined: boolean; prompt: AbortController }
    | undefined;
  return {
    async prepare(round: number) {
      if (active) {
        if (active.round !== round) await ack(round, false);
        return;
      }
      const own = { round, cancelled: false, settled: false, declined: false, prompt: new AbortController() };
      active = own;
      try {
        const asking = ask?.(own.prompt.signal);
        if (asking && !(await asking)) {
          if (own.cancelled) return;
          // The user's [취소], not a failure. Mark it before the ACK: the native rollback can arrive first.
          own.declined = true;
          await ack(round, false);
          return;
        }
        if (own.cancelled) return;
        const safe = await documents.prepareWindowClose();
        if (own.cancelled) return;
        await ack(round, safe);
        if (!safe) blocked();
      } catch {
        blocked();
        await ack(round, false).catch(() => {});
      } finally {
        own.settled = true;
        if (own.cancelled) documents.cancelWindowClose();
        // Keep successful preparations frozen until native exit or rollback.
        if (own.cancelled) active = undefined;
      }
    },
    cancel(round: number) {
      const own = active;
      if (own?.round !== round) return;
      own.cancelled = true;
      own.prompt.abort();
      documents.abortWindowClose?.();
      // If prepare already settled, reopening is safe; otherwise finally does it.
      if (own.settled) {
        documents.cancelWindowClose();
        active = undefined;
      }
      if (!own.declined) blocked();
    },
  };
}
