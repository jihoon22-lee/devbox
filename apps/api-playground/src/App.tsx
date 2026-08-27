import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  buildRevealedCurl,
  copyRawResponseCookies,
  copyRawResponseHeaders,
  pickMultipartFile,
  sanitizePersistedJson,
  sealSecret,
  sendRequest,
} from "./api";
import { CookieEditor } from "./CookieEditor";
import { GraphqlEditor } from "./GraphqlEditor";
import { HeaderTable } from "./HeaderTable";
import { MultipartEditor } from "./MultipartEditor";
import { OpenApiImport } from "./OpenApiImport";
import { ResponseViewer, type RawResponseCopyKind } from "./ResponseViewer";
import {
  addEntry,
  duplicateEntry,
  emptyStore as emptyCollectionStore,
  foldersOf,
  migrateCollections,
  removeEntry,
  renameEntry,
  saveStore,
  type CollectionEntry,
} from "./lib/collections";
import {
  buildRequestItemContextMenu,
  duplicateHistoryItem,
  removeHistoryItem,
  renameHistoryItem,
} from "./lib/contextMenu";
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
import {
  buildCookieHeader,
  hasActiveCookieHeader,
  hasCookieSourceConflict,
  validateCookies,
} from "./lib/cookies";
import { isHeaderEnabled } from "./lib/headers";
import {
  buildGraphqlBody,
  buildGraphqlGetUrl,
  isGraphqlDerivedHeader,
  validateGraphqlDocument,
  validateGraphqlEndpoint,
  GRAPHQL_OPERATION_INVALID,
  GRAPHQL_VARIABLES_INVALID,
  MAX_GRAPHQL_OPERATION_NAME_BYTES,
  MAX_GRAPHQL_QUERY_BYTES,
  MAX_GRAPHQL_VARIABLES_BYTES,
  GRAPHQL_HEADER_BYTES_ERROR,
  GRAPHQL_HEADER_ROWS_ERROR,
  GRAPHQL_URL_TOO_LARGE,
  MIN_GRAPHQL_TIMEOUT_MS,
  MAX_GRAPHQL_TIMEOUT_MS,
  validateGraphqlHeaders,
  validateGraphqlParams,
} from "./lib/graphql";
import {
  isMultipartPartEnabled,
  isMultipartDerivedHeader,
  validateMultipartParts,
} from "./lib/multipart";
import type { ApiResponse, GraphqlRequest, HistoryItem, KeyValue, RequestTemplate } from "./types";
import { OPENAPI_LIMITS, type OpenApiOperationPreview } from "./lib/openapi";
import "./App.css";

export { statusClass } from "./ResponseViewer";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const BODY_KINDS = ["none", "json", "form", "multipart", "raw", "graphql"];
const AUTH_KINDS = ["none", "basic", "bearer", "apikey"];

const emptyReq = (): RequestTemplate => ({
  method: "GET",
  url: "",
  headers: [],
  cookies: [],
  multipart: [],
  params: [],
  body_kind: "none",
  body: "",
  auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
  timeout_ms: 10000,
  graphql: null,
});

const emptyGraphql = (): GraphqlRequest => ({ query: "", variables: "", operation_name: "" });

