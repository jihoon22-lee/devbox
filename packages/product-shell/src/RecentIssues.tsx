import { useState, useSyncExternalStore } from "react";
import { diagnosticText, recentIssues, subscribeIssues } from "./issues";
import { version as workspaceVersion } from "../../../apps/devbox-workspace/package.json";
import { version as knowledgeVersion } from "../../../apps/devbox-knowledge/package.json";
import { version as studioVersion } from "../../../apps/devbox-api-studio/package.json";
import { version as controlVersion } from "../../../apps/devbox-control-center/package.json";
const versions: Record<string, string> = {
  workspace: workspaceVersion,
  knowledge: knowledgeVersion,
  "api-studio": studioVersion,
  "control-center": controlVersion,
};
export function RecentIssues({ product }: { product: string }) {
  const issues = useSyncExternalStore(subscribeIssues, recentIssues, recentIssues);
  const [notice, setNotice] = useState("");
  const visible = issues.filter((issue) => issue.product === product);
  return (
    <section className="shell-recent-issues" aria-label="최근 오류">
      <h3>최근 오류</h3>
      {visible.length === 0 ? (
        <p>최근 오류가 없습니다.</p>
      ) : (
        <ul>
          {visible.map((issue, index) => (
            <li key={`${issue.requestId}:${issue.occurredAt}:${index}`}>
              <code>{issue.code}</code>
              <span>
                {issue.component} · {issue.method}
              </span>
              <time dateTime={issue.occurredAt}>{new Date(issue.occurredAt).toLocaleString()}</time>
              <button
                type="button"
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(diagnosticText(issue, versions[product] ?? ""));
                    setNotice("진단 정보를 복사했습니다.");
                  } catch {
                    setNotice("진단 정보를 복사하지 못했습니다.");
                  }
                }}
              >
                진단 복사
              </button>
            </li>
          ))}
        </ul>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
