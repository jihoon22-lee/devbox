import { useCallback, useEffect, useRef, useState } from "react";
import { messageOf } from "./useOperation";

type Phase = "idle" | "previewing" | "reviewed" | "applying" | "done" | "failed";
type State<P, R> = { state: Phase; preview: P | null; result: R | null; issue: string | null };
export interface ReviewSteps<P, R> {
  preview(signal: AbortSignal): Promise<P>;
  apply(preview: P): Promise<R>;
  discard?(preview: P): Promise<void>;
}
type Pending<P, R> = { value: P; owner: ReviewSteps<P, R> };
async function discardQuietly<P, R>(entry: Pending<P, R>) {
  try {
    await entry.owner.discard?.(entry.value);
  } catch {
    /* Unmounted cleanup has no live error surface. */
  }
}

export function useReviewFlow<P, R>(steps: ReviewSteps<P, R>) {
  const [current, setCurrent] = useState<State<P, R>>({ state: "idle", preview: null, result: null, issue: null });
  const latest = useRef(steps);
  latest.current = steps;
  const mounted = useRef(false);
  const sequence = useRef(0);
  const phase = useRef<Phase>("idle");
  const pending = useRef<Pending<P, R> | null>(null);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current += 1;
      controller.current?.abort();
      // Applying owns the preview until completion; never race it with discard.
      if (phase.current !== "applying") {
        const entry = pending.current;
        pending.current = null;
        if (entry) void discardQuietly(entry);
      }
    };
  }, []);

  const start = useCallback(async () => {
    if (!mounted.current || phase.current === "previewing" || phase.current === "applying") return;
    const id = ++sequence.current;
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    const owner = latest.current;
    const previous = pending.current;
    phase.current = previous ? "applying" : "previewing";
    setCurrent({ state: phase.current, preview: previous?.value ?? null, result: null, issue: null });
    const owned = () => mounted.current && id === sequence.current && !abort.signal.aborted;
    try {
      if (previous) {
        await previous.owner.discard?.(previous.value);
        if (pending.current === previous) pending.current = null;
      }
      if (!owned()) return;
      phase.current = "previewing";
      setCurrent({ state: "previewing", preview: null, result: null, issue: null });
      const value = await owner.preview(abort.signal);
      const entry = { value, owner };
      if (!owned()) {
        await discardQuietly(entry);
        return;
      }
      pending.current = entry;
      phase.current = "reviewed";
      setCurrent({ state: "reviewed", preview: value, result: null, issue: null });
    } catch (error) {
      if (owned()) {
        phase.current = "failed";
        setCurrent({ state: "failed", preview: pending.current?.value ?? null, result: null, issue: messageOf(error) });
      }
    }
  }, []);

  const confirm = useCallback(async () => {
    const entry = pending.current;
    if (!mounted.current || !entry || (phase.current !== "reviewed" && phase.current !== "failed")) return;
    const id = sequence.current;
    phase.current = "applying";
    setCurrent((value) => ({ ...value, state: "applying", issue: null }));
    try {
      const result = await entry.owner.apply(entry.value);
      if (pending.current === entry) pending.current = null;
      if (mounted.current && id === sequence.current) {
        phase.current = "done";
        setCurrent({ state: "done", preview: entry.value, result, issue: null });
      }
    } catch (error) {
      if (mounted.current && id === sequence.current) {
        phase.current = "failed";
        setCurrent((value) => ({ ...value, state: "failed", issue: messageOf(error) }));
      } else {
        if (pending.current === entry) pending.current = null;
        await discardQuietly(entry);
      }
    }
  }, []);

  const reset = useCallback(async () => {
    if (!mounted.current || phase.current === "applying") return;
    const id = ++sequence.current;
    controller.current?.abort();
    controller.current = null;
    const entry = pending.current;
    if (!entry) {
      phase.current = "idle";
      setCurrent({ state: "idle", preview: null, result: null, issue: null });
      return;
    }
    // Cancellation owns the preview until native cleanup acknowledges it.
    phase.current = "applying";
    setCurrent((value) => ({ ...value, state: "applying", issue: null }));
    try {
      await entry.owner.discard?.(entry.value);
      if (pending.current === entry) pending.current = null;
      if (mounted.current && id === sequence.current) {
        phase.current = "idle";
        setCurrent({ state: "idle", preview: null, result: null, issue: null });
      }
    } catch (error) {
      if (mounted.current && id === sequence.current) {
        phase.current = "failed";
        setCurrent({ state: "failed", preview: entry.value, result: null, issue: messageOf(error) });
      } else {
        if (pending.current === entry) pending.current = null;
        await discardQuietly(entry);
      }
    }
  }, []);

  return { ...current, start, confirm, reset };
}
