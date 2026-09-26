import { useEnvironmentPersistence } from "./hooks/useEnvironmentPersistence";
import { storageFailureMessage } from "../storage/documentStorage";
import { RequestParameters } from "./components/RequestParameters";
import { SseControls } from "./components/SseControls";
export { tryPretty, buildCurl, shellQuote, curlFormQuote } from "./lib/requestPresentation";
import { RequestSidebar } from "./components/RequestSidebar";
import { RequestHandoffDialog } from "./components/RequestHandoffDialog";
import {
  sseStateLabel,
  downloadJson,
  defaultSseOptions,
  emptyReq,
  graphqlConfigError,
  tryPretty,
  buildCurl,
  safeRequestError,
  safeWebSocketUiError,
  safeHandoffError,
  isTerminalHandoffError,
  copyRevealedCurl,
} from "./lib/requestPresentation";
import { usePolling, useOperation, useReviewFlow } from "@devbox/hooks";
import { OpenApiDefinitions, type DefinitionSummary } from "./OpenApiDefinitions";
import { ApiWorkspacePanel, type ApiWorkspace } from "./ApiWorkspace";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { isImeComposing } from "@devbox/a11y";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import * as api from "./api";
import {
  ackApiRequest,
  claimApiRequest,
  copyRawResponseCookies,
  copyRawResponseHeaders,
  discardCurrentResponse,
  readJsonFile,
  onOpenRequest,
  renewApiRequest,
  restoreApiRequest,
  saveJsonFile,
  saveResponseBinary,
  sanitizePersistedJson,
  sendRequest,
  startSseStream,
  type SseStreamHandle,
  startWebSocket,
  type WebSocketHandle,
  takePendingOpen,
} from "./api";
const HistoryConsole = lazy(() => import("./HistoryConsole").then((module) => ({ default: module.HistoryConsole })));
import { SavedRequestPreview } from "./SavedRequestPreview";
const OpenApiImport = lazy(() => import("./OpenApiImport").then((module) => ({ default: module.OpenApiImport })));
const ProtocolLab = lazy(() => import("./ProtocolLab").then((module) => ({ default: module.ProtocolLab })));
import { ResponseViewer, type RawResponseCopyKind } from "./ResponseViewer";
import { SseEventViewer } from "./SseEventViewer";
import { WebSocketPanel } from "./WebSocketPanel";
import {
  addEntry,
  duplicateEntry,
  emptyStore as emptyCollectionStore,
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
import { emptyStore as emptyEnvStore, type EnvironmentStore, loadStore as loadEnvStore } from "./lib/environments";
import {
  emptyHistoryStore,
  migrateHistoryStorage,
  sanitizeRequestForPersistence,
  sanitizeHistoryStore,
  saveHistoryStore,
  toRequestTemplate,
  type HistoryStore,
} from "./lib/persistence";
import {
  filterHistory,
  historyDisplayLabel,
  historyMethod as historyMethodOf,
  type HistoryStatusFilter,
} from "./lib/history";
import {
  mergeImportedCollections,
  mergeImportedEnvironments,
  MAX_TRANSFER_BYTES,
  parseCollectionExport,
  parseEnvironmentExport,
  readTransferFile,
  serializeCollectionExport,
  serializeEnvironmentExport,
} from "./lib/transfer";
import { hasCookieSourceConflict, validateCookies } from "./lib/cookies";
import { validateMultipartParts } from "./lib/multipart";
import { OPENAPI_LIMITS } from "./lib/openapiLimits";
import type { OpenApiOperationPreview } from "./lib/openapi";
import { eventSize, MAX_DECODED_BYTES, MAX_RETAINED_EVENTS, type SseEvent } from "./lib/sse";
import { isTauri } from "./lib/isTauri";
import { WebSocketMessageBuffer } from "./lib/websocket";
import type {
  ApiRequestHandoffPreview,
  ApiResponse,
  HistoryItem,
  OpenRequest,
  RequestTemplate,
  SseOptions,
  SseUpdate,
  WebSocketConnectionState,
  WebSocketMessage,
  WebSocketUpdate,
} from "./types";
import "./App.css";

export { statusClass } from "./ResponseViewer";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const BODY_KINDS = ["none", "json", "form", "multipart", "raw", "graphql"];
const AUTH_KINDS = ["none", "basic", "bearer", "apikey"];
const MAX_SSE_UI_ROWS = 1_000;
const API_REQUEST_HANDOFF_KIND = "api-request/v1";

export default function App({
  section,
  onNavigate,
}: {
  section?: "requests" | "protocols" | "history";
  onNavigate?: (route: "requests") => void;
} = {}) {
  const [req, setReq] = useState<RequestTemplate>(emptyReq);
  const [resp, setResp] = useState<ApiResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sseOptions, setSseOptions] = useState<SseOptions>(defaultSseOptions);
  const [sseState, setSseState] = useState<"idle" | "connecting" | "connected" | "stopped" | "closed" | "error">(
    "idle",
  );
  const [sseEvents, setSseEvents] = useState<SseEvent[]>([]);
  const [sseDropped, setSseDropped] = useState(0);
  const [ssePaused, setSsePausedState] = useState(false);
  const sseHandleRef = useRef<SseStreamHandle | null>(null);
  const sseGenerationRef = useRef(0);
  const sseStopRequestedRef = useRef(false);
  const sseTerminalGenerationRef = useRef<number | null>(null);
  const ssePausedRef = useRef(false);
  const sseHistoryRef = useRef<SseEvent[]>([]);
  const sseHistoryBytesRef = useRef(0);
  const [showCurl, setShowCurl] = useState(false);
  const [showOpenApiImport, setShowOpenApiImport] = useState(false);
  const [localWorkspace, setWorkspace] = useState<"http" | "protocol">("http");
  const workspace = section ? (section === "protocols" ? "protocol" : "http") : localWorkspace;
  const [protocolVisited, setProtocolVisited] = useState(section === "protocols");
  useEffect(() => {
    if (workspace === "protocol") setProtocolVisited(true);
  }, [workspace]);
  const [tab, setTab] = useState<"params" | "headers" | "cookies" | "body" | "auth">("params");
  const [pretty, setPretty] = useState(true);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [historyQuery, setHistoryQuery] = useState("");
  const [historyMethod, setHistoryMethod] = useState("");
  const [historyStatus, setHistoryStatus] = useState<HistoryStatusFilter>("all");
  const [collections, setCollections] = useState(emptyCollectionStore);
  const [apiWorkspace, setApiWorkspace] = useState<ApiWorkspace | null>(null);
  const [openApiDefinitionRevision, setOpenApiDefinitionRevision] = useState(0);
  const [openApiDefinitions, setOpenApiDefinitions] = useState<DefinitionSummary[]>([]);
  const [collName, setCollName] = useState("");
  const [collFolder, setCollFolder] = useState("");
  const [collFilter, setCollFilter] = useState("");
  const [collSaving, setCollSaving] = useState(false);
  const [envStore, setEnvStore] = useState(emptyEnvStore);
  const [currentEnvId, setCurrentEnvId] = useState("");
  const [envName, setEnvName] = useState("");
  const [migrationNotice, setMigrationNotice] = useState<string | null>(null);
  const [persistenceWarning, setPersistenceWarning] = useState<string | null>(null);
  const [persistenceReady, setPersistenceReady] = useState(false);
  const [transferBusy, setTransferBusy] = useState(false);
  const [browserImportKind, setBrowserImportKind] = useState<"collection" | "environment" | null>(null);
  const [environmentBusy, setEnvironmentBusy] = useState(false);
  const [contextActionBusy, setContextActionBusy] = useState(false);
  const [savedPreview, setSavedPreview] = useState<{ kind: "history" | "collection"; id: string } | null>(null);
  const [selectedHistoryId, setSelectedHistoryId] = useState<string | null>(null);
  const [selectedCollectionId, setSelectedCollectionId] = useState<string | null>(null);
  const [requestEditorRevision, setRequestEditorRevision] = useState(0);
  const openApiCollectionSavingRef = useRef(false);
  const [contextHistory, setContextHistory] = useState<HistoryItem | null>(null);
  const [contextCollection, setContextCollection] = useState<CollectionEntry | null>(null);
  const [webSocketState, setWebSocketState] = useState<WebSocketConnectionState>("idle");
  const [webSocketMessages, setWebSocketMessages] = useState<WebSocketMessage[]>([]);
  const [webSocketDropped, setWebSocketDropped] = useState(0);
  const { busy: webSocketBusy, run: runWebSocketOperation } = useOperation();
  const webSocketActionPending = useRef(false);
  const runWebSocketAction = useCallback(
    async (task: () => Promise<void>) => {
      if (webSocketActionPending.current) return;
      webSocketActionPending.current = true;
      try {
        await runWebSocketOperation(task);
      } finally {
        webSocketActionPending.current = false;
      }
    },
    [runWebSocketOperation],
  );
  const mountedRef = useRef(true);
  const requestSequenceRef = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);
  const webSocketHandleRef = useRef<WebSocketHandle | null>(null);
  const webSocketGenerationRef = useRef(0);
  const webSocketTerminalGenerationRef = useRef<number | null>(null);
  const webSocketSequenceRef = useRef(0);
  const webSocketBufferRef = useRef(new WebSocketMessageBuffer());
  const handoffPreviewRef = useRef<ApiRequestHandoffPreview | null>(null);
  const handoffBusyRef = useRef(false);
  const requestedHandoffId = useRef("");
  const handoffFlow = useReviewFlow<ApiRequestHandoffPreview, RequestTemplate>({
    async preview(signal) {
      try {
        const preview = await claimApiRequest(requestedHandoffId.current);
        if (!signal.aborted && mountedRef.current) handoffPreviewRef.current = preview;
        return preview;
      } catch (cause) {
        throw new Error(safeHandoffError(cause));
      }
    },
    async apply(preview) {
      try {
        const request = await ackApiRequest(preview.handoffId);
        handoffPreviewRef.current = null;
        if (mountedRef.current) {
          setReq(request);
          setRequestEditorRevision((revision) => revision + 1);
          setResp(null);
          setPersistenceWarning(null);
        }
        return request;
      } catch (cause) {
        throw new Error(safeHandoffError(cause));
      }
    },
    async discard(preview) {
      try {
        await restoreApiRequest(preview.handoffId);
      } catch (cause) {
        const message = safeHandoffError(cause);
        if (!isTerminalHandoffError(message)) throw new Error(message);
      }
      if (handoffPreviewRef.current?.handoffId === preview.handoffId) handoffPreviewRef.current = null;
    },
  });
  const handoffPreview = handoffFlow.state === "idle" || handoffFlow.state === "done" ? null : handoffFlow.preview;
  const handoffBusy = handoffFlow.state === "previewing" || handoffFlow.state === "applying";
  const { issue: handoffIssue, reset: resetHandoff } = handoffFlow;
  useEffect(() => {
    if (!handoffIssue) return;
    setError(handoffIssue);
    if (isTerminalHandoffError(handoffIssue)) void resetHandoff();
  }, [handoffIssue, resetHandoff]);

  const handoffDialogRef = useRef<HTMLElement | null>(null);
  const handoffCancelButtonRef = useRef<HTMLButtonElement | null>(null);
  const browserImportInputRef = useRef<HTMLInputElement | null>(null);
  const browserImportKindRef = useRef<"collection" | "environment" | null>(null);
  const handoffPreviousFocusRef = useRef<HTMLElement | null>(null);
  const cancelHandoffRef = useRef<() => void>(() => undefined);
  const collectionStoreRef = useRef(collections);
  const collectionRevisionRef = useRef(0);
  const collectionMutationBusyRef = useRef(false);
  const envStoreRef = useRef(envStore);
  const environmentRevisionRef = useRef(0);
  const environmentMutationBusyRef = useRef(false);
  const environmentBusyRef = useRef(false);
  const transferBusyRef = useRef(false);

  // Keep imperative async continuations on the latest stores instead of on a
  // render-time closure. This is especially important for native secret
  // sealing and file transfer, both of which can finish after another edit.
  collectionStoreRef.current = collections;
  envStoreRef.current = envStore;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      requestSequenceRef.current += 1;
      abortControllerRef.current?.abort();
      browserImportKindRef.current = null;
      // Abort only stops the renderer-side wait. Explicitly clear the native
      // one-shot response vault as well so a late result cannot survive this
      // renderer instance.
      void discardCurrentResponse().catch(() => undefined);
      webSocketGenerationRef.current += 1;
      const handle = webSocketHandleRef.current;
      webSocketHandleRef.current = null;
      if (handle) void handle.stop().catch(() => undefined);
    };
  }, []);

  // Browsers do not dispatch `change` when the native file picker is
  // cancelled. Listen to the input's cancel event and use a focus fallback so
  // a cancelled picker never leaves the import flow permanently busy.
  useEffect(() => {
    const input = browserImportInputRef.current;
    if (!input) return undefined;
    const clearPicker = () => {
      browserImportKindRef.current = null;
      if (mountedRef.current) setBrowserImportKind(null);
    };
    const onCancel = () => clearPicker();
    const onWindowFocus = () => {
      window.setTimeout(() => {
        if (browserImportKindRef.current && !input.files?.length) clearPicker();
      }, 0);
    };
    input.addEventListener("cancel", onCancel);
    window.addEventListener("focus", onWindowFocus);
    return () => {
      input.removeEventListener("cancel", onCancel);
      window.removeEventListener("focus", onWindowFocus);
    };
  }, []);

  // AppLink delivery is intentionally independent of History migration. The
  // native shell stores the request before emitting, so the listener is
  // registered first and both cold/hot paths pull the same pending slot.
  const handleOpenRequest = (request: OpenRequest) => {
    if (request.target.kind !== "handoff") return;
    if (request.target.handoffKind !== API_REQUEST_HANDOFF_KIND) {
      setError("지원하지 않는 handoff 요청입니다");
      return;
    }
    if (handoffBusyRef.current || handoffPreviewRef.current) {
      setError("기존 handoff 미리보기를 먼저 적용하거나 취소하세요");
      return;
    }
    handoffBusyRef.current = true;
    requestedHandoffId.current = request.target.id;
    setError(null);
    void handoffFlow.start().finally(() => {
      handoffBusyRef.current = false;
    });
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let coldStartConsumed = false;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequestRef.current(request);
        })
        .catch((cause) => {
          if (!disposed) setError(safeHandoffError(cause));
        });
    };
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };

    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => {
        consumeColdStart();
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  usePolling(
    async () => {
      const preview = handoffPreviewRef.current;
      if (!preview || handoffBusyRef.current) return;
      const id = preview.handoffId;
      try {
        await renewApiRequest(id);
      } catch (cause) {
        if (!mountedRef.current || handoffPreviewRef.current?.handoffId !== id) return;
        const message = safeHandoffError(cause);
        if (isTerminalHandoffError(message)) {
          void resetHandoff();
        }
        setError(message);
      }
    },
    { intervalMs: 30_000, active: Boolean(handoffPreview), immediate: false },
  );

  const prepareHistoryContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.historyId;
      const item = history.find((candidate) => candidate.id === id);
      if (!item) return;
      setSelectedHistoryId(item.id);
      setContextHistory(item);
    },
    [history],
  );
  const historyContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareHistoryContext(target),
  });

  const prepareCollectionContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.collectionId;
      const item = collections.collections.find((candidate) => candidate.id === id);
      if (!item) return;
      setSelectedCollectionId(item.id);
      setContextCollection(item);
    },
    [collections.collections],
  );
  const collectionContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareCollectionContext(target),
  });

  const currentEnv = envStore.environments.find((e) => e.id === currentEnvId) ?? null;
  const historyMethods = useMemo(() => [...new Set(history.map(historyMethodOf))].sort(), [history]);
  const visibleHistory = useMemo(
    () => filterHistory(history, { query: historyQuery, method: historyMethod, status: historyStatus }),
    [history, historyMethod, historyQuery, historyStatus],
  );
  const cookieIssues = validateCookies(req.cookies);
  const cookieConflict = hasCookieSourceConflict(req.cookies, req.headers);
  const cookieConfigurationError = cookieConflict
    ? "활성 Cookie header와 구조화 Cookie 중 하나만 사용하세요."
    : cookieIssues[0]
      ? `${cookieIssues[0].index + 1}번 Cookie: ${cookieIssues[0].message}`
      : null;
  const multipartIssue = req.body_kind === "multipart" ? (validateMultipartParts(req.multipart)[0] ?? null) : null;
  const graphqlIssue = graphqlConfigError(req);
  const requestConfigurationError =
    graphqlIssue ??
    cookieConfigurationError ??
    (multipartIssue ? `${multipartIssue.index + 1}번 multipart part: ${multipartIssue.message}` : null);
  const { persistEnvs, onCreateEnv, tryPersistEnvs, startSecretSeal } = useEnvironmentPersistence({
    environmentRevisionRef,
    environmentBusyRef,
    envStoreRef,
    setEnvStore,
    environmentMutationBusyRef,
    mountedRef,
    setPersistenceReady,
    setPersistenceWarning,
    transferBusyRef,
    setEnvironmentBusy,
    setError,
    persistenceReady,
    envName,
    setCurrentEnvId,
    setEnvName,
  });

  const sanitizeForPersistence = useCallback(
    (serialized: string) =>
      sanitizePersistedJson(
        serialized,
        envStore.environments.flatMap((environment) => environment.variables),
      ),
    [envStore.environments],
  );

  const persistCollections = async (
    store: ReturnType<typeof emptyCollectionStore>,
    expectedRevision = collectionRevisionRef.current,
  ) => {
    if (expectedRevision !== collectionRevisionRef.current || collectionMutationBusyRef.current) {
      throw new Error("collection mutation is stale or busy");
    }
    collectionMutationBusyRef.current = true;
    try {
      const safe = await saveStore(
        store,
        sanitizeForPersistence,
        undefined,
        () => expectedRevision === collectionRevisionRef.current,
      );
      if (expectedRevision !== collectionRevisionRef.current) throw new Error("collection mutation is stale");
      collectionStoreRef.current = safe;
      collectionRevisionRef.current += 1;
      if (mountedRef.current) setCollections(safe);
      return safe;
    } catch (cause) {
      if (cause instanceof Error && cause.name === "store_revision_conflict") {
        const reloaded = await migrateCollections(sanitizeForPersistence);
        if (!reloaded.failed) {
          collectionStoreRef.current = reloaded.store;
          collectionRevisionRef.current += 1;
          if (mountedRef.current) setCollections(reloaded.store);
        }
      }
      throw cause;
    } finally {
      collectionMutationBusyRef.current = false;
    }
  };

  const persistHistory = async (store: HistoryStore) => {
    const safe = await saveHistoryStore(store, sanitizeForPersistence);
    setHistory(safe.history);
    return safe;
  };

  const applyImportedTransfer = async (
    kind: "collection" | "environment",
    raw: string,
    expectedCollectionRevision: number,
    expectedEnvironmentRevision: number,
  ) => {
    if (kind === "collection") {
      const imported = parseCollectionExport(raw);
      if (!imported) throw new Error("컬렉션 JSON 형식이 올바르지 않습니다");
      if (expectedCollectionRevision !== collectionRevisionRef.current) {
        throw new Error("오래된 컬렉션 가져오기로 현재 상태를 덮어쓰지 않았습니다");
      }
      let sequence = 0;
      const merged = mergeImportedCollections(
        collectionStoreRef.current,
        imported,
        () => `c-import-${Date.now()}-${sequence++}`,
      );
      if (!merged) throw new Error("컬렉션 가져오기를 한 번에 적용할 수 없습니다");
      const previousCount = collectionStoreRef.current.collections.length;
      const safe = await persistCollections(merged, expectedCollectionRevision);
      if (!mountedRef.current) return;
      const added = Math.max(0, safe.collections.length - previousCount);
      setSelectedCollectionId(safe.collections[0]?.id ?? null);
      setMigrationNotice(`컬렉션 ${added}건을 추가했습니다. 기존 항목은 덮어쓰지 않았습니다.`);
      return;
    }

    const imported = parseEnvironmentExport(raw);
    if (!imported) throw new Error("환경 JSON 형식이 올바르지 않습니다");
    if (expectedEnvironmentRevision !== environmentRevisionRef.current) {
      throw new Error("오래된 환경 가져오기로 현재 상태를 덮어쓰지 않았습니다");
    }
    let sequence = 0;
    const next = mergeImportedEnvironments(envStoreRef.current, imported, () => `e-import-${Date.now()}-${sequence++}`);
    if (!next) throw new Error("환경 가져오기를 한 번에 적용할 수 없습니다");
    const previousCount = envStoreRef.current.environments.length;
    const saved = await persistEnvs(next, expectedEnvironmentRevision);
    if (!mountedRef.current) return;
    const added = Math.max(0, saved.environments.length - previousCount);
    if (!currentEnvId) setCurrentEnvId(saved.environments[0]?.id ?? "");
    setMigrationNotice(`환경 ${added}건을 추가했습니다. secret 값은 보안상 다시 입력해야 합니다.`);
  };

  const onImportTransfer = (kind: "collection" | "environment") => {
    if (
      !persistenceReady ||
      transferBusyRef.current ||
      browserImportKind ||
      environmentBusyRef.current ||
      sending ||
      collSaving ||
      contextActionBusy
    )
      return;
    const expectedCollectionRevision = collectionRevisionRef.current;
    const expectedEnvironmentRevision = environmentRevisionRef.current;
    if (isTauri()) {
      transferBusyRef.current = true;
      setTransferBusy(true);
      setPersistenceWarning(null);
      void (async () => {
        try {
          const raw = await readJsonFile();
          if (raw !== null) {
            await applyImportedTransfer(kind, raw, expectedCollectionRevision, expectedEnvironmentRevision);
          }
        } catch (storageCause) {
          if (mountedRef.current)
            setPersistenceWarning(
              storageFailureMessage(storageCause, "JSON 파일을 가져오지 않았습니다. 파일 선택과 schema를 확인하세요."),
            );
        } finally {
          transferBusyRef.current = false;
          if (mountedRef.current) setTransferBusy(false);
        }
      })();
      return;
    }
    const input = browserImportInputRef.current;
    if (!input) return;
    browserImportKindRef.current = kind;
    input.value = "";
    setBrowserImportKind(kind);
    input.click();
  };

  const onBrowserImportFile = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0];
    const kind = browserImportKindRef.current;
    event.currentTarget.value = "";
    browserImportKindRef.current = null;
    setBrowserImportKind(null);
    if (!file || !kind) return;
    if (file.size > MAX_TRANSFER_BYTES) {
      setPersistenceWarning("JSON 파일이 허용된 크기(1 MiB)를 초과해 가져오지 않았습니다.");
      return;
    }
    transferBusyRef.current = true;
    setTransferBusy(true);
    setPersistenceWarning(null);
    const expectedCollectionRevision = collectionRevisionRef.current;
    const expectedEnvironmentRevision = environmentRevisionRef.current;
    void readTransferFile(file)
      .then(async (raw) => {
        try {
          await applyImportedTransfer(kind, raw, expectedCollectionRevision, expectedEnvironmentRevision);
        } catch {
          if (mountedRef.current) {
            setPersistenceWarning(
              `${kind === "collection" ? "컬렉션" : "환경"} JSON을 가져오지 않았습니다. schema, 크기와 secret 정책을 확인하세요.`,
            );
          }
        }
      })
      .catch((storageCause) => {
        if (mountedRef.current)
          setPersistenceWarning(storageFailureMessage(storageCause, "JSON 파일을 읽지 못해 가져오지 않았습니다."));
      })
      .finally(() => {
        transferBusyRef.current = false;
        if (mountedRef.current) setTransferBusy(false);
      });
  };

  const onExportTransfer = (kind: "collection" | "environment") => {
    if (
      !persistenceReady ||
      transferBusyRef.current ||
      environmentBusyRef.current ||
      sending ||
      collSaving ||
      contextActionBusy
    )
      return;
    transferBusyRef.current = true;
    setTransferBusy(true);
    setPersistenceWarning(null);
    try {
      const content =
        kind === "collection"
          ? serializeCollectionExport(collectionStoreRef.current)
          : serializeEnvironmentExport(envStoreRef.current);
      if (new TextEncoder().encode(content).byteLength > MAX_TRANSFER_BYTES) {
        throw new Error("transfer too large");
      }
      const fileName = kind === "collection" ? "api-playground-collections.json" : "api-playground-environments.json";
      if (isTauri()) {
        void saveJsonFile(content, fileName)
          .then((saved) => {
            if (saved && mountedRef.current) {
              setMigrationNotice(`${kind === "collection" ? "컬렉션" : "환경"} JSON 내보내기를 완료했습니다.`);
            }
          })
          .catch((storageCause) => {
            if (mountedRef.current)
              setPersistenceWarning(
                storageFailureMessage(storageCause, "JSON 파일을 저장하지 않았습니다. native 저장 위치를 확인하세요."),
              );
          })
          .finally(() => {
            transferBusyRef.current = false;
            if (mountedRef.current) setTransferBusy(false);
          });
      } else {
        downloadJson(content, fileName);
        setMigrationNotice(`${kind === "collection" ? "컬렉션" : "환경"} JSON 다운로드를 시작했습니다.`);
        transferBusyRef.current = false;
        setTransferBusy(false);
      }
    } catch (storageCause) {
      setPersistenceWarning(
        storageFailureMessage(storageCause, "JSON 내보내기를 생성하지 못했습니다. 항목 수와 크기를 확인하세요."),
      );
      transferBusyRef.current = false;
      setTransferBusy(false);
    }
  };

  const onSaveCollection = async () => {
    if (collectionMutationBusyRef.current || environmentBusyRef.current || transferBusyRef.current) return;
    setCollSaving(true);
    setPersistenceWarning(null);
    try {
      const next = addEntry(
        collectionStoreRef.current,
        { name: collName, folder: collFolder, request: req },
        Date.now(),
        () => `c-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      );
      await persistCollections(next);
      setCollName("");
      setCollFolder("");
    } catch (storageCause) {
      setPersistenceWarning(
        storageFailureMessage(storageCause, "민감정보 안전 검증에 실패해 컬렉션을 저장하지 않았습니다."),
      );
    } finally {
      setCollSaving(false);
    }
  };

  const applyOpenApiRequest = (request: RequestTemplate) => {
    setReq(request);
    setRequestEditorRevision((revision) => revision + 1);
    setTab(request.body_kind !== "none" ? "body" : "params");
    setResp(null);
    setError(null);
    setPersistenceWarning(null);
  };

  const onOpenApiApply = (operation: OpenApiOperationPreview) => applyOpenApiRequest(operation.request);

  const onOpenApiAddToCollection = async (operations: OpenApiOperationPreview[]) => {
    if (openApiCollectionSavingRef.current) throw new Error("OpenAPI 컬렉션 저장이 이미 진행 중입니다");
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
            randomId =
              typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
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
          {
            name: operation.label.slice(0, OPENAPI_LIMITS.maxCollectionNameLength),
            folder: "OpenAPI",
            request: operation.request,
          },
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

  const startup = useRef<Promise<{
    environments: EnvironmentStore;
    collections: ReturnType<typeof emptyCollectionStore>;
    history: HistoryItem[];
  }> | null>(null);
  useEffect(() => {
    let active = true;
    startup.current ??= (async () => {
      const environments = await loadEnvStore();
      const variables = environments.environments.flatMap((environment) => environment.variables);
      const sanitize = (serialized: string) => sanitizePersistedJson(serialized, variables);
      const [history, collections] = await Promise.all([migrateHistoryStorage(), migrateCollections(sanitize)]);
      if (history.failed || collections.failed)
        throw new Error("저장된 요청을 안전하게 불러오지 못했습니다. 원본은 유지됩니다.");
      const safe = await saveHistoryStore(history.store, sanitize);
      return { environments, collections: collections.store, history: safe.history };
    })();
    void startup.current
      .then((loaded) => {
        if (!active) return;
        envStoreRef.current = loaded.environments;
        collectionStoreRef.current = loaded.collections;
        environmentRevisionRef.current += 1;
        collectionRevisionRef.current += 1;
        setEnvStore(loaded.environments);
        setCollections(loaded.collections);
        setHistory(loaded.history);
        setPersistenceReady(true);
      })
      .catch((storageCause) => {
        if (active)
          setPersistenceWarning(
            storageFailureMessage(
              storageCause,
              "저장된 요청을 안전하게 불러오지 못했습니다. 원본은 유지되며 다음 실행에서 다시 시도합니다.",
            ),
          );
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const id = contextHistory?.id;
    if (!id) return;
    const current = history.find((item) => item.id === id) ?? null;
    if (current) setContextHistory(current);
    else {
      historyContextMenu.close();
      setContextHistory(null);
      setSelectedHistoryId((selected) => (selected === id ? null : selected));
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
      setSelectedCollectionId((selected) => (selected === id ? null : selected));
    }
  }, [collectionContextMenu.close, collections.collections, contextCollection?.id]);

  const persistHistoryRequest = useCallback(
    async (request: RequestTemplate, status: number | undefined, isCurrent: () => boolean) => {
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
      try {
        const safe = await saveHistoryStore(candidate, sanitizeForPersistence);
        if (isCurrent()) setHistory(safe.history);
        return safe;
      } catch (cause) {
        if (cause instanceof Error && cause.name === "store_revision_conflict") {
          const reloaded = await migrateHistoryStorage();
          if (!reloaded.failed && isCurrent()) {
            const safe = await sanitizeHistoryStore(reloaded.store, sanitizeForPersistence);
            setHistory(safe.history);
          }
        }
        throw cause;
      }
    },
    [history, sanitizeForPersistence],
  );

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
      } catch (storageCause) {
        if (mountedRef.current && requestSequenceRef.current === sequence) {
          setPersistenceWarning(
            storageFailureMessage(
              storageCause,
              "요청은 완료됐지만 민감정보 안전 검증에 실패해 기록을 저장하지 않았습니다.",
            ),
          );
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
      } catch (storageCause) {
        if (mountedRef.current && requestSequenceRef.current === sequence) {
          setPersistenceWarning(
            storageFailureMessage(
              storageCause,
              "실패한 요청은 민감정보 안전 검증을 통과하지 못해 기록에 저장하지 않았습니다.",
            ),
          );
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

  const sseActive = sseState === "connecting" || sseState === "connected";

  const updateSseHistory = useCallback((event: SseEvent) => {
    const history = sseHistoryRef.current;
    history.push(event);
    sseHistoryBytesRef.current += eventSize(event);
    while (history.length > MAX_RETAINED_EVENTS || sseHistoryBytesRef.current > MAX_DECODED_BYTES) {
      const oldest = history.shift();
      if (!oldest) {
        sseHistoryBytesRef.current = 0;
        break;
      }
      sseHistoryBytesRef.current = Math.max(0, sseHistoryBytesRef.current - eventSize(oldest));
      setSseDropped((count) => count + 1);
    }
    if (!ssePausedRef.current) {
      setSseEvents(history.slice(-MAX_SSE_UI_ROWS));
    }
  }, []);

  const handleSseUpdate = useCallback(
    (generation: number, update: SseUpdate) => {
      if (generation !== sseGenerationRef.current) return;
      if (update.kind === "event" && typeof update.event === "string" && typeof update.data === "string") {
        updateSseHistory({
          event: update.event,
          data: update.data,
          ...(update.id ? { id: update.id } : {}),
          ...(update.retryMs === undefined ? {} : { retryMs: update.retryMs }),
        });
        return;
      }
      if (update.kind === "connected") {
        setSseState("connected");
      } else if (update.kind === "closed") {
        setSseState("closed");
        sseTerminalGenerationRef.current = generation;
        const handle = sseHandleRef.current;
        sseHandleRef.current = null;
        if (handle) void handle.stop().catch(() => undefined);
      } else if (update.kind === "error") {
        setSseState("error");
        setError(update.message ?? "SSE 스트림에 실패했습니다.");
        sseTerminalGenerationRef.current = generation;
        const handle = sseHandleRef.current;
        sseHandleRef.current = null;
        if (handle) void handle.stop().catch(() => undefined);
      }
    },
    [updateSseHistory],
  );

  const clearSseHistory = () => {
    sseHistoryRef.current = [];
    sseHistoryBytesRef.current = 0;
    setSseEvents([]);
    setSseDropped(0);
  };

  const setSsePaused = (paused: boolean) => {
    ssePausedRef.current = paused;
    setSsePausedState(paused);
    if (!paused) setSseEvents(sseHistoryRef.current.slice(-MAX_SSE_UI_ROWS));
  };

  const onStartSse = async () => {
    if (sseActive || sending || contextActionBusy || requestConfigurationError || !req.url) return;
    if (req.method !== "GET" && req.method !== "POST") {
      setError("SSE 스트림은 GET 또는 POST만 지원합니다.");
      return;
    }
    const generation = sseGenerationRef.current + 1;
    sseGenerationRef.current = generation;
    sseStopRequestedRef.current = false;
    sseTerminalGenerationRef.current = null;
    clearSseHistory();
    setSseState("connecting");
    setError(null);
    try {
      const handle = await startSseStream(req, currentEnv?.variables ?? [], sseOptions, (update) =>
        handleSseUpdate(generation, update),
      );
      if (
        generation !== sseGenerationRef.current ||
        sseStopRequestedRef.current ||
        sseTerminalGenerationRef.current === generation
      ) {
        await handle.stop().catch(() => undefined);
        return;
      }
      sseHandleRef.current = handle;
    } catch (cause) {
      if (generation !== sseGenerationRef.current || sseStopRequestedRef.current) return;
      setSseState("error");
      setError(safeRequestError(cause));
    }
  };

  const onStopSse = async () => {
    if (!sseActive && !sseHandleRef.current) return;
    sseStopRequestedRef.current = true;
    sseGenerationRef.current += 1;
    const handle = sseHandleRef.current;
    sseHandleRef.current = null;
    setSseState("stopped");
    if (handle) {
      try {
        await handle.stop();
      } catch {
        setError("SSE 스트림을 중지하지 못했습니다.");
      }
    }
  };

  useEffect(
    () => () => {
      sseStopRequestedRef.current = true;
      sseGenerationRef.current += 1;
      const handle = sseHandleRef.current;
      sseHandleRef.current = null;
      if (handle) void handle.stop().catch(() => undefined);
    },
    [],
  );

  const applyWebSocketUpdate = (generation: number, update: WebSocketUpdate) => {
    if (!mountedRef.current || webSocketGenerationRef.current !== generation) return;
    if (update.kind === "state") {
      webSocketSequenceRef.current = Math.max(webSocketSequenceRef.current, update.sequence);
      if (update.state) setWebSocketState(update.state);
      setWebSocketDropped((current) => Math.max(current, update.dropped, webSocketBufferRef.current.evicted));
      if (update.message) setError(update.message);
      if (update.state === "closed" || update.state === "error") {
        webSocketTerminalGenerationRef.current = generation;
        const handle = webSocketHandleRef.current;
        if (handle) void handle.stop().catch(() => undefined);
      }
      return;
    }
    if (update.sequence <= webSocketSequenceRef.current) return;
    if (!update.messageId || !update.messageType || !update.direction) return;
    webSocketSequenceRef.current = update.sequence;
    const message: WebSocketMessage = {
      id: update.messageId,
      direction: update.direction,
      kind: update.messageType,
      text: update.text,
      textTruncated: update.textTruncated,
      binaryHex: update.binaryHex,
      binaryText: update.binaryText,
      binarySize: update.binarySize,
      binaryTruncated: update.binaryTruncated,
      closeCode: update.closeCode,
      closeReason: update.closeReason,
    };
    const retained = webSocketBufferRef.current;
    retained.push(message);
    setWebSocketMessages([...retained.messages]);
    setWebSocketDropped(Math.max(update.dropped, retained.evicted));
  };

  const onWebSocketConnect = () =>
    runWebSocketAction(async () => {
      if (webSocketBusy || webSocketState === "open" || webSocketState === "connecting" || webSocketState === "closing")
        return;
      const generation = webSocketGenerationRef.current + 1;
      webSocketGenerationRef.current = generation;
      webSocketTerminalGenerationRef.current = null;
      webSocketSequenceRef.current = 0;
      webSocketBufferRef.current = new WebSocketMessageBuffer();
      const previous = webSocketHandleRef.current;
      webSocketHandleRef.current = null;
      setWebSocketMessages([]);
      setWebSocketDropped(0);
      setWebSocketState("connecting");
      setError(null);
      if (previous) await previous.stop().catch(() => undefined);
      try {
        const handle = await startWebSocket(req, currentEnv?.variables ?? [], (update) =>
          applyWebSocketUpdate(generation, update),
        );
        if (!mountedRef.current || webSocketGenerationRef.current !== generation) {
          await handle.stop().catch(() => undefined);
          return;
        }
        if (webSocketTerminalGenerationRef.current === generation) {
          await handle.stop().catch(() => undefined);
          if (mountedRef.current && webSocketGenerationRef.current === generation) {
            webSocketHandleRef.current = handle;
          }
          return;
        }
        webSocketHandleRef.current = handle;
      } catch (cause) {
        if (mountedRef.current && webSocketGenerationRef.current === generation) {
          setWebSocketState("error");
          setError(safeWebSocketUiError(cause));
        }
      }
    });

  const onWebSocketDisconnect = () =>
    runWebSocketAction(async () => {
      if (webSocketBusy) return;
      const handle = webSocketHandleRef.current;
      if (!handle) {
        setWebSocketState("idle");
        return;
      }
      try {
        await handle.close(1000, "client disconnect");
      } catch (cause) {
        setWebSocketState("error");
        setError(safeWebSocketUiError(cause));
      }
    });

  const onWebSocketSend = (kind: "text" | "binary", value: string, encoding: "text" | "hex") =>
    runWebSocketAction(async () => {
      const handle = webSocketHandleRef.current;
      if (!handle || webSocketBusy) return;
      setError(null);
      try {
        await handle.send(kind, value, encoding);
      } catch (cause) {
        setError(safeWebSocketUiError(cause));
      }
    });

  const onWebSocketPing = (value: string, encoding: "text" | "hex") =>
    runWebSocketAction(async () => {
      const handle = webSocketHandleRef.current;
      if (!handle || webSocketBusy) return;
      setError(null);
      try {
        await handle.ping(value, encoding);
      } catch (cause) {
        setError(safeWebSocketUiError(cause));
      }
    });

  const onWebSocketClose = (code: number | undefined, reason: string) =>
    runWebSocketAction(async () => {
      const handle = webSocketHandleRef.current;
      if (!handle || webSocketBusy) return;
      setError(null);
      try {
        await handle.close(code, reason);
      } catch (cause) {
        setError(safeWebSocketUiError(cause));
      }
    });

  const onWebSocketSaveBinary = (messageId: number) =>
    runWebSocketAction(async () => {
      const handle = webSocketHandleRef.current;
      if (!handle || webSocketBusy) return;
      setError(null);
      try {
        await handle.saveBinary(messageId);
      } catch (cause) {
        setError(safeWebSocketUiError(cause));
      }
    });

  const onApplyHandoff = async () => {
    if (!handoffPreviewRef.current || handoffBusyRef.current) return;
    handoffBusyRef.current = true;
    setError(null);
    try {
      await handoffFlow.confirm();
    } finally {
      handoffBusyRef.current = false;
    }
  };
  const onCancelHandoff = async () => {
    if (!handoffPreviewRef.current || handoffBusyRef.current) return;
    handoffBusyRef.current = true;
    setError(null);
    try {
      await resetHandoff();
    } finally {
      handoffBusyRef.current = false;
    }
  };

  cancelHandoffRef.current = onCancelHandoff;

  useEffect(() => {
    const preview = handoffPreview;
    const dialog = handoffDialogRef.current;
    if (!preview || !dialog) return undefined;

    const active = document.activeElement;
    const previous = active instanceof HTMLElement && !dialog.contains(active) ? active : null;
    handoffPreviousFocusRef.current = previous;

    const focusableElements = () =>
      Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      );
    handoffCancelButtonRef.current?.focus();

    const onDialogKeyDown = (event: KeyboardEvent) => {
      if (isImeComposing(event)) return;
      if (event.key === "Escape") {
        event.preventDefault();
        if (!handoffBusyRef.current) void cancelHandoffRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = focusableElements();
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!dialog.contains(document.activeElement)) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    dialog.addEventListener("keydown", onDialogKeyDown);
    return () => {
      dialog.removeEventListener("keydown", onDialogKeyDown);
      if (!handoffPreviewRef.current && handoffPreviousFocusRef.current === previous) {
        previous?.focus();
        handoffPreviousFocusRef.current = null;
      }
    };
  }, [handoffPreview]);

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

  const responseText = resp?.is_json && pretty ? tryPretty(resp.body) : (resp?.body ?? "");
  const copyRawResponse = (kind: RawResponseCopyKind, responseId: string) =>
    kind === "headers" ? copyRawResponseHeaders(responseId) : copyRawResponseCookies(responseId);
  const saveBinaryResponse = async (responseId: string): Promise<boolean> => {
    try {
      return await saveResponseBinary(responseId);
    } catch {
      setError("binary 응답을 안전하게 저장하지 못했습니다.");
      return false;
    }
  };
  const contextItems = useMemo<readonly ContextMenuEntry[]>(
    () =>
      buildRequestItemContextMenu(
        contextActionBusy || collSaving || sending || transferBusy || environmentBusy || !persistenceReady,
      ),
    [collSaving, contextActionBusy, environmentBusy, persistenceReady, sending, transferBusy],
  );

  const runContextAction = async (action: () => Promise<void>, failureMessage: string) => {
    setContextActionBusy(true);
    setPersistenceWarning(null);
    try {
      await action();
    } catch (storageCause) {
      if (storageCause instanceof Error && storageCause.name === "store_revision_conflict") {
        const reloaded = await migrateHistoryStorage();
        if (!reloaded.failed && mountedRef.current) setHistory(reloaded.store.history);
      }
      setPersistenceWarning(storageFailureMessage(storageCause, failureMessage));
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
    }, "기록 복제 상태를 안전하게 저장하지 못했습니다.");
  };

  const renameHistory = (item: HistoryItem) => {
    const name = window.prompt("기록 이름 변경", item.name ?? item.request.url);
    if (name === null) return;
    if (!name.trim()) {
      setPersistenceWarning("기록 이름은 비워둘 수 없습니다.");
      return;
    }
    void runContextAction(async () => {
      await persistHistory(renameHistoryItem({ ...emptyHistoryStore(), history }, item.id, name));
    }, "기록 이름을 안전하게 저장하지 못했습니다.");
  };

  const deleteHistory = (item: HistoryItem) => {
    const label = (item.name ?? item.request.url) || "(no url)";
    if (!window.confirm(`'${label}' 기록을 삭제할까요? 이 작업은 되돌릴 수 없습니다.`)) return;
    void runContextAction(async () => {
      await persistHistory(removeHistoryItem({ ...emptyHistoryStore(), history }, item.id));
    }, "기록 삭제 상태를 안전하게 저장하지 못했습니다.");
  };

  const duplicateCollection = (item: CollectionEntry) => {
    void runContextAction(async () => {
      const safe = await persistCollections(
        duplicateEntry(
          collectionStoreRef.current,
          item.id,
          Date.now(),
          () => `c-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
        ),
      );
      setSelectedCollectionId(safe.collections[0]?.id ?? null);
    }, "컬렉션 복제 상태를 안전하게 저장하지 못했습니다.");
  };

  const renameCollection = (item: CollectionEntry) => {
    const name = window.prompt("컬렉션 이름 변경", item.name);
    if (name === null) return;
    if (!name.trim()) {
      setPersistenceWarning("컬렉션 이름은 비워둘 수 없습니다.");
      return;
    }
    void runContextAction(async () => {
      await persistCollections(renameEntry(collectionStoreRef.current, item.id, name));
    }, "컬렉션 이름을 안전하게 저장하지 못했습니다.");
  };

  const deleteCollection = (item: CollectionEntry) => {
    if (!window.confirm(`'${item.name}' 컬렉션을 삭제할까요? 이 작업은 되돌릴 수 없습니다.`)) return;
    void runContextAction(async () => {
      await persistCollections(removeEntry(collectionStoreRef.current, item.id));
    }, "컬렉션 삭제 상태를 안전하게 저장하지 못했습니다.");
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

  const previewRecord =
    savedPreview?.kind === "collection"
      ? collections.collections.find((item) => item.id === savedPreview.id)
      : history.find((item) => item.id === savedPreview?.id);
  const applySavedPreview = () => {
    if (!previewRecord || !savedPreview || sending || !persistenceReady) return;
    setReq(toRequestTemplate(previewRecord.request));
    setRequestEditorRevision((revision) => revision + 1);
    setResp(null);
    if (savedPreview.kind === "collection") setSelectedCollectionId(savedPreview.id);
    else setSelectedHistoryId(savedPreview.id);
    setPersistenceWarning("저장된 요청입니다. 마스킹된 값과 필요한 환경·인증을 확인하고 직접 전송하세요.");
    setSavedPreview(null);
  };

  return (
    <div className="app">
      {previewRecord && savedPreview && !handoffPreview && (
        <SavedRequestPreview
          title={
            "name" in previewRecord && previewRecord.name ? previewRecord.name : historyDisplayLabel(previewRecord)
          }
          request={previewRecord.request}
          canApply={persistenceReady && !sending && !contextActionBusy && !transferBusy}
          onClose={() => setSavedPreview(null)}
          onApply={applySavedPreview}
        />
      )}
      <input
        ref={browserImportInputRef}
        className="transfer-input"
        type="file"
        accept="application/json,.json"
        aria-label="JSON 파일 가져오기"
        onChange={onBrowserImportFile}
      />
      {handoffPreview && (
        <RequestHandoffDialog
          handoffDialogRef={handoffDialogRef}
          handoffPreview={handoffPreview}
          handoffCancelButtonRef={handoffCancelButtonRef}
          handoffBusy={handoffBusy}
          onCancelHandoff={onCancelHandoff}
          onApplyHandoff={onApplyHandoff}
        />
      )}
      <RequestSidebar
        section={section}
        historyQuery={historyQuery}
        setHistoryQuery={setHistoryQuery}
        historyMethod={historyMethod}
        setHistoryMethod={setHistoryMethod}
        historyMethods={historyMethods}
        historyStatus={historyStatus}
        setHistoryStatus={setHistoryStatus}
        visibleHistory={visibleHistory}
        selectedHistoryId={selectedHistoryId}
        setSavedPreview={setSavedPreview}
        setSelectedHistoryId={setSelectedHistoryId}
        setReq={setReq}
        setRequestEditorRevision={setRequestEditorRevision}
        setPersistenceWarning={setPersistenceWarning}
        setResp={setResp}
        historyContextMenu={historyContextMenu}
        history={history}
        persistenceReady={persistenceReady}
        transferBusy={transferBusy}
        browserImportKind={browserImportKind}
        environmentBusy={environmentBusy}
        sending={sending}
        collSaving={collSaving}
        contextActionBusy={contextActionBusy}
        onExportTransfer={onExportTransfer}
        onImportTransfer={onImportTransfer}
        collName={collName}
        setCollName={setCollName}
        collFolder={collFolder}
        setCollFolder={setCollFolder}
        req={req}
        onSaveCollection={onSaveCollection}
        collections={collections}
        collFilter={collFilter}
        setCollFilter={setCollFilter}
        apiWorkspace={apiWorkspace}
        selectedCollectionId={selectedCollectionId}
        setSelectedCollectionId={setSelectedCollectionId}
        collectionContextMenu={collectionContextMenu}
        deleteCollection={deleteCollection}
        envName={envName}
        setEnvName={setEnvName}
        onCreateEnv={onCreateEnv}
        envStore={envStore}
        currentEnvId={currentEnvId}
        setCurrentEnvId={setCurrentEnvId}
        tryPersistEnvs={tryPersistEnvs}
        envStoreRef={envStoreRef}
        currentEnv={currentEnv}
        startSecretSeal={startSecretSeal}
      />

      <main className="content">
        {!!section && (
          <ApiWorkspacePanel
            collections={collections.collections}
            environments={envStore.environments}
            definitions={openApiDefinitions}
            onChange={setApiWorkspace}
          />
        )}
        {!!section && (
          <OpenApiDefinitions
            revision={openApiDefinitionRevision}
            linkedIds={apiWorkspace?.links.openApiDefinitionIds ?? null}
            onSummaries={setOpenApiDefinitions}
            disabled={!persistenceReady || sending || sseActive || contextActionBusy || transferBusy}
            onApply={(request) => {
              applyOpenApiRequest(request);
              setPersistenceWarning("보관한 OpenAPI 초안입니다. 환경·인증을 다시 확인한 뒤 직접 전송하세요.");
              onNavigate?.("requests");
            }}
          />
        )}
        <nav hidden={!!section} className="workspace-tabs" aria-label="API Playground 작업 공간">
          <button
            type="button"
            className={workspace === "http" ? "active" : ""}
            aria-current={workspace === "http" ? "page" : undefined}
            onClick={() => setWorkspace("http")}
          >
            HTTP · Streams
          </button>
          <button
            type="button"
            className={workspace === "protocol" ? "active" : ""}
            aria-current={workspace === "protocol" ? "page" : undefined}
            onClick={() => setWorkspace("protocol")}
          >
            Protocol Lab
          </button>
        </nav>
        {section === "history" && (
          <Suspense fallback={<p role="status">기록을 불러오고 있습니다…</p>}>
            <HistoryConsole
              history={history}
              activity={{
                sending,
                sse: sseStateLabel(sseState),
                sseEvents: sseEvents.length,
                websocket: webSocketState,
                websocketMessages: webSocketMessages.length,
              }}
              canApply={persistenceReady && !sending && !contextActionBusy && !transferBusy}
              onApply={(item) => {
                setSelectedHistoryId(item.id);
                setReq(toRequestTemplate(item.request));
                setRequestEditorRevision((revision) => revision + 1);
                setResp(null);
                setPersistenceWarning("마스킹된 기록입니다. 필요한 환경·인증을 확인하고 직접 전송하세요.");
                onNavigate?.("requests");
              }}
            />
          </Suspense>
        )}
        {(protocolVisited || workspace === "protocol") && (
          <div hidden={workspace !== "protocol"}>
            <Suspense fallback={<p role="status">프로토콜 화면을 불러오고 있습니다…</p>}>
              <ProtocolLab environment={currentEnv?.variables ?? []} native={isTauri()} />
            </Suspense>
          </div>
        )}
        {workspace !== "protocol" && section !== "history" && (
          <>
            {migrationNotice && <div className="migration-notice">{migrationNotice}</div>}
            {persistenceWarning && <div className="persistence-warning">{persistenceWarning}</div>}
            <div className="request-bar">
              <select
                aria-label="HTTP method"
                className="method-select"
                value={req.method}
                onChange={(e) => setReq({ ...req, method: e.currentTarget.value })}
              >
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
              <button
                className="btn send"
                onClick={() => (sending ? onCancel() : void onSend())}
                disabled={
                  !persistenceReady ||
                  transferBusy ||
                  contextActionBusy ||
                  (!sending && (sseActive || !req.url || Boolean(requestConfigurationError)))
                }
              >
                {!persistenceReady ? "확인 중…" : sending ? "취소" : "보내기"}
              </button>
              <button
                className={`btn ${showCurl ? "active" : ""}`}
                onClick={() => setShowCurl((v) => !v)}
                disabled={!req.url || Boolean(requestConfigurationError)}
              >
                cURL
              </button>
              <button
                className="btn"
                type="button"
                onClick={() => setShowOpenApiImport(true)}
                disabled={!persistenceReady || transferBusy || sending || sseActive || contextActionBusy || collSaving}
              >
                OpenAPI
              </button>
            </div>

            <SseControls
              onStartSse={onStartSse}
              persistenceReady={persistenceReady}
              sending={sending}
              sseActive={sseActive}
              contextActionBusy={contextActionBusy}
              req={req}
              requestConfigurationError={requestConfigurationError}
              sseState={sseState}
              onStopSse={onStopSse}
              sseHandleRef={sseHandleRef}
              sseOptions={sseOptions}
              setSseOptions={setSseOptions}
            />

            {showCurl && !requestConfigurationError && (
              <div className="curl-panel">
                <div className="io-label">
                  cURL
                  <button className="copy-btn" onClick={() => void navigator.clipboard.writeText(buildCurl(req))}>
                    마스킹 복사
                  </button>
                  <button
                    className="copy-btn"
                    onClick={() => void copyRevealedCurl(req, currentEnv?.variables ?? [], setError)}
                  >
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
              <div className="error" role="alert">
                {cookieConfigurationError}
              </div>
            )}
            {!cookieConfigurationError && multipartIssue && (
              <div className="error" role="alert">
                {multipartIssue.index + 1}번 multipart part: {multipartIssue.message}
              </div>
            )}

            <RequestParameters
              tab={tab}
              req={req}
              setReq={setReq}
              currentEnv={currentEnv}
              requestEditorRevision={requestEditorRevision}
              BODY_KINDS={BODY_KINDS}
              setAuth={setAuth}
              AUTH_KINDS={AUTH_KINDS}
            />

            <WebSocketPanel
              state={webSocketState}
              messages={webSocketMessages}
              dropped={webSocketDropped}
              native={isTauri()}
              canConnect={persistenceReady && Boolean(req.url.trim()) && !contextActionBusy}
              busy={webSocketBusy || sending || contextActionBusy}
              onConnect={() => void onWebSocketConnect()}
              onDisconnect={() => void onWebSocketDisconnect()}
              onSend={(kind, value, encoding) => void onWebSocketSend(kind, value, encoding)}
              onPing={(value, encoding) => void onWebSocketPing(value, encoding)}
              onClose={(code, reason) => void onWebSocketClose(code, reason)}
              onSaveBinary={(messageId) => void onWebSocketSaveBinary(messageId)}
            />

            {graphqlIssue && (
              <div className="error" role="alert">
                {graphqlIssue}
              </div>
            )}
            {error && (
              <div className="error" role="alert">
                {error}
              </div>
            )}

            <ResponseViewer
              response={resp}
              responseText={responseText}
              pretty={pretty}
              onPrettyChange={setPretty}
              onRawCopy={copyRawResponse}
              onBinarySave={saveBinaryResponse}
              onSendSelection={api.sendSelectionToToolbox}
              native={isTauri()}
              onError={setError}
            />
            <SseEventViewer
              events={sseEvents}
              dropped={sseDropped}
              paused={ssePaused}
              onPauseChange={setSsePaused}
              onError={setError}
            />
          </>
        )}
      </main>
      <ContextMenu
        open={historyContextMenu.open}
        anchor={historyContextMenu.anchor}
        restoreFocusTo={historyContextMenu.restoreFocusTo}
        items={contextItems}
        onSelect={onHistoryContextSelect}
        onClose={historyContextMenu.close}
        ariaLabel="기록 메뉴"
      />
      <ContextMenu
        open={collectionContextMenu.open}
        anchor={collectionContextMenu.anchor}
        restoreFocusTo={collectionContextMenu.restoreFocusTo}
        items={contextItems}
        onSelect={onCollectionContextSelect}
        onClose={collectionContextMenu.close}
        ariaLabel="컬렉션 메뉴"
      />
      {showOpenApiImport && (
        <Suspense fallback={<p role="status">OpenAPI 가져오기를 준비하고 있습니다…</p>}>
          <OpenApiImport
            onClose={() => setShowOpenApiImport(false)}
            onApply={onOpenApiApply}
            onAddToCollection={onOpenApiAddToCollection}
            environment={JSON.stringify(envStore)}
            onSavedDefinition={() => setOpenApiDefinitionRevision((value) => value + 1)}
          />
        </Suspense>
      )}
    </div>
  );
}