function graphqlConfigError(request: RequestTemplate): string | null {
  if (request.body_kind !== "graphql") return null;
  if (!request.graphql || !["GET", "POST"].includes(request.method)) {
    return "GraphQL 요청 구성이 올바르지 않습니다";
  }
  if (request.timeout_ms < MIN_GRAPHQL_TIMEOUT_MS || request.timeout_ms > MAX_GRAPHQL_TIMEOUT_MS) {
    return "GraphQL timeout이 허용된 범위를 벗어났습니다";
  }
  const encoder = new TextEncoder();
  if (encoder.encode(request.graphql.query).byteLength > MAX_GRAPHQL_QUERY_BYTES
    || encoder.encode(request.graphql.variables).byteLength > MAX_GRAPHQL_VARIABLES_BYTES
    || encoder.encode(request.graphql.operation_name).byteLength > MAX_GRAPHQL_OPERATION_NAME_BYTES) {
    return "GraphQL 요청 구성이 올바르지 않습니다";
  }
  try {
    validateGraphqlHeaders(request.headers);
    validateGraphqlParams(request.params);
    validateGraphqlEndpoint(request.url);
    validateGraphqlDocument(request.graphql.query, request.graphql.operation_name);
    buildGraphqlBody(request.graphql);
    if (request.method === "GET") buildGraphqlGetUrl(request.url, request.params, request.graphql);
    return null;
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : "";
    const safe = new Set([
      "GraphQL endpoint URL이 올바르지 않습니다",
      "GraphQL endpoint query에 credential을 넣을 수 없습니다",
      "GraphQL query가 허용된 크기를 초과했습니다",
      "GraphQL variables가 허용된 크기를 초과했습니다",
      "GraphQL 문서 형식이 올바르지 않습니다",
      GRAPHQL_OPERATION_INVALID,
      GRAPHQL_VARIABLES_INVALID,
      "GraphQL variables 구조가 허용된 한계를 초과했습니다",
      "GraphQL 요청 본문이 허용된 크기를 초과했습니다",
      "GraphQL introspection 요청은 지원하지 않습니다",
      "GraphQL subscription은 지원하지 않습니다",
      GRAPHQL_HEADER_ROWS_ERROR,
      GRAPHQL_HEADER_BYTES_ERROR,
      GRAPHQL_URL_TOO_LARGE,
    ]);
    return safe.has(message) ? message : "GraphQL 요청 구성이 올바르지 않습니다";
  }
}

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

