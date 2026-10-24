import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createElement, useEffect, useState } from "react";
import { type RpcClient, RpcError } from "../../rpc/client";
import type { Observation } from "../../rpc/gen/rpc";
import { useRpc } from "../../rpc/RpcProvider";
import { useActivityStorage, useActivityTray } from "./useActivityStatus";

/** `activity_drain`: the native buffer's oldest observations, kept until `activity_ack`. */
export interface Drained {
  runId: string;
  events: Observation[];
  /** Cumulative for the run (the daemon adds only the increase). */
  dropped: number;
  upto: number;
}
export type UplinkClient = Pick<RpcClient, "call" | "onState" | "state" | "subscribe">;
const INTERVAL_MS = 1000;
const MAX_EVENTS = 1000;

/** Only the main window drains: hidden capture and popout windows share the buffer. */
export function nativeMain() {
  return "__TAURI_INTERNALS__" in window && getCurrentWindow().label === "main";
}
function report(error: unknown) {
  console.error("활동 기록을 보내지 못했습니다", error);
}

/**
 * Every second while connected: drain → `activity.ingest` → ack on success.
 * A batch whose response was lost is resent unchanged (same `clientRequestId`);
 * any other failure re-drains next time. Rules are fetched again after each
 * reconnect and whenever `activity.changed` arrives (rules may have changed).
 */
export function startActivityUplink(client: UplinkClient): () => void {
  if (!nativeMain()) return () => {};
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Rules to fetch again: set by reconnects and `activity.changed`, also mid-fetch.
  let stale = true;
  let configuredRev: string | null = null;
  let held: Drained | null = null;
  const offState = client.onState((state) => {
    if (state !== "ready") stale = true;
  });
  const reconfigure = () => {
    stale = true;
  };
  const offChanged = client.subscribe("activity.changed", reconfigure, reconfigure);
  const configure = async () => {
    stale = false;
    const view = await client.call("activity.rules", {}).catch(() => null);
    if (view === null) stale = true;
    // `activity.changed` also follows stored sessions: recompile only on a new rules rev.
    else if (view.rev === configuredRev) return;
    configuredRev = null;
    await invoke("activity_configure", {
      rules: view?.rules ?? null,
      idleThresholdMs: view?.idleThresholdMs ?? null,
    });
    configuredRev = view?.rev ?? null;
  };
  const step = async () => {
    if (client.state !== "ready") return;
    if (stale) await configure();
    const fresh = await invoke<Drained>("activity_drain", { max: MAX_EVENTS });
    // Resend an uncertain batch only while the native queue still holds it:
    // a withdrawn consent empties the queue, and that must never be sent.
    const first = fresh.events[0]?.seq;
    const batch = held && fresh.runId === held.runId && first !== undefined && first <= held.upto ? held : fresh;
    held = null;
    if (stopped || batch.events.length === 0) return;
    held = batch;
    try {
      await client.call(
        "activity.ingest",
        { batch: { runId: batch.runId, upto: batch.upto }, events: batch.events, dropped: batch.dropped },
        { clientRequestId: `activity-${batch.runId}-${batch.upto}` },
      );
    } catch (error) {
      if (!(error instanceof RpcError && error.code === "connection_lost")) held = null;
      return;
    }
    held = null;
    await invoke("activity_ack", { upto: batch.upto });
  };
  const schedule = () => {
    timer = setTimeout(() => {
      void step()
        .catch(report)
        .finally(() => {
          if (!stopped) schedule();
        });
    }, INTERVAL_MS);
  };
  schedule();
  return () => {
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
    offState();
    offChanged();
  };
}

function NativeActivity() {
  const client = useRpc();
  useEffect(() => startActivityUplink(client), [client]);
  useActivityTray(useActivityStorage());
  return null;
}
/** Mount once inside the RPC session; renders nothing outside the native main window. */
export function ActivityUplink() {
  const [main] = useState(nativeMain);
  return main ? createElement(NativeActivity) : null;
}
