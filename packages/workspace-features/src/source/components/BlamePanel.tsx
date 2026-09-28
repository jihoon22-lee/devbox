import { useCallback, useEffect, useState } from "react";
import { repoBlame, type RepoEntry, type Blame } from "../api";
import { useSourceAction } from "./useSourceAction";
export interface BlameTarget {
  file: string;
  commitId: string | null;
  sequence: number;
}
interface Props {
  repo: RepoEntry;
  target?: BlameTarget | null;
  onShowCommit?: (id: string) => void;
  onBusyChange?: (busy: boolean) => void;
}
const date = new Intl.DateTimeFormat("ko-KR", { dateStyle: "medium" });
function authoredAt(seconds: number) {
  const value = new Date(seconds * 1000);
  return Number.isFinite(value.valueOf()) ? date.format(value) : "날짜 없음";
}
export default function BlamePanel({ repo, target, onShowCommit, onBusyChange }: Props) {
  const [path, setPath] = useState("");
  const [view, setView] = useState<Blame | null>(null);
  const { busy, issue, execute } = useSourceAction(repo, onBusyChange);
  const load = useCallback(
    async (file: string, commitId: string | null, signal: AbortSignal) => {
      const next = await repoBlame(repo.path, file, commitId);
      if (!signal.aborted) setView(next);
    },
    [repo.path],
  );
  useEffect(() => {
    setView(null);
    setPath(target?.file ?? "");
    if (target) void execute((_id, signal) => load(target.file, target.commitId, signal));
  }, [target, execute, load]);
  return (
    <section aria-label="작성 이력" className="source-blame-panel">
      <h2>작성 이력</h2>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void execute((_id, signal) => load(path.trim(), null, signal));
        }}
      >
        <label>
          파일 경로
          <input value={path} disabled={busy} onChange={(event) => setPath(event.target.value)} required />
        </label>
        <button disabled={busy || !path.trim()}>작성 이력 불러오기</button>
      </form>
      {issue && <p role="alert">{issue}</p>}
      {view?.truncated && <p>처음 10,000줄만 보여 줍니다.</p>}
      {view && (
        <div className="source-blame-lines">
          <table>
            <caption>{view.file} 줄별 작성 이력</caption>
            <thead>
              <tr>
                <th scope="col">Commit</th>
                <th scope="col">줄</th>
                <th scope="col">내용</th>
              </tr>
            </thead>
            <tbody>
              {view.lines.map((line, index) => {
                const metadata = view.commits[line.commit];
                const group = index === 0 || view.lines[index - 1].commit !== line.commit;
                const label = metadata
                  ? `${line.commit.slice(0, 8)} · ${metadata.author} · ${authoredAt(metadata.authorTime)} · ${metadata.summary}`
                  : line.commit.slice(0, 8);
                return (
                  <tr key={line.line}>
                    <td>
                      {group &&
                        (/^0+$/.test(line.commit) ? (
                          "커밋되지 않은 변경"
                        ) : onShowCommit ? (
                          <button type="button" onClick={() => onShowCommit(line.commit)}>
                            {label}
                          </button>
                        ) : (
                          label
                        ))}
                    </td>
                    <td>{line.line}</td>
                    <td>
                      <code>{line.text}</code>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
