import { useCallback, useEffect, useState } from "react";
import {
  repoPrStatus,
  repoPrList,
  repoPrCreate,
  repoLastCommit,
  repoBranches,
  repoPush,
  repoRemoteCancel,
  type RepoEntry,
  type PrStatus,
  type PrListItem,
} from "../api";
import { WorkspaceOperationError } from "../../transport";
import { openUrl } from "../../lib/openUrl";
import { useSourceAction } from "./useSourceAction";
interface Props {
  repo: RepoEntry;
  onBusyChange?: (busy: boolean) => void;
  onDirtyChange?: (dirty: boolean) => void;
}
export default function PullRequestPanel({ repo, onBusyChange, onDirtyChange }: Props) {
  const { busy, issue, execute } = useSourceAction(repo, onBusyChange);
  const [status, setStatus] = useState<PrStatus | null>(null);
  const [list, setList] = useState<PrListItem[]>([]);
  const [branches, setBranches] = useState<string[]>([]);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [base, setBase] = useState("");
  const [draft, setDraft] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [needsPush, setNeedsPush] = useState(false);
  const [created, setCreated] = useState<string | null>(null);
  const [linkError, setLinkError] = useState("");
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  const load = useCallback(
    async (signal: AbortSignal) => {
      const next = await repoPrStatus(repo.path);
      if (signal.aborted) return;
      setStatus(next);
      if (!next.available) {
        setList([]);
        return;
      }
      const [items, refs, last] = await Promise.all([
        repoPrList(repo.path),
        repoBranches(repo.path),
        repoLastCommit(repo.path).catch(() => null),
      ]);
      if (signal.aborted) return;
      setList(items);
      setBranches([
        ...new Set(
          refs.branches
            .filter((branch) => branch.remote && !branch.name.endsWith("/HEAD"))
            .map((branch) => branch.name.slice(branch.name.indexOf("/") + 1)),
        ),
      ]);
      if (last) setTitle((previous) => previous || last.message.split(/\r?\n/)[0]);
    },
    [repo.path],
  );
  useEffect(() => {
    void execute((_id, signal) => load(signal));
  }, [execute, load]);
  const create = () =>
    void execute(async (id, signal) => {
      setNeedsPush(false);
      try {
        const result = await repoPrCreate(repo.path, title, body, base || null, draft, id);
        if (!signal.aborted) {
          setCreated(result.url);
          setDirty(false);
        }
      } catch (cause) {
        if (!signal.aborted && cause instanceof WorkspaceOperationError && cause.code === "pr_branch_not_pushed")
          setNeedsPush(true);
        throw cause;
      }
    });
  const push = () =>
    void execute(async (id, signal) => {
      const cancel = () => {
        void repoRemoteCancel(id).catch(() => undefined);
      };
      signal.addEventListener("abort", cancel, { once: true });
      try {
        await repoPush(repo.path, id);
        if (!signal.aborted) setNeedsPush(false);
      } finally {
        signal.removeEventListener("abort", cancel);
      }
    });
  const open = (url: string) => {
    setLinkError("");
    void openUrl(url).catch(() => setLinkError("GitHub 주소를 열지 못했습니다."));
  };
  const pr = status?.pr;
  const state = pr ? ({ OPEN: "열림", CLOSED: "닫힘", MERGED: "병합됨" }[pr.state] ?? pr.state) : "";
  const review = pr
    ? ({ REVIEW_REQUIRED: "검토 필요", APPROVED: "승인됨", CHANGES_REQUESTED: "변경 요청" }[pr.reviewDecision ?? ""] ??
      "검토 정보 없음")
    : "";
  return (
    <section aria-label="GitHub PR" className="source-pr-panel">
      <h2>GitHub PR</h2>
      <button type="button" disabled={busy} onClick={() => void execute((_id, signal) => load(signal))}>
        PR 새로 고침
      </button>
      {issue && <p role="alert">{issue}</p>}
      {linkError && <p role="alert">{linkError}</p>}
      {status && !status.available && (
        <p>
          {status.reason === "gh_missing"
            ? "GitHub CLI(gh)를 설치하고 터미널에서 `gh auth login`을 실행해 주세요."
            : "`gh auth login`으로 로그인해 주세요."}
        </p>
      )}
      {pr && (
        <div>
          <p>{`#${pr.number} ${pr.title} · ${state} · ${review} · 검사 통과 ${pr.checks.passed} · 실패 ${pr.checks.failed} · 진행 중 ${pr.checks.pending}`}</p>
          {pr.isDraft && <p>초안 PR</p>}
          <button type="button" onClick={() => open(pr.url)}>
            GitHub에서 열기
          </button>
        </div>
      )}
      {status?.available && !pr && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            create();
          }}
        >
          <label>
            PR 제목
            <input
              value={title}
              maxLength={256}
              disabled={busy}
              onChange={(event) => {
                setTitle(event.target.value);
                setDirty(true);
              }}
              required
            />
          </label>
          <label>
            PR 본문
            <textarea
              value={body}
              disabled={busy}
              onChange={(event) => {
                setBody(event.target.value);
                setDirty(true);
              }}
            />
          </label>
          <label>
            대상 branch
            <select
              value={base}
              disabled={busy}
              onChange={(event) => {
                setBase(event.target.value);
                setDirty(true);
              }}
            >
              <option value="">저장소 기본 branch</option>
              {branches.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label>
            <input
              type="checkbox"
              checked={draft}
              disabled={busy}
              onChange={(event) => {
                setDraft(event.target.checked);
                setDirty(true);
              }}
            />
            초안
          </label>
          <button disabled={busy || !title.trim() || new TextEncoder().encode(body).length > 65536 || created !== null}>
            PR 만들기
          </button>
        </form>
      )}
      {needsPush && (
        <div>
          <p>먼저 push해 주세요. upstream이 없는 새 branch는 터미널에서 원격 branch를 지정해야 합니다.</p>
          <button type="button" disabled={busy} onClick={push}>
            push
          </button>
        </div>
      )}
      {created?.startsWith("https://") && (
        <a
          href={created}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(event) => {
            event.preventDefault();
            open(created);
          }}
        >
          만든 PR 열기
        </a>
      )}
      {status?.available && list.length > 0 && (
        <details>
          <summary>열린 PR {list.length}개</summary>
          <ul>
            {list.map((item) => (
              <li key={item.number}>
                <button type="button" onClick={() => open(item.url)}>
                  #{item.number} {item.title}
                </button>{" "}
                · {item.author} · {item.head}
                {item.isDraft ? " · 초안" : ""}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
