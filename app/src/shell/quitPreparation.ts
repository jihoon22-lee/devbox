import type { DocumentsStore } from "../features/documents/store";

/** One renderer participates in a native exit round; cancellation cannot race a durable save. */
export function createQuitPreparation(
  documents: Pick<DocumentsStore, "prepareWindowClose" | "cancelWindowClose"> &
    Partial<Pick<DocumentsStore, "abortWindowClose">>,
  ack: (round: number, safe: boolean) => Promise<unknown>,
  blocked: () => void,
) {
  let active: { round: number; cancelled: boolean; settled: boolean } | undefined;
  return {
    async prepare(round: number) {
      if (active) {
        if (active.round !== round) await ack(round, false);
        return;
      }
      const own = { round, cancelled: false, settled: false };
      active = own;
      try {
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
      if (active?.round !== round) return;
      active.cancelled = true;
      documents.abortWindowClose?.();
      // If prepare already settled, reopening is safe; otherwise finally does it.
      if (active.settled) {
        documents.cancelWindowClose();
        active = undefined;
      }
      blocked();
    },
  };
}
