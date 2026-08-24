import { useCallback, useEffect, useState } from "react";
import { buildRevealedCurl, sanitizePersistedJson, sealSecret, sendRequest } from "./api";
import { addEntry, emptyStore as emptyCollectionStore, foldersOf, migrateCollections, removeEntry, saveStore } from "./lib/collections";
import {
  addEnvironment,
  loadStore as loadEnvStore,
  removeEnvironment,
  saveStore as saveEnvStore,
  setVariable,
} from "./lib/environments";
import {
  containsReference,
  emptyHistoryStore,
  migrateHistoryStorage,
  sanitizeRequestForPersistence,
  saveHistoryStore,
  toRequestTemplate,
  type HistoryStore,
} from "./lib/persistence";
import type { ApiResponse, HistoryItem, KeyValue, RequestTemplate } from "./types";
import "./App.css";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const BODY_KINDS = ["none", "json", "form", "raw"];
const AUTH_KINDS = ["none", "basic", "bearer", "apikey"];

const emptyReq = (): RequestTemplate => ({
  method: "GET",
  url: "",
  headers: [],
  params: [],
  body_kind: "none",
  body: "",
  auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
  timeout_ms: 10000,
});

function KeyValueEditor({
  rows,
  onChange,
  namePlaceholder,
}: {
  rows: KeyValue[];
  onChange: (rows: KeyValue[]) => void;
  namePlaceholder: string;
}) {
  const update = (i: number, patch: Partial<KeyValue>) => {
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  };
  return (
    <div className="kv-editor">
      {rows.map((r, i) => (
        <div className="kv-row" key={i}>
          <input
            placeholder={namePlaceholder}
            value={r.key}
            onChange={(e) => update(i, { key: e.currentTarget.value })}
            spellCheck={false}
          />
          <input
            placeholder="Value"
            value={r.value}
            onChange={(e) => update(i, { value: e.currentTarget.value })}
            spellCheck={false}
          />
          <button className="kv-del" onClick={() => onChange(rows.filter((_, idx) => idx !== i))}>
            ✕
          </button>
        </div>
      ))}
      <button className="btn kv-add" onClick={() => onChange([...rows, { key: "", value: "" }])}>
        + Add
      </button>
    </div>
  );
}

export function statusClass(status: number) {
  if (status >= 200 && status < 300) return "status-2xx";
  if (status >= 400) return "status-4xx";
  return "status-other";
}

