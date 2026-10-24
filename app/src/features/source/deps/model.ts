import { useCallback, useEffect, useRef, useState } from "react";
import type { AuditJob, AuditReport, Dependency, Ecosystem } from "../../../rpc/gen/rpc";
import { useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { useRpc, useRpcSession } from "../../../rpc/RpcProvider";
export const CACHE_MS = 30 * 60 * 1000;
export function normalizedName(ecosystem: Ecosystem, name: string) {
  if (ecosystem === "python") return name.toLowerCase().replace(/[-_.]+/g, "-");
  if (ecosystem === "cargo") return name.toLowerCase().replaceAll("_", "-");
  return name;
}
export function audited(report: AuditReport | undefined, dependency: Dependency, version: string) {
  return report?.packages.find(
    (p) =>
      p.ecosystem === dependency.ecosystem &&
      p.name === normalizedName(dependency.ecosystem, dependency.name) &&
      p.version === version,
  );
}
export function useDependencyLens(projectId: string) {
  const client = useRpc(),
    { state } = useRpcSession();
  const inventory = useRpcQuery("deps.list", { projectId });
  const mutation = useRpcMutation("deps.audit");
  const [job, setJob] = useState<AuditJob>();
  const jobRef = useRef<AuditJob | undefined>(undefined);
  const [notice, setNotice] = useState("");
  const [starting, setStarting] = useState(false);
  const busy = useRef(false),
    mounted = useRef(true),
    serial = useRef(0);
  const [now, setNow] = useState(Date.now);
  const accept = useCallback((value: AuditJob) => {
    jobRef.current = value;
    setJob(value);
  }, []);
  const refetch = inventory.refetch;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      serial.current++;
    };
  }, []);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  const refresh = useCallback(async () => {
    const current = jobRef.current;
    if (!current) return;
    const request = ++serial.current;
    try {
      const value = await client.call("deps.audit_status", { projectId, id: current.id });
      if (!mounted.current || serial.current !== request || jobRef.current?.id !== value.id) return;
      accept(value);
      setNotice("");
      setNow(Date.now());
      if (value.state !== "running") void refetch();
    } catch {
      if (mounted.current && serial.current === request) {
        setNotice("조회 상태를 확인하지 못했습니다. 다시 시도하세요.");
        accept({
          ...current,
          state: "failed",
          error: "network",
          result: null,
          lastSuccess: current.result ?? current.lastSuccess,
        });
      }
    }
  }, [accept, client, projectId, refetch]);
  useEffect(
    () =>
      client.subscribe(
        "deps.audit_changed",
        (_rev, event) => {
          if (event.projectId !== projectId || event.id !== jobRef.current?.id) return;
          if (event.state === "running") accept({ ...jobRef.current, completed: event.completed, total: event.total });
          else void refresh();
        },
        () => {
          void refresh();
          void refetch();
        },
      ),
    [accept, client, projectId, refresh, refetch],
  );
  useEffect(() => {
    if (state === "ready") void refresh();
  }, [state, refresh]);
  useEffect(() => {
    if (!job?.id || job.state !== "running" || state !== "ready") return;
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => clearInterval(timer);
  }, [job?.id, job?.state, refresh, state]);
  const start = async () => {
    if (busy.current || jobRef.current?.state === "running" || state !== "ready") return;
    busy.current = true;
    setStarting(true);
    setNotice("");
    serial.current++;
    try {
      const next = await mutation.mutateAsync({ projectId });
      if (mounted.current) {
        accept(next);
        setNow(Date.now());
      }
    } catch {
      if (mounted.current) setNotice("조회 실패 · 다시 시도하세요. 마지막 성공 결과는 보존합니다.");
    } finally {
      busy.current = false;
      if (mounted.current) setStarting(false);
    }
  };
  return { inventory, job, notice, starting, start, now, connected: state === "ready" };
}
