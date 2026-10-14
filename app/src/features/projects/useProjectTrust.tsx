import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { Methods } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { ProjectTrustDialog } from "./ProjectTrustDialog";

export type TrustContext = Methods["projects.trust_preview"]["params"];
type Preview = Methods["projects.trust_preview"]["result"];
export function isProjectTrustError(error: RpcError) {
  return (
    error.code === "agents.project_trust_required" ||
    error.code === "projects.trust_required" ||
    error.code === "projects.trust_preview_stale"
  );
}

export function useProjectTrust(target: string) {
  const { client, generation, state } = useRpcSession();
  const approve = useRpcMutation("projects.trust_approve");
  const [preview, setPreview] = useState<Preview>();
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState("");
  const version = useRef(0);
  const working = useRef(false);
  const mounted = useRef(true);
  const identity = `${target}:${generation}:${state}`;
  const previous = useRef(identity);
  const request = useRef<{ context: TrustContext; stamp: number; resolve: (approved: boolean) => void } | undefined>(
    undefined,
  );
  if (previous.current !== identity) {
    previous.current = identity;
    version.current++;
    request.current?.resolve(false);
    request.current = undefined;
  }
  const finish = (approved: boolean) => {
    version.current++;
    working.current = false;
    request.current?.resolve(approved);
    request.current = undefined;
    if (mounted.current) {
      setPreview(undefined);
      setPending(false);
      setNotice("");
    }
  };
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      version.current++;
      request.current?.resolve(false);
      request.current = undefined;
    };
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: Target and daemon session changes must clear the visible approval state.
  useEffect(() => {
    setPreview(undefined);
    setPending(false);
    setNotice("");
  }, [identity]);
  const live = (stamp: number) => mounted.current && request.current?.stamp === stamp && version.current === stamp;
  const load = async (context: TrustContext, stamp: number, changed = false) => {
    working.current = true;
    setPending(true);
    try {
      const value = await client.call("projects.trust_preview", context);
      if (!live(stamp)) return;
      working.current = false;
      setPending(false);
      if (!changed && (!value.requiresApproval || value.approved)) finish(true);
      else if (!value.config || !value.digest) {
        finish(false);
        setNotice("이 동작에는 저장소 명령 승인이 필요하지 않습니다. 동작을 다시 확인해 주세요.");
      } else {
        setPreview(value);
        if (changed) setNotice("프로젝트 경로나 설정이 바뀌었습니다. 새 내용을 확인한 뒤 다시 승인해 주세요.");
      }
    } catch (error) {
      if (!live(stamp)) return;
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = messageFor(failure.code, failure.detail);
      working.current = false;
      setPending(false);
      setNotice(`${message.title} ${message.action ?? ""}`);
      // Preview failures cannot authorize an operation. Keep an existing stale
      // dialog open for retry; a first-preview failure returns to its caller.
      if (!changed) {
        request.current?.resolve(false);
        request.current = undefined;
      }
    }
  };
  const ensure = (context: TrustContext): Promise<boolean> => {
    finish(false);
    const frozen = { ...context, worktrees: [...context.worktrees] };
    const stamp = version.current;
    return new Promise((resolve) => {
      request.current = { context: frozen, stamp, resolve };
      void load(frozen, stamp);
    });
  };
  const confirm = async () => {
    const active = request.current;
    if (!active || !preview || working.current) return;
    const value = preview;
    if (!value.digest || !value.config) return;
    working.current = true;
    setPending(true);
    try {
      await approve.mutateAsync({
        id: value.projectId,
        expectedCanonicalPath: value.canonicalPath,
        expectedDigest: value.digest,
        expectedScopeVersion: value.scopeVersion,
      });
      if (live(active.stamp)) finish(true);
    } catch (error) {
      if (!live(active.stamp)) return;
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      if (failure.code === "projects.trust_preview_stale") {
        setPreview(undefined);
        setNotice("프로젝트 경로나 설정이 바뀌었습니다. 새 내용을 불러오는 중…");
        await load(active.context, active.stamp, true);
      } else {
        const message = messageFor(failure.code, failure.detail);
        working.current = false;
        setPending(false);
        setNotice(`${message.title} ${message.action ?? ""}`);
      }
    }
  };
  return {
    ensure,
    cancel: () => finish(false),
    pending: pending || !!request.current,
    notice: !request.current ? notice : "",
    dialog:
      request.current && (preview || notice) ? (
        <ProjectTrustDialog
          preview={preview}
          pending={pending}
          notice={notice}
          onCancel={() => finish(false)}
          onApprove={() => void confirm()}
          onRetry={() => {
            const active = request.current;
            if (active) void load(active.context, active.stamp, true);
          }}
        />
      ) : null,
  };
}
