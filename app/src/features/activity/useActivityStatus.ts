import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";
import type { ActivityStatus } from "../../rpc/gen/rpc";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";

const KEYS = [["activity.status", {}]] as const;

/** Storage health, its own query so list reads cannot overwrite it (01 §9.1-5). */
export function useActivityStatus(): ActivityStatus | undefined {
  useTopicInvalidation("activity.status_changed", KEYS);
  return useRpcQuery("activity.status", {}).data;
}

/** Mirrors a storage failure into the tray ("활동 저장 중단 · n건 대기"). */
export function useActivityTray(status: ActivityStatus | undefined) {
  const known = status !== undefined;
  const failed = status?.state === "storage_failed";
  const pending = status?.pending ?? 0;
  useEffect(() => {
    if (!known) return;
    void invoke("activity_tray", { storageFailed: failed, pending }).catch((error: unknown) =>
      console.error("트레이 활동 상태를 바꾸지 못했습니다", error),
    );
  }, [known, failed, pending]);
}
