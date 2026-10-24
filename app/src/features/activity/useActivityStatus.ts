import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";
import type { ActivityStatus } from "../../rpc/gen/rpc";
import { setIfNewer, useRpcQuery } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { useNotifications } from "../notifications/store";
import { useNativeActivity } from "./nativeState";

const KEY = ["activity.status", {}] as const;
/** Storage events carry a full revisioned snapshot; late query replies cannot replace it. */
export function useActivityStorage(): ActivityStatus | undefined {
  const { client, qc } = useRpcSession();
  useEffect(
    () =>
      client.subscribe(
        "activity.status_changed",
        (_rev, value) => {
          setIfNewer(qc, KEY, value);
        },
        () => {
          void qc.invalidateQueries({ queryKey: KEY });
        },
      ),
    [client, qc],
  );
  return useRpcQuery("activity.status", {}).data;
}
/** Controls derive from native state; storage health is a separate source from day/range lists. */
export function useActivityStatus() {
  const control = useNativeActivity();
  const storage = useActivityStorage();
  return { ...control, storage };
}
const trays = new WeakMap<object, string>();
export function useActivityTray(status: ActivityStatus | undefined) {
  const { client } = useRpcSession();
  const known = status !== undefined;
  const failed = status?.state === "storage_failed";
  const pending = status?.pending ?? 0;
  useEffect(() => {
    const key = `${failed}:${pending}`;
    if (!known || trays.get(client) === key) return;
    trays.set(client, key);
    void invoke("activity_tray", { storageFailed: failed, pending }).catch(() => {
      if (trays.get(client) === key) trays.delete(client);
    });
  }, [client, known, failed, pending]);
}
const failures = new WeakMap<object, { failed: boolean; epoch: number }>();
export function useActivityHealth() {
  const { client } = useRpcSession();
  const status = useActivityStorage();
  useActivityTray(status);
  useEffect(() => {
    if (!status) return;
    const old = failures.get(client) ?? { failed: false, epoch: 0 };
    const failed = status.state === "storage_failed";
    if (failed && !old.failed) {
      old.epoch += 1;
      useNotifications.getState().pushOnce({
        key: `activity:storage:${client.daemonId ?? "session"}:${old.epoch}`,
        kind: "error",
        title: "활동 기록 저장 중단",
        body: `${status.pending}건을 보관하고 있습니다. 저장 공간과 앱 진단을 확인하세요.`,
        target: { to: "/settings", search: { tab: "activity" } },
      });
    }
    old.failed = failed;
    failures.set(client, old);
  }, [client, status]);
  return status;
}
