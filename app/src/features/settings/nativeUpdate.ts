import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { isNative } from "./nativePreferences";

const phases = ["idle", "checking", "available", "downloading", "ready", "installing", "failed"] as const;
const errors = [
  "network",
  "invalidRelease",
  "size",
  "digest",
  "timeout",
  "storage",
  "cancelled",
  "staleOperation",
] as const;
export type UpdateError = (typeof errors)[number];
export type UpdateAction = "check" | "download" | "cancel";
export interface UpdateSnapshot {
  enabled: boolean;
  disabledReason: null | "channel" | "developmentVersion";
  phase: (typeof phases)[number];
  generation: number;
  revision: number;
  currentVersion: string;
  availableVersion: string | null;
  receivedBytes: number;
  totalBytes: number | null;
  error: UpdateError | null;
  nextAction: "none" | UpdateAction;
}
export type UpdateIssue = UpdateError | "unavailable" | "invalidSnapshot";
const integer = (value: unknown) => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const version = (value: unknown) =>
  typeof value === "string" && value.length <= 128 && /^\d+\.\d+\.\d+(?:[-+][\w.+-]+)?$/.test(value);

/** Validate a display copy; eligibility, versions and operation ownership remain native decisions. */
export function updateSnapshot(value: unknown): UpdateSnapshot | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const candidate = value as Partial<UpdateSnapshot>;
  if (
    typeof candidate.enabled !== "boolean" ||
    !(
      candidate.disabledReason === null ||
      candidate.disabledReason === "channel" ||
      candidate.disabledReason === "developmentVersion"
    ) ||
    !phases.includes(candidate.phase as UpdateSnapshot["phase"]) ||
    !integer(candidate.generation) ||
    !integer(candidate.revision) ||
    !version(candidate.currentVersion) ||
    !(candidate.availableVersion === null || version(candidate.availableVersion)) ||
    !integer(candidate.receivedBytes) ||
    !(candidate.totalBytes === null || integer(candidate.totalBytes)) ||
    !(candidate.error === null || errors.includes(candidate.error as UpdateError)) ||
    !["none", "check", "download", "cancel"].includes(candidate.nextAction ?? "invalid")
  )
    return null;
  return {
    enabled: candidate.enabled,
    disabledReason: candidate.disabledReason,
    phase: candidate.phase,
    generation: candidate.generation,
    revision: candidate.revision,
    currentVersion: candidate.currentVersion,
    availableVersion: candidate.availableVersion,
    receivedBytes: candidate.receivedBytes,
    totalBytes: candidate.totalBytes,
    error: candidate.error,
    nextAction: candidate.nextAction,
  } as UpdateSnapshot;
}

interface View {
  snapshot: UpdateSnapshot | null;
  issue: UpdateIssue | null;
  connected: boolean;
  busy: boolean;
}
const initial: View = { snapshot: null, issue: null, connected: false, busy: false };

export function useNativeUpdate() {
  const native = isNative();
  const [view, setView] = useState<View>(initial);
  const [attempt, setAttempt] = useState(0);
  const lifetime = useRef(0);
  const current = useRef<UpdateSnapshot | null>(null);
  const pending = useRef(false);
  const receive = useCallback((value: unknown, owner: number, requestedRevision?: number) => {
    if (owner !== lifetime.current) return;
    const snapshot = updateSnapshot(value);
    if (!snapshot) {
      if (requestedRevision === undefined || (current.current?.revision ?? -1) === requestedRevision) {
        setView((previous) => ({ ...previous, issue: "invalidSnapshot" }));
      }
      return;
    }
    if (current.current && snapshot.revision <= current.current.revision) return;
    current.current = snapshot;
    setView((previous) => ({ ...previous, snapshot, issue: null }));
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: an explicit retry re-registers before reading native state.
  useEffect(() => {
    const owner = ++lifetime.current;
    let off: (() => void) | undefined;
    current.current = null;
    pending.current = false;
    setView(initial);
    if (!native) return;
    void (async () => {
      try {
        const stop = await listen<unknown>("devbox://update", ({ payload }) => receive(payload, owner));
        if (owner !== lifetime.current) {
          stop();
          return;
        }
        off = stop;
        setView((previous) => ({ ...previous, connected: true }));
        receive(await invoke<unknown>("update_status"), owner, -1);
      } catch {
        if (owner === lifetime.current && (!off || !current.current)) {
          setView((previous) => ({ ...previous, issue: "unavailable" }));
        }
      }
    })();
    return () => {
      lifetime.current++;
      off?.();
    };
  }, [native, attempt, receive]);

  const run = async (action: UpdateAction) => {
    const snapshot = current.current;
    if (
      !native ||
      !view.connected ||
      pending.current ||
      !snapshot?.enabled ||
      snapshot.nextAction !== action ||
      view.issue === "invalidSnapshot" ||
      view.issue === "unavailable" ||
      view.issue === "staleOperation"
    )
      return;
    const owner = lifetime.current;
    pending.current = true;
    setView((previous) => ({ ...previous, busy: true, issue: null }));
    try {
      const result =
        action === "check"
          ? await invoke<unknown>("update_check")
          : await invoke<unknown>(`update_${action}`, { generation: snapshot.generation });
      receive(result, owner, snapshot.revision);
    } catch (error) {
      if (owner === lifetime.current && current.current?.revision === snapshot.revision) {
        const issue = errors.includes(error as UpdateError) ? (error as UpdateError) : "unavailable";
        setView((previous) => ({ ...previous, issue }));
      }
    } finally {
      if (owner === lifetime.current) {
        pending.current = false;
        setView((previous) => ({ ...previous, busy: false }));
      }
    }
  };
  return { ...view, native, run, refresh: () => setAttempt((value) => value + 1) };
}
