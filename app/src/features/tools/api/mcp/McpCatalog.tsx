/**
 * 준비된 MCP 연결의 목록(도구·리소스·프롬프트)과 호출. 인자 JSON은 입력한 글자 그대로 보낸다(데몬이 객체·크기를
 * 검사한다). 결과는 데몬이 비밀 값을 가린 JSON이며 최근 것이 위다.
 */
import { useState } from "react";
import type { McpList } from "../../../../rpc/gen/rpc";
import { Button } from "../../../../ui";
import s from "../Api.module.css";
import type { McpState, Pending } from "./session";
import type { McpRequest } from "./useMcpSession";

const LISTS: { what: McpList; label: string }[] = [
  { what: "tools", label: "도구" },
  { what: "resources", label: "리소스" },
  { what: "prompts", label: "프롬프트" },
];

const pretty = (json: string) => {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
};

export function McpCatalog({ state, send }: { state: McpState; send: (input: McpRequest, pending: Pending) => void }) {
  const [what, setWhat] = useState<McpList>("tools");
  const [selected, setSelected] = useState<string | null>(null);
  const [args, setArgs] = useState("{}");
  const items = state[what];
  const item = items.find((i) => i.name === selected) ?? null;
  const list = (next: McpList, cursor: string | null) =>
    send({ type: "list", what: next, cursor }, { kind: "list", what: next, append: cursor !== null });

  function choose(next: McpList) {
    setWhat(next);
    setSelected(null);
    if (state[next].length === 0) list(next, null);
  }

  function run() {
    if (!item) return;
    if (what === "tools")
      send({ type: "call", name: item.name, argumentsJson: args }, { kind: "call", label: item.name });
    else if (what === "prompts")
      send({ type: "getPrompt", name: item.name, argumentsJson: args }, { kind: "call", label: item.name });
    else send({ type: "read", uri: item.name }, { kind: "call", label: item.name });
  }

  const action = what === "tools" ? "도구 호출" : what === "prompts" ? "프롬프트 가져오기" : "리소스 읽기";
  return (
    <div>
      <div className={s.bar} role="toolbar" aria-label="MCP 목록">
        {LISTS.map((l) => (
          <Button key={l.what} size="sm" variant={l.what === what ? "primary" : "ghost"} onClick={() => choose(l.what)}>
            {l.label}
          </Button>
        ))}
        <Button size="sm" variant="ghost" onClick={() => list(what, null)}>
          새로고침
        </Button>
      </div>
      <div role="listbox" aria-label="MCP 항목" className={s.tree}>
        {items.map((i) => (
          <div
            key={i.name}
            role="option"
            tabIndex={0}
            aria-selected={i.name === selected}
            className={s.treeItem}
            onClick={() => setSelected(i.name)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                setSelected(i.name);
              }
            }}
          >
            {i.name}
            {i.description && <span className={s.muted}> {i.description}</span>}
          </div>
        ))}
      </div>
      {state.next[what] && (
        <Button size="sm" variant="ghost" onClick={() => list(what, state.next[what] ?? null)}>
          더 보기
        </Button>
      )}
      {item && what !== "resources" && (
        <textarea
          className={`${s.input} ${s.mono}`}
          aria-label={what === "tools" ? "도구 인자 JSON" : "프롬프트 인자 JSON"}
          value={args}
          onChange={(event) => setArgs(event.target.value)}
        />
      )}
      {item && (
        <Button variant="primary" onClick={run}>
          {action}
        </Button>
      )}
      {state.results.length > 0 && (
        <section role="region" aria-label="MCP 결과">
          {state.results.map((r) => (
            <div key={r.id}>
              <h4>{r.label}</h4>
              {r.error ? (
                <p className={s.muted}>오류 {r.error}</p>
              ) : (
                <pre className={s.mono}>{pretty(r.json ?? "")}</pre>
              )}
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
