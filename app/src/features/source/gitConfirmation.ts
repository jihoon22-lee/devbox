import type { RpcClient } from "../../rpc/client";
import { type GitAction, gitError, useSourceGit } from "./gitState";
/** Refresh only the same selected identities; never add new cleanup candidates implicitly. */
export async function refreshGitConfirmation(client: RpcClient, id: string) {
  const current = useSourceGit.getState().confirmation;
  if (!current || current.id !== id || current.busy) return;
  useSourceGit.setState({ confirmation: { ...current, busy: true } });
  const { action } = current,
    { target } = action.params;
  try {
    let next: GitAction;
    switch (action.method) {
      case "git.cleanup": {
        const fresh = await client.call("git.cleanup_candidates", { target });
        const selected = action.params.candidates.map((old) =>
          fresh.find((item) => item.branch === old.branch && item.path === old.path),
        );
        if (selected.some((item) => !item)) throw new Error("protected");
        next = { method: action.method, params: { target, candidates: selected.map((item) => item!) } };
        break;
      }
      case "git.stage":
      case "git.unstage":
      case "git.discard": {
        const fresh = await client.call("git.diff", {
          target,
          path: action.params.path,
          staged: action.method === "git.unstage",
        });
        if (action.params.hunk && !fresh.hunks.some((hunk) => hunk.id === action.params.hunk))
          throw new Error("changed hunk");
        next = { method: action.method, params: { ...action.params, expect: fresh.expect } };
        break;
      }
      case "git.fetch":
        next = action;
        break;
      default: {
        const fresh = await client.call("git.status", { target });
        if (action.method === "git.push" || action.method === "git.pull") {
          next = { ...action, params: { ...action.params, expect: fresh.expect.state } } as GitAction;
        } else if (action.method === "git.branch_delete") {
          const branches = await client.call("git.branches", { target });
          const branch = branches.find((item) => item.name === action.params.branch && !item.remote && !item.current);
          if (!branch) throw new Error("protected");
          next = { method: action.method, params: { ...action.params, oid: branch.oid, expect: fresh.expect } };
        } else next = { ...action, params: { ...action.params, expect: fresh.expect } } as GitAction;
      }
    }
    if (useSourceGit.getState().confirmation?.id === id)
      useSourceGit.setState({ confirmation: { id: crypto.randomUUID(), action: next } });
  } catch (error) {
    if (useSourceGit.getState().confirmation?.id === id)
      useSourceGit.setState({
        confirmation: {
          ...current,
          error:
            error instanceof Error && !("code" in error)
              ? "선택한 대상이 바뀌었거나 보존 대상입니다. 취소하고 목록에서 다시 선택하세요."
              : gitError(error),
        },
      });
  }
}