export default function App() {
  const [req, setReq] = useState<RequestTemplate>(emptyReq);
  const [resp, setResp] = useState<ApiResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [showCurl, setShowCurl] = useState(false);
  const [tab, setTab] = useState<"params" | "headers" | "body" | "auth">("params");
  const [showHeaders, setShowHeaders] = useState(false);
  const [pretty, setPretty] = useState(true);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [collections, setCollections] = useState(emptyCollectionStore);
  const [collName, setCollName] = useState("");
  const [collFolder, setCollFolder] = useState("");
  const [collFilter, setCollFilter] = useState("");
  const [collSaving, setCollSaving] = useState(false);
  const [envStore, setEnvStore] = useState(loadEnvStore);
  const [currentEnvId, setCurrentEnvId] = useState("");
  const [envName, setEnvName] = useState("");
  const [migrationNotice, setMigrationNotice] = useState<string | null>(null);
  const [persistenceWarning, setPersistenceWarning] = useState<string | null>(null);
  const [persistenceReady, setPersistenceReady] = useState(false);

  const currentEnv = envStore.environments.find((e) => e.id === currentEnvId) ?? null;

  const persistEnvs = (store: ReturnType<typeof loadEnvStore>) => {
    setEnvStore(store);
    saveEnvStore(store);
  };

  const onCreateEnv = () => {
    const next = addEnvironment(envStore, envName, () => `e-${Date.now()}-${Math.floor(Math.random() * 1e6)}`);
    persistEnvs(next);
    setCurrentEnvId(next.environments[0].id);
    setEnvName("");
  };

  const environmentVariables = envStore.environments.flatMap((environment) => environment.variables);
  const sanitizeForPersistence = (serialized: string) =>
    sanitizePersistedJson(serialized, environmentVariables);

  const persistCollections = async (store: ReturnType<typeof emptyCollectionStore>) => {
    const safe = await saveStore(store, sanitizeForPersistence);
    setCollections(safe);
    return safe;
  };

  const onSaveCollection = async () => {
    setCollSaving(true);
    setPersistenceWarning(null);
    try {
      const next = addEntry(
        collections,
        { name: collName, folder: collFolder, request: req },
        Date.now(),
        () => `c-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      );
      await persistCollections(next);
      setCollName("");
      setCollFolder("");
    } catch {
      setPersistenceWarning("민감정보 안전 검증에 실패해 Collection을 저장하지 않았습니다.");
    } finally {
      setCollSaving(false);
    }
  };

  useEffect(() => {
    const historyMigration = migrateHistoryStorage();
    const initialVariables = envStore.environments.flatMap((environment) => environment.variables);
    let historyTask: Promise<void>;
    if (historyMigration.failed) {
      setPersistenceWarning("이전 History 삭제를 완료하지 못했습니다. 원본은 격리되며 다음 실행에서 재시도합니다.");
      historyTask = Promise.resolve();
    } else {
      historyTask = saveHistoryStore(
        historyMigration.store,
        (serialized) => sanitizePersistedJson(serialized, initialVariables),
      ).then((safe) => {
        setHistory(safe.history);
        if (historyMigration.migrated) {
          setMigrationNotice(`안전을 확인할 수 없는 이전 History ${historyMigration.removedLegacyEntries}건을 제거했습니다.`);
        }
      }).catch(() => {
        setHistory([]);
        setPersistenceWarning("History v2 안전 검증을 완료하지 못해 내용을 격리했습니다. 다음 실행에서 재시도합니다.");
      });
    }

    const collectionTask = migrateCollections((serialized) => sanitizePersistedJson(serialized, initialVariables)).then((migration) => {
      setCollections(migration.store);
      if (migration.failed) {
        setPersistenceWarning("이전 Collection 안전 변환을 완료하지 못했습니다. 원본은 격리되며 다음 실행에서 재시도합니다.");
      } else if (migration.migrated) {
        setMigrationNotice((current) =>
          [current, `이전 Collection을 v2로 안전 변환했습니다(검토 필요 ${migration.removedLegacyEntries}건).`]
            .filter(Boolean)
            .join(" "),
        );
      }
    });
    void Promise.allSettled([historyTask, collectionTask]).then(() => setPersistenceReady(true));
    // 최초 실행 migration은 시작 시점의 봉인 환경 snapshot으로 한 번만 검증한다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const persistHistoryRequest = useCallback(async (status?: number) => {
    const item: HistoryItem = {
      id: String(Date.now()),
      saved_at: Date.now(),
      request: sanitizeRequestForPersistence(req),
      status,
    };
    const candidate: HistoryStore = {
      ...emptyHistoryStore(),
      history: [item, ...history].slice(0, 50),
    };
    const safe = await saveHistoryStore(candidate, sanitizeForPersistence);
    setHistory(safe.history);
  }, [history, req, environmentVariables]);

  const onSend = async () => {
    setSending(true);
    setError(null);
    try {
      const result = await sendRequest(req, currentEnv?.variables ?? []);
      setResp(result);
      try {
        await persistHistoryRequest(result.status);
      } catch {
        setPersistenceWarning("요청은 완료됐지만 민감정보 안전 검증에 실패해 History를 저장하지 않았습니다.");
      }
    } catch {
      setError("요청에 실패했습니다. URL, 연결 상태와 secret 설정을 확인하세요.");
      setResp(null);
      try {
        await persistHistoryRequest();
      } catch {
        setPersistenceWarning("실패한 요청은 민감정보 안전 검증을 통과하지 못해 History에 저장하지 않았습니다.");
      }
    } finally {
      setSending(false);
    }
  };

  const setAuth = (patch: Partial<NonNullable<RequestTemplate["auth"]>>) =>
    setReq({
      ...req,
      auth: {
        kind: "none",
        username: "",
        password: "",
        token: "",
        api_key: "",
        api_value: "",
        ...req.auth,
        ...patch,
      },
    });

  const responseText = resp?.is_json && pretty ? tryPretty(resp.body) : resp?.body ?? "";

  return (
    <div className="app">
      <aside className="sidebar">
        <h1 className="app-title">API Playground</h1>
        <div className="group-name">History</div>
        {history.map((h) => (
          <button
            key={h.id}
            className="history-item"
            onClick={() => {
              setReq(toRequestTemplate(h.request));
              if (h.request.requiresSecretReview) {
                setPersistenceWarning("마스킹된 History입니다. 민감한 값을 환경 변수 참조로 다시 설정하세요.");
              }
              setResp(null);
            }}
          >
            <span className={`method ${h.request.method.toLowerCase()}`}>{h.request.method}</span>
            <span className="history-url" title={h.request.url}>
              {h.request.url || "(no url)"}
            </span>
          </button>
        ))}
        {history.length === 0 && <div className="dim">No requests yet</div>}

        <div className="group-name">Collections</div>
        <div className="coll-save-row">
          <input
            className="coll-input"
            placeholder="저장 이름"
            value={collName}
            onChange={(e) => setCollName(e.currentTarget.value)}
          />
          <input
            className="coll-input"
            placeholder="폴더 (선택)"
            value={collFolder}
            onChange={(e) => setCollFolder(e.currentTarget.value)}
          />
          <button className="btn" disabled={!persistenceReady || collSaving || !req.url.trim()} onClick={() => void onSaveCollection()}>
            Save
          </button>
        </div>
        {foldersOf(collections).length > 0 && (
          <select
            className="coll-filter"
            value={collFilter}
            onChange={(e) => setCollFilter(e.currentTarget.value)}
          >
            <option value="">모든 폴더</option>
            {foldersOf(collections).map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
        )}
        {collections.collections
          .filter((c) => !collFilter || c.folder === collFilter)
          .map((c) => (
            <div key={c.id} className="history-item coll-item" title={`${c.folder ? `[${c.folder}] ` : ""}${c.name}`}>
              <button
                className="coll-open"
                onClick={() => {
                  setReq(toRequestTemplate(c.request));
                  if (c.requiresSecretReview) {
                    setPersistenceWarning("안전 변환된 Collection입니다. 마스킹된 값을 환경 변수 참조로 다시 설정하세요.");
                  }
                  setResp(null);
                }}
              >
                <span className={`method ${c.request.method.toLowerCase()}`}>{c.request.method}</span>
                <span className="history-url">{c.folder ? `[${c.folder}] ` : ""}{c.name}</span>
              </button>
              <button className="coll-del" onClick={() => {
                void persistCollections(removeEntry(collections, c.id)).catch(() =>
                  setPersistenceWarning("Collection 삭제 상태를 안전하게 저장하지 못했습니다."),
                );
              }}>
                ✕
              </button>
            </div>
          ))}
        {collections.collections.length === 0 && <div className="dim">저장된 collection 없음</div>}

        <div className="group-name">Environments</div>
        <div className="coll-save-row">
          <input
            className="coll-input"
            placeholder="환경 이름 (예: dev)"
            value={envName}
            onChange={(e) => setEnvName(e.currentTarget.value)}
          />
          <button className="btn" onClick={onCreateEnv}>
            추가
          </button>
        </div>
        {envStore.environments.map((env) => (
          <div key={env.id} className={`env-item ${env.id === currentEnvId ? "active" : ""}`}>
            <button className="env-name" onClick={() => setCurrentEnvId(env.id)}>
              {env.name}
            </button>
            <button className="coll-del" onClick={() => persistEnvs(removeEnvironment(envStore, env.id))}>
              ✕
            </button>
          </div>
        ))}
        {currentEnv && (
          <div className="env-vars">
            {currentEnv.variables.map((v) => (
              <div key={v.key} className="env-var-row">
                <span className="env-var-key">{v.key}</span>
                {v.secret ? (
                  <>
                    <span className="env-var-secret" title="봉인됨 — 평문 미보관">
                      ••••••••
                    </span>
                    <button className="btn mini" onClick={() => {
                      const plain = window.prompt(`${v.key} 새 값 입력`);
                      if (plain != null) {
                        void sealSecret(plain)
                          .then((blob) => persistEnvs(setVariable(envStore, currentEnv.id, v.key, blob, true)))
                          .catch(() => setError("secret 봉인에 실패했습니다. 데스크톱 앱에서 다시 시도하세요."));
                      }
                    }}>
                      변경
                    </button>
                    <button className="btn mini" title="secret 해제 (저장 값 삭제)" onClick={() => persistEnvs(setVariable(envStore, currentEnv.id, v.key, "", false))}>
                      해제
                    </button>
                  </>
                ) : (
                  <>
                    <input
                      className="coll-input"
                      value={v.value}
                      onChange={(e) => persistEnvs(setVariable(envStore, currentEnv.id, v.key, e.currentTarget.value, false))}
                    />
                    <button className="btn mini" title="이 변수를 봉인해 secret으로 저장" onClick={() => {
                      if (v.value) {
                        void sealSecret(v.value)
                          .then((blob) => persistEnvs(setVariable(envStore, currentEnv.id, v.key, blob, true)))
                          .catch(() => setError("secret 봉인에 실패했습니다. 데스크톱 앱에서 다시 시도하세요."));
                      }
                    }}>
                      🔒
                    </button>
                  </>
                )}
              </div>
            ))}
            {currentEnv.variables.length === 0 && <div className="dim">변수 없음 — {"{{var}}"}를 요청에 쓰세요.</div>}
            <div className="env-add-var">
              <button
                className="btn"
                onClick={() => {
                  const name = `var${currentEnv.variables.length + 1}`;
                  persistEnvs(setVariable(envStore, currentEnv.id, name, "", false));
                }}
              >
                + 변수
              </button>
            </div>
          </div>
        )}
      </aside>

      <main className="content">
        {migrationNotice && <div className="migration-notice">{migrationNotice}</div>}
        {persistenceWarning && <div className="persistence-warning">{persistenceWarning}</div>}
        <div className="request-bar">
          <select className="method-select" value={req.method} onChange={(e) => setReq({ ...req, method: e.currentTarget.value })}>
            {METHODS.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <input
            className="url-input"
            placeholder="https://api.example.com/users"
            value={req.url}
            onChange={(e) => setReq({ ...req, url: e.currentTarget.value })}
            spellCheck={false}
          />
          <button className="btn send" onClick={() => void onSend()} disabled={!persistenceReady || sending || !req.url}>
            {!persistenceReady ? "Checking..." : sending ? "Sending..." : "Send"}
          </button>
          <button className={`btn ${showCurl ? "active" : ""}`} onClick={() => setShowCurl((v) => !v)} disabled={!req.url}>
            cURL
          </button>
        </div>

        {showCurl && (
          <div className="curl-panel">
            <div className="io-label">
              cURL
              <button className="copy-btn" onClick={() => void navigator.clipboard.writeText(buildCurl(req))}>
                마스킹 복사
              </button>
              <button className="copy-btn" onClick={() => void copyRevealedCurl(req, currentEnv?.variables ?? [], setError)}>
                원문 1회 복사
              </button>
            </div>
            <pre className="curl-text">{buildCurl(req) || " "}</pre>
          </div>
        )}

        <div className="tabs">
          {(["params", "headers", "body", "auth"] as const).map((t) => (
            <button key={t} className={`tab ${tab === t ? "active" : ""}`} onClick={() => setTab(t)}>
              {t.toUpperCase()}
            </button>
          ))}
        </div>

        <div className="tab-body">
          {tab === "params" && (
            <KeyValueEditor rows={req.params} onChange={(params) => setReq({ ...req, params })} namePlaceholder="Key" />
          )}
          {tab === "headers" && (
            <KeyValueEditor rows={req.headers} onChange={(headers) => setReq({ ...req, headers })} namePlaceholder="Header" />
          )}
          {tab === "body" && (
            <div>
              <select className="select-sm" value={req.body_kind} onChange={(e) => setReq({ ...req, body_kind: e.currentTarget.value })}>
                {BODY_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              {req.body_kind !== "none" && (
                <textarea
                  className="body-input"
                  rows={8}
                  placeholder={req.body_kind === "json" ? '{ "key": "value" }' : "key=value"}
                  value={req.body}
                  onChange={(e) => setReq({ ...req, body: e.currentTarget.value })}
                  spellCheck={false}
                />
              )}
            </div>
          )}
          {tab === "auth" && (
            <div className="auth-body">
              <select className="select-sm" value={req.auth?.kind ?? "none"} onChange={(e) => setAuth({ kind: e.currentTarget.value })}>
                {AUTH_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              {req.auth?.kind === "basic" && (
                <div className="kv-row">
                  <input placeholder="Username" value={req.auth.username} onChange={(e) => setAuth({ username: e.currentTarget.value })} />
                  <input placeholder="Password" type="password" value={req.auth.password} onChange={(e) => setAuth({ password: e.currentTarget.value })} />
                </div>
              )}
              {req.auth?.kind === "bearer" && (
                <div className="kv-row">
                  <input placeholder="Token" value={req.auth.token} onChange={(e) => setAuth({ token: e.currentTarget.value })} />
                </div>
              )}
              {req.auth?.kind === "apikey" && (
                <div className="kv-row">
                  <input placeholder="Header name (e.g. X-API-Key)" value={req.auth.api_key} onChange={(e) => setAuth({ api_key: e.currentTarget.value })} />
                  <input placeholder="Value" value={req.auth.api_value} onChange={(e) => setAuth({ api_value: e.currentTarget.value })} />
                </div>
              )}
            </div>
          )}
        </div>

        {error && <div className="error">{error}</div>}

        <div className="response">
          <div className="response-head">
            {resp ? (
              <>
                <span className={`status-badge ${statusClass(resp.status)}`}>
                  {resp.status} {resp.status_text}
                </span>
                <span className="dim">{resp.duration_ms}ms</span>
                <span className="dim">{(resp.size_bytes / 1024).toFixed(2)} KB</span>
                <span className="spacer" />
                {resp.is_json && (
                  <label className="toggle">
                    <input type="checkbox" checked={pretty} onChange={(e) => setPretty(e.currentTarget.checked)} />
                    pretty
                  </label>
                )}
                <button className="btn" onClick={() => void navigator.clipboard.writeText(responseText)}>
                  Copy
                </button>
                <button className="btn" onClick={() => setShowHeaders((v) => !v)}>
                  {showHeaders ? "Hide" : "Show"} headers
                </button>
              </>
            ) : (
              <span className="dim">Send a request to see the response</span>
            )}
          </div>
          {showHeaders && resp && (
            <pre className="resp-headers">
              {resp.headers.map((h) => `${h.key}: ${h.value}`).join("\n")}
            </pre>
          )}
          <pre className="resp-body">{responseText || " "}</pre>
        </div>
      </main>
    </div>
  );
}

export function tryPretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}

/** 요청 구성을 기본 마스킹된 curl 명령으로 만든다. */
export function buildCurl(template: RequestTemplate): string {
  if (!template.url) return "";
  const req = sanitizeRequestForPersistence(template);

  const params = new URLSearchParams();
  for (const p of req.params) if (p.key) params.append(p.key, p.value);
  const sep = req.url.includes("?") ? "&" : "?";
  const url = params.size ? req.url + sep + params.toString() : req.url;

  const lines = [`curl --request ${req.method} ${shellQuote(url)}`];

  const headers: [string, string][] = [];
  for (const h of req.headers) if (h.key) headers.push([h.key, h.value]);
  if (req.auth?.kind === "basic" && req.auth.username) {
    headers.push(["Authorization", "Basic [REDACTED]"]);
  } else if (req.auth?.kind === "bearer" && req.auth.token) {
    headers.push(["Authorization", `Bearer ${containsReference(req.auth.token) ? req.auth.token : "[REDACTED]"}`]);
  } else if (req.auth?.kind === "apikey" && req.auth.api_key) {
    headers.push([req.auth.api_key, containsReference(req.auth.api_value) ? req.auth.api_value : "[REDACTED]"]);
  }
  for (const [k, v] of headers) {
    lines.push(`  --header ${shellQuote(`${k}: ${v}`)}`);
  }

  if (req.body_kind !== "none" && req.body) {
    lines.push(`  --data ${shellQuote(req.body)}`);
  }

  return lines.join(" \\\n");
}

export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

async function copyRevealedCurl(
  req: RequestTemplate,
  environment: Parameters<typeof buildRevealedCurl>[1],
  setError: (message: string | null) => void,
): Promise<void> {
  const confirmed = window.confirm(
    "원문 cURL에는 Authorization, Cookie, API key와 secret 값이 포함될 수 있습니다. 클립보드에 한 번 복사할까요?",
  );
  if (!confirmed) return;
  try {
    const revealed = await buildRevealedCurl(req, environment);
    await navigator.clipboard.writeText(revealed);
  } catch {
    setError("원문 cURL을 안전하게 만들거나 복사하지 못했습니다.");
  }
}