export default function App() {
  const [req, setReq] = useState<RequestTemplate>(emptyReq);
  const [resp, setResp] = useState<ApiResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [showCurl, setShowCurl] = useState(false);
  const [showOpenApiImport, setShowOpenApiImport] = useState(false);
  const [tab, setTab] = useState<"params" | "headers" | "cookies" | "body" | "auth">("params");
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
  const [contextActionBusy, setContextActionBusy] = useState(false);
  const [selectedHistoryId, setSelectedHistoryId] = useState<string | null>(null);
  const [selectedCollectionId, setSelectedCollectionId] = useState<string | null>(null);
  const [requestEditorRevision, setRequestEditorRevision] = useState(0);
  const openApiCollectionSavingRef = useRef(false);
  const [contextHistory, setContextHistory] = useState<HistoryItem | null>(null);
  const [contextCollection, setContextCollection] = useState<CollectionEntry | null>(null);
  const mountedRef = useRef(true);
  const requestSequenceRef = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      requestSequenceRef.current += 1;
      abortControllerRef.current?.abort();
    };
  }, []);

  const prepareHistoryContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.historyId;
    const item = history.find((candidate) => candidate.id === id);
    if (!item) return;
    setSelectedHistoryId(item.id);
    setContextHistory(item);
  }, [history]);
  const historyContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareHistoryContext(target),
  });

  const prepareCollectionContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.collectionId;
    const item = collections.collections.find((candidate) => candidate.id === id);
    if (!item) return;
    setSelectedCollectionId(item.id);
    setContextCollection(item);
  }, [collections.collections]);
  const collectionContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareCollectionContext(target),
  });

  const currentEnv = envStore.environments.find((e) => e.id === currentEnvId) ?? null;
  const cookieIssues = validateCookies(req.cookies);
  const cookieConflict = hasCookieSourceConflict(req.cookies, req.headers);
  const cookieConfigurationError = cookieConflict
    ? "활성 Cookie header와 구조화 Cookie 중 하나만 사용하세요."
    : cookieIssues[0]
      ? `${cookieIssues[0].index + 1}번 Cookie: ${cookieIssues[0].message}`
      : null;
  const multipartIssue = req.body_kind === "multipart"
    ? validateMultipartParts(req.multipart)[0] ?? null
    : null;
  const graphqlIssue = graphqlConfigError(req);
  const requestConfigurationError = graphqlIssue ?? cookieConfigurationError ?? (
    multipartIssue
      ? `${multipartIssue.index + 1}번 multipart part: ${multipartIssue.message}`
      : null
  );

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
    if (mountedRef.current) setCollections(safe);
    return safe;
  };

  const persistHistory = async (store: HistoryStore) => {
    const safe = await saveHistoryStore(store, sanitizeForPersistence);
    setHistory(safe.history);
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

  const onOpenApiApply = (operation: OpenApiOperationPreview) => {
    setReq(operation.request);
    setRequestEditorRevision((revision) => revision + 1);
    setTab(operation.request.body_kind !== "none" ? "body" : "params");
    setResp(null);
    setError(null);
    setPersistenceWarning(null);
  };

  const onOpenApiAddToCollection = async (operations: OpenApiOperationPreview[]) => {
    if (openApiCollectionSavingRef.current) throw new Error("OpenAPI Collection 저장이 이미 진행 중입니다");
    openApiCollectionSavingRef.current = true;
    setCollSaving(true);
    const timestamp = Date.now();
    let sequence = 0;
    let next = collections;
    const usedIds = new Set(next.collections.map((entry) => entry.id));
    try {
      // addEntry prepends new items; reverse the batch so the deterministic
      // preview order is retained in the Collection list.
      for (const operation of [...operations].reverse()) {
        const id = () => {
          let randomId = "";
          try {
            randomId = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
              ? `c-${crypto.randomUUID()}`
              : "";
          } catch {
            // A restricted WebView may expose randomUUID but reject it. The
            // collision-checked local fallback still keeps IDs unique.
            randomId = "";
          }
          let candidate = randomId;
          while (!candidate || usedIds.has(candidate)) {
            candidate = `c-openapi-${timestamp}-${sequence}`;
            sequence += 1;
          }
          usedIds.add(candidate);
          return candidate;
        };
        next = addEntry(
          next,
          { name: operation.label.slice(0, OPENAPI_LIMITS.maxCollectionNameLength), folder: "OpenAPI", request: operation.request },
          timestamp + sequence,
          id,
        );
      }
      await persistCollections(next);
    } finally {
      openApiCollectionSavingRef.current = false;
      if (mountedRef.current) setCollSaving(false);
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

  useEffect(() => {
    const id = contextHistory?.id;
    if (!id) return;
    const current = history.find((item) => item.id === id) ?? null;
    if (current) setContextHistory(current);
    else {
      historyContextMenu.close();
      setContextHistory(null);
      setSelectedHistoryId((selected) => selected === id ? null : selected);
    }
  }, [contextHistory?.id, history, historyContextMenu.close]);

  useEffect(() => {
    const id = contextCollection?.id;
    if (!id) return;
    const current = collections.collections.find((item) => item.id === id) ?? null;
    if (current) setContextCollection(current);
    else {
      collectionContextMenu.close();
      setContextCollection(null);
      setSelectedCollectionId((selected) => selected === id ? null : selected);
    }
  }, [collectionContextMenu.close, collections.collections, contextCollection?.id]);

  const persistHistoryRequest = useCallback(async (
    request: RequestTemplate,
    status: number | undefined,
    isCurrent: () => boolean,
  ) => {
    const item: HistoryItem = {
      id: String(Date.now()),
      saved_at: Date.now(),
      request: sanitizeRequestForPersistence(request),
      status,
    };
    const candidate: HistoryStore = {
      ...emptyHistoryStore(),
      history: [item, ...history].slice(0, 50),
    };
    const safe = await saveHistoryStore(candidate, sanitizeForPersistence);
    if (isCurrent()) setHistory(safe.history);
    return safe;
  }, [history, sanitizeForPersistence]);

  const onSend = async () => {
    if (sending || abortControllerRef.current) return;
    if (requestConfigurationError) {
      setError(requestConfigurationError);
      setTab(cookieConfigurationError ? "cookies" : "body");
      return;
    }
    const requestSnapshot = req;
    const environmentSnapshot = currentEnv?.variables ?? [];
    const controller = new AbortController();
    const sequence = requestSequenceRef.current + 1;
    requestSequenceRef.current = sequence;
    abortControllerRef.current = controller;
    setSending(true);
    setError(null);
    try {
      const result = await sendRequest(requestSnapshot, environmentSnapshot, controller.signal);
      if (!mountedRef.current || requestSequenceRef.current !== sequence) return;
      setResp(result);
      try {
        await persistHistoryRequest(
          requestSnapshot,
          result.status,
          () => mountedRef.current && requestSequenceRef.current === sequence,
        );
      } catch {
        if (mountedRef.current && requestSequenceRef.current === sequence) {
          setPersistenceWarning("요청은 완료됐지만 민감정보 안전 검증에 실패해 History를 저장하지 않았습니다.");
        }
      }
    } catch (cause) {
      if (!mountedRef.current || requestSequenceRef.current !== sequence) return;
      setError(safeRequestError(cause));
      setResp(null);
      try {
        await persistHistoryRequest(
          requestSnapshot,
          undefined,
          () => mountedRef.current && requestSequenceRef.current === sequence,
        );
      } catch {
        if (mountedRef.current && requestSequenceRef.current === sequence) {
          setPersistenceWarning("실패한 요청은 민감정보 안전 검증을 통과하지 못해 History에 저장하지 않았습니다.");
        }
      }
    } finally {
      if (mountedRef.current && requestSequenceRef.current === sequence) {
        setSending(false);
        abortControllerRef.current = null;
      }
    }
  };

  const onCancel = () => {
    if (!sending) return;
    requestSequenceRef.current += 1;
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setSending(false);
    setError("요청이 취소되었습니다");
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
  const copyRawResponse = (kind: RawResponseCopyKind, responseId: string) => (
    kind === "headers"
      ? copyRawResponseHeaders(responseId)
      : copyRawResponseCookies(responseId)
  );
  const contextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildRequestItemContextMenu(
      contextActionBusy || collSaving || sending || !persistenceReady,
    ),
    [collSaving, contextActionBusy, persistenceReady, sending],
  );

  const runContextAction = async (action: () => Promise<void>, failureMessage: string) => {
    setContextActionBusy(true);
    setPersistenceWarning(null);
    try {
      await action();
    } catch {
      setPersistenceWarning(failureMessage);
    } finally {
      setContextActionBusy(false);
    }
  };

  const copyMaskedCurl = (request: HistoryItem["request"]) => {
    void runContextAction(async () => {
      const curl = buildCurl(toRequestTemplate(request));
      if (!curl) throw new Error("invalid request");
      await navigator.clipboard.writeText(curl);
    }, "마스킹된 cURL을 복사하지 못했습니다.");
  };

  const duplicateHistory = (item: HistoryItem) => {
    void runContextAction(async () => {
      const next = duplicateHistoryItem(
        { ...emptyHistoryStore(), history },
        item.id,
        Date.now(),
        () => `h-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      );
      const safe = await persistHistory(next);
      setSelectedHistoryId(safe.history[0]?.id ?? null);
    }, "History 복제 상태를 안전하게 저장하지 못했습니다.");
  };

  const renameHistory = (item: HistoryItem) => {
    const name = window.prompt("History 이름 변경", item.name ?? item.request.url);
    if (name === null) return;
    if (!name.trim()) {
      setPersistenceWarning("History 이름은 비워둘 수 없습니다.");
      return;
    }
    void runContextAction(async () => {
      await persistHistory(renameHistoryItem(
        { ...emptyHistoryStore(), history },
        item.id,
        name,
      ));
    }, "History 이름을 안전하게 저장하지 못했습니다.");
  };

  const deleteHistory = (item: HistoryItem) => {
    const label = (item.name ?? item.request.url) || "(no url)";
    if (!window.confirm(`'${label}' History를 삭제할까요? 이 작업은 되돌릴 수 없습니다.`)) return;
    void runContextAction(async () => {
      await persistHistory(removeHistoryItem({ ...emptyHistoryStore(), history }, item.id));
    }, "History 삭제 상태를 안전하게 저장하지 못했습니다.");
  };

  const duplicateCollection = (item: CollectionEntry) => {
    void runContextAction(async () => {
      const safe = await persistCollections(duplicateEntry(
        collections,
        item.id,
        Date.now(),
        () => `c-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      ));
      setSelectedCollectionId(safe.collections[0]?.id ?? null);
    }, "Collection 복제 상태를 안전하게 저장하지 못했습니다.");
  };

  const renameCollection = (item: CollectionEntry) => {
    const name = window.prompt("Collection 이름 변경", item.name);
    if (name === null) return;
    if (!name.trim()) {
      setPersistenceWarning("Collection 이름은 비워둘 수 없습니다.");
      return;
    }
    void runContextAction(async () => {
      await persistCollections(renameEntry(collections, item.id, name));
    }, "Collection 이름을 안전하게 저장하지 못했습니다.");
  };

  const deleteCollection = (item: CollectionEntry) => {
    if (!window.confirm(`'${item.name}' Collection을 삭제할까요? 이 작업은 되돌릴 수 없습니다.`)) return;
    void runContextAction(async () => {
      await persistCollections(removeEntry(collections, item.id));
    }, "Collection 삭제 상태를 안전하게 저장하지 못했습니다.");
  };

  const onHistoryContextSelect = (id: string) => {
    const item = contextHistory;
    if (!item) return;
    if (id === "duplicate") duplicateHistory(item);
    else if (id === "rename") renameHistory(item);
    else if (id === "delete") deleteHistory(item);
    else if (id === "copy-curl") copyMaskedCurl(item.request);
  };

  const onCollectionContextSelect = (id: string) => {
    const item = contextCollection;
    if (!item) return;
    if (id === "duplicate") duplicateCollection(item);
    else if (id === "rename") renameCollection(item);
    else if (id === "delete") deleteCollection(item);
    else if (id === "copy-curl") copyMaskedCurl(item.request);
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <h1 className="app-title">API Playground</h1>
        <div className="group-name">History</div>
        {history.map((h) => (
          <button
            key={h.id}
            className={`history-item ${selectedHistoryId === h.id ? "selected" : ""}`}
            aria-current={selectedHistoryId === h.id ? "true" : undefined}
            aria-label={`History 항목: ${(h.name ?? h.request.url) || "(no url)"}`}
            data-history-id={h.id}
            onClick={() => {
              setSelectedHistoryId(h.id);
              setReq(toRequestTemplate(h.request));
              setRequestEditorRevision((revision) => revision + 1);
              if (h.request.requiresSecretReview) {
                setPersistenceWarning("마스킹된 History입니다. 민감한 값을 환경 변수 참조로 다시 설정하세요.");
              }
              setResp(null);
            }}
            {...historyContextMenu.triggerProps}
          >
            <span className={`method ${h.request.method.toLowerCase()}`}>{h.request.method}</span>
            <span className="history-url" title={h.name ?? h.request.url}>
              {(h.name ?? h.request.url) || "(no url)"}
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
          <button className="btn" disabled={!persistenceReady || collSaving || contextActionBusy || !req.url.trim()} onClick={() => void onSaveCollection()}>
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
            <div
              key={c.id}
              className={`history-item coll-item ${selectedCollectionId === c.id ? "selected" : ""}`}
              title={`${c.folder ? `[${c.folder}] ` : ""}${c.name}`}
              tabIndex={0}
              aria-current={selectedCollectionId === c.id ? "true" : undefined}
              aria-label={`Collection 항목: ${c.name}`}
              data-collection-id={c.id}
              onClick={() => setSelectedCollectionId(c.id)}
              {...collectionContextMenu.triggerProps}
            >
              <button
                className="coll-open"
                onClick={() => {
                  setSelectedCollectionId(c.id);
                  setReq(toRequestTemplate(c.request));
                  setRequestEditorRevision((revision) => revision + 1);
                  if (c.requiresSecretReview) {
                    setPersistenceWarning("안전 변환된 Collection입니다. 마스킹된 값을 환경 변수 참조로 다시 설정하세요.");
                  }
                  setResp(null);
                }}
              >
                <span className={`method ${c.request.method.toLowerCase()}`}>{c.request.method}</span>
                <span className="history-url">{c.folder ? `[${c.folder}] ` : ""}{c.name}</span>
              </button>
              <button
                className="coll-del"
                aria-label={`${c.name} Collection 삭제`}
                disabled={contextActionBusy || collSaving || sending || !persistenceReady}
                onClick={() => deleteCollection(c)}
              >
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
            {(req.body_kind === "graphql" ? ["GET", "POST"] : METHODS).map((m) => (
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
          <button className="btn send" onClick={() => sending ? onCancel() : void onSend()} disabled={!persistenceReady || contextActionBusy || (!sending && (!req.url || Boolean(requestConfigurationError)))}>
            {!persistenceReady ? "Checking..." : sending ? "Cancel" : "Send"}
          </button>
          <button className={`btn ${showCurl ? "active" : ""}`} onClick={() => setShowCurl((v) => !v)} disabled={!req.url || Boolean(requestConfigurationError)}>
            cURL
          </button>
          <button className="btn" type="button" onClick={() => setShowOpenApiImport(true)} disabled={!persistenceReady || sending || contextActionBusy || collSaving}>
            OpenAPI
          </button>
        </div>

        {showCurl && !requestConfigurationError && (
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
          {(["params", "headers", "cookies", "body", "auth"] as const).map((t) => (
            <button key={t} className={`tab ${tab === t ? "active" : ""}`} onClick={() => setTab(t)}>
              {t.toUpperCase()}
            </button>
          ))}
        </div>

        {cookieConfigurationError && (
          <div className="error" role="alert">{cookieConfigurationError}</div>
        )}
        {!cookieConfigurationError && multipartIssue && (
          <div className="error" role="alert">
            {multipartIssue.index + 1}번 multipart part: {multipartIssue.message}
          </div>
        )}

        <div className="tab-body">
          {tab === "params" && (
            <KeyValueEditor rows={req.params} onChange={(params) => setReq({ ...req, params })} namePlaceholder="Key" />
          )}
          {tab === "headers" && (
            <HeaderTable
              rows={req.headers}
              secretNames={(currentEnv?.variables ?? [])
                .filter((variable) => variable.secret)
                .map((variable) => variable.key)}
              onChange={(headers) => setReq({ ...req, headers })}
            />
          )}
          {tab === "cookies" && (
            <CookieEditor
              key={requestEditorRevision}
              rows={req.cookies}
              secretNames={(currentEnv?.variables ?? [])
                .filter((variable) => variable.secret)
                .map((variable) => variable.key)}
              hasRawCookieHeader={hasActiveCookieHeader(req.headers)}
              onChange={(cookies) => setReq({ ...req, cookies })}
            />
          )}
          {tab === "body" && (
            <div>
              <select
                className="select-sm"
                value={req.body_kind}
                onChange={(e) => {
                  const bodyKind = e.currentTarget.value;
                  setReq({
                    ...req,
                    body_kind: bodyKind,
                    method: bodyKind === "graphql" && !["GET", "POST"].includes(req.method)
                      ? "POST"
                      : req.method,
                    graphql: bodyKind === "graphql" ? req.graphql ?? emptyGraphql() : null,
                  });
                }}
              >
                {BODY_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              {req.body_kind === "graphql" ? (
                <GraphqlEditor
                  key={requestEditorRevision}
                  value={req.graphql ?? emptyGraphql()}
                  onChange={(graphql) => setReq({ ...req, graphql, body: "" })}
                />
              ) : req.body_kind === "multipart" ? (
                <MultipartEditor
                  key={requestEditorRevision}
                  rows={req.multipart}
                  secretNames={(currentEnv?.variables ?? [])
                    .filter((variable) => variable.secret)
                    .map((variable) => variable.key)}
                  onChange={(multipart) => setReq({ ...req, multipart })}
                  onPickFile={pickMultipartFile}
                />
              ) : req.body_kind !== "none" && (
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

        {graphqlIssue && <div className="error" role="alert">{graphqlIssue}</div>}
        {error && <div className="error" role="alert">{error}</div>}

        <ResponseViewer
          response={resp}
          responseText={responseText}
          pretty={pretty}
          onPrettyChange={setPretty}
          onRawCopy={copyRawResponse}
          onError={setError}
        />
      </main>
      <ContextMenu
        open={historyContextMenu.open}
        anchor={historyContextMenu.anchor}
        restoreFocusTo={historyContextMenu.restoreFocusTo}
        items={contextItems}
        onSelect={onHistoryContextSelect}
        onClose={historyContextMenu.close}
        ariaLabel="History 메뉴"
      />
      <ContextMenu
        open={collectionContextMenu.open}
        anchor={collectionContextMenu.anchor}
        restoreFocusTo={collectionContextMenu.restoreFocusTo}
        items={contextItems}
        onSelect={onCollectionContextSelect}
        onClose={collectionContextMenu.close}
        ariaLabel="Collection 메뉴"
      />
      {showOpenApiImport && (
        <OpenApiImport
          onClose={() => setShowOpenApiImport(false)}
          onApply={onOpenApiApply}
          onAddToCollection={onOpenApiAddToCollection}
        />
      )}
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
  if (
    validateCookies(template.cookies).length > 0 ||
    hasCookieSourceConflict(template.cookies, template.headers) ||
    (template.body_kind === "multipart" &&
      validateMultipartParts(template.multipart).some((issue) => issue.field !== "file"))
  ) {
    return "";
  }
  if (template.body_kind === "graphql") {
    try {
      validateGraphqlHeaders(template.headers);
      validateGraphqlParams(template.params);
      validateGraphqlEndpoint(template.url);
    } catch {
      return "";
    }
  }
  const req = sanitizeRequestForPersistence(template);
  if (req.body_kind === "graphql" && (!req.graphql || !["GET", "POST"].includes(req.method))) return "";
  const safeGraphql = req.graphql && (() => {
    try {
      buildGraphqlBody(req.graphql);
      return req.graphql;
    } catch {
      // A malformed or redacted variables draft remains visible in the editor, but
      // masked cURL uses an empty variables object instead of leaking raw text.
      return { ...req.graphql, variables: "{}" };
    }
  })();
  const safeGraphqlBody = req.body_kind === "graphql" && safeGraphql
    ? (() => {
      try {
        return buildGraphqlBody(safeGraphql);
      } catch {
        return JSON.stringify({
          ...(safeGraphql.operation_name ? { operationName: safeGraphql.operation_name } : {}),
          query: safeGraphql.query,
          variables: {},
        });
      }
    })()
    : "";

  const params = new URLSearchParams();
  for (const p of req.params) if (p.key) params.append(p.key, p.value);
  const sep = req.url.includes("?") ? "&" : "?";
  const url = req.body_kind === "graphql" && safeGraphql && req.method === "GET"
    ? (() => {
      try {
        return buildGraphqlGetUrl(req.url, req.params, safeGraphql);
      } catch {
        return "";
      }
    })()
    : params.size ? req.url + sep + params.toString() : req.url;
  if (!url) return "";

  const lines = [`curl --request ${req.method} ${shellQuote(url)}`];

  const headers: [string, string][] = [];
  for (const h of req.headers) {
    if (
      isHeaderEnabled(h) &&
      h.key &&
      !(req.body_kind === "multipart" && isMultipartDerivedHeader(h.key)) &&
      !(req.body_kind === "graphql" && isGraphqlDerivedHeader(h.key))
    ) {
      headers.push([h.key, h.value]);
    }
  }
  const cookieHeader = buildCookieHeader(req.cookies);
  if (cookieHeader) headers.push(["Cookie", cookieHeader]);
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

  if (req.body_kind === "multipart") {
    for (const part of req.multipart) {
      if (!isMultipartPartEnabled(part) || !part.name) continue;
      const suffix = part.content_type ? `;type=${part.content_type}` : "";
      const value = part.kind === "text"
        ? curlFormQuote(part.value)
        : `@${curlFormQuote(`[RESELECT_FILE:${part.file_name || "file"}]`)}`;
      lines.push(`  --form ${shellQuote(`${part.name}=${value}${suffix}`)}`);
    }
  } else if (req.body_kind === "graphql" && safeGraphql && req.method === "POST") {
    lines.push(`  --header ${shellQuote("Content-Type: application/json")}`);
    lines.push(`  --data ${shellQuote(safeGraphqlBody)}`);
  } else if (req.body_kind !== "none" && req.body_kind !== "graphql" && req.body) {
    lines.push(`  --data ${shellQuote(req.body)}`);
  }

  return lines.join(" \\\n");
}

function safeRequestError(cause: unknown): string {
  if ((typeof DOMException !== "undefined" && cause instanceof DOMException && cause.name === "AbortError")
    || (cause instanceof Error && cause.name === "AbortError")) {
    return "요청이 취소되었습니다";
  }
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/, "");
  const safeMessages = [
    "multipart는 최대 50개 part까지 사용할 수 있습니다.",
    "part 이름이 필요합니다.",
    "part 이름은 120자 이하의 HTTP token이어야 합니다.",
    "Content-Type은 type/subtype 형식이어야 합니다.",
    "전송할 파일을 선택하세요.",
    "선택한 파일 경로가 올바르지 않습니다.",
    "활성 text part 전체는 UTF-8 기준 1,000,000바이트 이하여야 합니다.",
    "multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다",
    "part별 Content-Type 전송은 데스크톱 앱에서만 사용할 수 있습니다",
    "선택한 multipart 파일을 찾을 수 없습니다",
    "선택한 multipart 파일을 읽을 수 없습니다",
    "multipart 파일은 각각 25 MiB 이하여야 합니다",
    "multipart 파일 전체는 50 MiB 이하여야 합니다",
    "요청 시간이 초과되었습니다",
    "요청이 취소되었습니다",
    "GraphQL 요청 구성이 올바르지 않습니다",
    "GraphQL endpoint URL이 올바르지 않습니다",
    "GraphQL endpoint query에 credential을 넣을 수 없습니다",
    "GraphQL query가 허용된 크기를 초과했습니다",
    "GraphQL variables가 허용된 크기를 초과했습니다",
    "GraphQL 문서 형식이 올바르지 않습니다",
    "GraphQL operation 선택이 올바르지 않습니다",
    "GraphQL variables는 유효한 JSON object여야 합니다",
    "GraphQL variables 구조가 허용된 한계를 초과했습니다",
    "GraphQL 요청 본문이 허용된 크기를 초과했습니다",
    "GraphQL introspection 요청은 지원하지 않습니다",
    "GraphQL subscription은 지원하지 않습니다",
    "GraphQL timeout이 허용된 범위를 벗어났습니다",
    "GraphQL header 행 수가 허용된 한계를 초과했습니다",
    "GraphQL header 크기가 허용된 한계를 초과했습니다",
    GRAPHQL_HEADER_ROWS_ERROR,
    GRAPHQL_HEADER_BYTES_ERROR,
    GRAPHQL_URL_TOO_LARGE,
    "GraphQL 리다이렉트를 브라우저 미리보기에서 처리할 수 없습니다",
    "응답 본문이 허용된 크기를 초과했습니다",
  ];
  if (safeMessages.includes(message) || /^'.+' 파일을 다시 선택하세요\.$/.test(message)) {
    return message;
  }
  return "요청에 실패했습니다. URL, 연결 상태와 secret 설정을 확인하세요.";
}

export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** curl -F의 쉼표/세미콜론/@ 및 quote parsing과 shell parsing을 분리한다. */
export function curlFormQuote(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
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
