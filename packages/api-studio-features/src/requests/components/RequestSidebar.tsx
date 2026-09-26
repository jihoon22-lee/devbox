import { isKeyboardActivation } from "@devbox/a11y";
import { foldersOf } from "../lib/collections";
import { removeEnvironment, setVariable } from "../lib/environments";
import { toRequestTemplate } from "../lib/persistence";
import {
  historyDisplayLabel,
  historyMethod as historyMethodOf,
  MAX_HISTORY_QUERY_CHARS,
  type HistoryStatusFilter,
} from "../lib/history";
import type * as React from "react";

interface Props {
  section: "requests" | "protocols" | "history" | undefined;
  historyQuery: string;
  setHistoryQuery: React.Dispatch<React.SetStateAction<string>>;
  historyMethod: string;
  setHistoryMethod: React.Dispatch<React.SetStateAction<string>>;
  historyMethods: string[];
  historyStatus: import("../lib/history").HistoryStatusFilter;
  setHistoryStatus: React.Dispatch<React.SetStateAction<import("../lib/history").HistoryStatusFilter>>;
  visibleHistory: import("../types").HistoryItem[];
  selectedHistoryId: string | null;
  setSavedPreview: React.Dispatch<React.SetStateAction<{ kind: "history" | "collection"; id: string } | null>>;
  setSelectedHistoryId: React.Dispatch<React.SetStateAction<string | null>>;
  setReq: React.Dispatch<React.SetStateAction<import("../types").RequestTemplate>>;
  setRequestEditorRevision: React.Dispatch<React.SetStateAction<number>>;
  setPersistenceWarning: React.Dispatch<React.SetStateAction<string | null>>;
  setResp: React.Dispatch<React.SetStateAction<import("../../generated/ApiResponse").ApiResponse | null>>;
  historyContextMenu: import("@devbox/context-menu").ContextMenuController;
  history: import("../types").HistoryItem[];
  persistenceReady: boolean;
  transferBusy: boolean;
  browserImportKind: "collection" | "environment" | null;
  environmentBusy: boolean;
  sending: boolean;
  collSaving: boolean;
  contextActionBusy: boolean;
  onExportTransfer: (kind: "collection" | "environment") => void;
  onImportTransfer: (kind: "collection" | "environment") => void;
  collName: string;
  setCollName: React.Dispatch<React.SetStateAction<string>>;
  collFolder: string;
  setCollFolder: React.Dispatch<React.SetStateAction<string>>;
  req: import("../types").RequestTemplate;
  onSaveCollection: () => Promise<void>;
  collections: import("../lib/collections").CollectionStore;
  collFilter: string;
  setCollFilter: React.Dispatch<React.SetStateAction<string>>;
  apiWorkspace: import("../ApiWorkspace").ApiWorkspace | null;
  selectedCollectionId: string | null;
  setSelectedCollectionId: React.Dispatch<React.SetStateAction<string | null>>;
  collectionContextMenu: import("@devbox/context-menu").ContextMenuController;
  deleteCollection: (item: import("../lib/collections").CollectionEntry) => void;
  envName: string;
  setEnvName: React.Dispatch<React.SetStateAction<string>>;
  onCreateEnv: () => void;
  envStore: import("../lib/environments").EnvironmentStore;
  currentEnvId: string;
  setCurrentEnvId: React.Dispatch<React.SetStateAction<string>>;
  tryPersistEnvs: (
    store: import("../lib/environments").EnvironmentStore,
    expectedRevision?: number,
    allowEnvironmentBusy?: boolean,
  ) => Promise<import("../lib/environments").EnvironmentStore | null>;
  envStoreRef: React.RefObject<import("../lib/environments").EnvironmentStore>;
  currentEnv: import("../lib/environments").Environment | null;
  startSecretSeal: (
    environmentId: string,
    key: string,
    plain: string,
    expectedValue: string,
    expectedSecret: boolean,
    secret: boolean,
  ) => void;
}

export function RequestSidebar({
  section,
  historyQuery,
  setHistoryQuery,
  historyMethod,
  setHistoryMethod,
  historyMethods,
  historyStatus,
  setHistoryStatus,
  visibleHistory,
  selectedHistoryId,
  setSavedPreview,
  setSelectedHistoryId,
  setReq,
  setRequestEditorRevision,
  setPersistenceWarning,
  setResp,
  historyContextMenu,
  history,
  persistenceReady,
  transferBusy,
  browserImportKind,
  environmentBusy,
  sending,
  collSaving,
  contextActionBusy,
  onExportTransfer,
  onImportTransfer,
  collName,
  setCollName,
  collFolder,
  setCollFolder,
  req,
  onSaveCollection,
  collections,
  collFilter,
  setCollFilter,
  apiWorkspace,
  selectedCollectionId,
  setSelectedCollectionId,
  collectionContextMenu,
  deleteCollection,
  envName,
  setEnvName,
  onCreateEnv,
  envStore,
  currentEnvId,
  setCurrentEnvId,
  tryPersistEnvs,
  envStoreRef,
  currentEnv,
  startSecretSeal,
}: Props) {
  return (
    <aside className="sidebar" hidden={section === "history"}>
      <h1 className="app-title">{section ? "Requests" : "API Playground"}</h1>
      <div className="group-name">기록</div>
      <div className="history-toolbar" aria-label="기록 검색 및 필터">
        <input
          className="coll-input history-search"
          aria-label="기록 검색"
          placeholder="이름, URL, method 검색"
          value={historyQuery}
          maxLength={MAX_HISTORY_QUERY_CHARS}
          onChange={(event) => setHistoryQuery(event.currentTarget.value)}
          spellCheck={false}
        />
        <div className="history-filter-row">
          <select
            className="coll-filter"
            aria-label="기록 method 필터"
            value={historyMethod}
            onChange={(event) => setHistoryMethod(event.currentTarget.value)}
          >
            <option value="">모든 method</option>
            {historyMethods.map((method) => (
              <option key={method} value={method}>
                {method}
              </option>
            ))}
          </select>
          <select
            className="coll-filter"
            aria-label="기록 상태 필터"
            value={historyStatus}
            onChange={(event) => setHistoryStatus(event.currentTarget.value as HistoryStatusFilter)}
          >
            <option value="all">모든 상태</option>
            <option value="success">성공</option>
            <option value="error">실패</option>
          </select>
        </div>
      </div>
      {visibleHistory.map((h) => (
        <button
          key={h.id}
          className={`history-item ${selectedHistoryId === h.id ? "selected" : ""}`}
          aria-current={selectedHistoryId === h.id ? "true" : undefined}
          aria-label={`기록 항목: ${historyDisplayLabel(h)}`}
          data-history-id={h.id}
          onClick={() => {
            if (section) {
              setSavedPreview({ kind: "history", id: h.id });
              return;
            }
            setSelectedHistoryId(h.id);
            setReq(toRequestTemplate(h.request));
            setRequestEditorRevision((revision) => revision + 1);
            if (h.request.requiresSecretReview) {
              setPersistenceWarning("마스킹된 기록입니다. 민감한 값을 환경 변수 참조로 다시 설정하세요.");
            }
            setResp(null);
          }}
          {...historyContextMenu.triggerProps}
        >
          <span className={`method ${historyMethodOf(h).toLowerCase()}`}>{historyMethodOf(h)}</span>
          <span className="history-url" title={historyDisplayLabel(h)}>
            {historyDisplayLabel(h)}
          </span>
        </button>
      ))}
      {history.length === 0 && <div className="dim">아직 요청이 없습니다</div>}
      {history.length > 0 && visibleHistory.length === 0 && (
        <div className="dim" role="status" aria-live="polite">
          검색 결과 없음
        </div>
      )}

      <div className="group-name">컬렉션</div>
      <div className="transfer-actions" aria-label="컬렉션 JSON 전송">
        <button
          type="button"
          className="btn mini"
          disabled={
            !persistenceReady ||
            transferBusy ||
            Boolean(browserImportKind) ||
            environmentBusy ||
            sending ||
            collSaving ||
            contextActionBusy
          }
          onClick={() => onExportTransfer("collection")}
        >
          JSON 내보내기
        </button>
        <button
          type="button"
          className="btn mini"
          disabled={
            !persistenceReady ||
            transferBusy ||
            Boolean(browserImportKind) ||
            environmentBusy ||
            sending ||
            collSaving ||
            contextActionBusy
          }
          onClick={() => onImportTransfer("collection")}
        >
          JSON 가져오기
        </button>
      </div>
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
        <button
          className="btn"
          disabled={
            !persistenceReady || transferBusy || environmentBusy || collSaving || contextActionBusy || !req.url.trim()
          }
          onClick={() => void onSaveCollection()}
        >
          저장
        </button>
      </div>
      {foldersOf(collections).length > 0 && (
        <select
          className="coll-filter"
          aria-label="컬렉션 폴더 필터"
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
        .filter((item) => !apiWorkspace || apiWorkspace.links.collectionIds.includes(item.id))
        .filter((c) => !collFilter || c.folder === collFilter)
        .map((c) => (
          <div
            key={c.id}
            className={`history-item coll-item ${selectedCollectionId === c.id ? "selected" : ""}`}
            title={`${c.folder ? `[${c.folder}] ` : ""}${c.name}`}
            tabIndex={0}
            aria-current={selectedCollectionId === c.id ? "true" : undefined}
            aria-label={`컬렉션 항목: ${c.name}`}
            data-collection-id={c.id}
            onClick={() => setSelectedCollectionId(c.id)}
            onContextMenu={collectionContextMenu.triggerProps.onContextMenu}
            onKeyDown={(event) => {
              collectionContextMenu.triggerProps.onKeyDown?.(event);
              if (event.defaultPrevented || event.target !== event.currentTarget || !isKeyboardActivation(event))
                return;
              event.preventDefault();
              setSelectedCollectionId(c.id);
            }}
          >
            <button
              className="coll-open"
              aria-label={section ? `컬렉션 요청 미리보기: ${c.name}` : undefined}
              onClick={() => {
                if (section) {
                  setSavedPreview({ kind: "collection", id: c.id });
                  return;
                }
                setSelectedCollectionId(c.id);
                setReq(toRequestTemplate(c.request));
                setRequestEditorRevision((revision) => revision + 1);
                if (c.requiresSecretReview) {
                  setPersistenceWarning("안전 변환된 컬렉션입니다. 마스킹된 값을 환경 변수 참조로 다시 설정하세요.");
                }
                setResp(null);
              }}
            >
              <span className={`method ${c.request.method.toLowerCase()}`}>{c.request.method}</span>
              <span className="history-url">
                {c.folder ? `[${c.folder}] ` : ""}
                {c.name}
              </span>
            </button>
            <button
              className="coll-del"
              aria-label={`${c.name} 컬렉션 삭제`}
              disabled={
                contextActionBusy || transferBusy || environmentBusy || collSaving || sending || !persistenceReady
              }
              onClick={() => deleteCollection(c)}
            >
              ✕
            </button>
          </div>
        ))}
      {collections.collections.filter((item) => !apiWorkspace || apiWorkspace.links.collectionIds.includes(item.id))
        .length === 0 && (
        <div className="dim">
          {apiWorkspace ? "이 Workspace에 연결한 컬렉션이 없습니다" : "저장된 컬렉션이 없습니다"}
        </div>
      )}

      <div className="group-name">환경</div>
      <div className="transfer-actions" aria-label="환경 JSON 전송">
        <button
          type="button"
          className="btn mini"
          disabled={
            !persistenceReady ||
            transferBusy ||
            Boolean(browserImportKind) ||
            environmentBusy ||
            sending ||
            collSaving ||
            contextActionBusy
          }
          onClick={() => onExportTransfer("environment")}
        >
          JSON 내보내기
        </button>
        <button
          type="button"
          className="btn mini"
          disabled={
            !persistenceReady ||
            transferBusy ||
            Boolean(browserImportKind) ||
            environmentBusy ||
            sending ||
            collSaving ||
            contextActionBusy
          }
          onClick={() => onImportTransfer("environment")}
        >
          JSON 가져오기
        </button>
      </div>
      <div className="coll-save-row">
        <input
          className="coll-input"
          placeholder="환경 이름 (예: dev)"
          value={envName}
          onChange={(e) => setEnvName(e.currentTarget.value)}
        />
        <button className="btn" disabled={!persistenceReady || transferBusy || environmentBusy} onClick={onCreateEnv}>
          추가
        </button>
      </div>
      {envStore.environments
        .filter((env) => !apiWorkspace || apiWorkspace.links.environmentIds.includes(env.id))
        .map((env) => (
          <div key={env.id} className={`env-item ${env.id === currentEnvId ? "active" : ""}`}>
            <button className="env-name" onClick={() => setCurrentEnvId(env.id)}>
              {env.name}
            </button>
            <button
              className="coll-del"
              disabled={environmentBusy || transferBusy || sending || contextActionBusy || !persistenceReady}
              onClick={() => {
                tryPersistEnvs(removeEnvironment(envStoreRef.current, env.id));
              }}
            >
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
                  <span
                    className={`env-var-secret ${v.value ? "" : "unconfigured"}`}
                    title={v.value ? "봉인됨 — 평문 미보관" : "내보내기에서 secret 원문을 제외해 다시 입력해야 합니다"}
                  >
                    {v.value ? "••••••••" : "미설정"}
                  </span>
                  <button
                    className="btn mini"
                    disabled={environmentBusy || transferBusy || !persistenceReady}
                    onClick={() => {
                      const plain = window.prompt(`${v.key} 새 값 입력`);
                      if (plain != null) {
                        startSecretSeal(currentEnv.id, v.key, plain, v.value, true, true);
                      }
                    }}
                  >
                    변경
                  </button>
                  <button
                    className="btn mini"
                    disabled={environmentBusy || transferBusy || !persistenceReady}
                    title="secret 해제 (저장 값 삭제)"
                    onClick={() => {
                      tryPersistEnvs(setVariable(envStoreRef.current, currentEnv.id, v.key, "", false));
                    }}
                  >
                    해제
                  </button>
                </>
              ) : (
                <>
                  <input
                    className="coll-input"
                    value={v.value}
                    disabled={environmentBusy || transferBusy || !persistenceReady}
                    onChange={(e) => {
                      tryPersistEnvs(
                        setVariable(envStoreRef.current, currentEnv.id, v.key, e.currentTarget.value, false),
                      );
                    }}
                  />
                  <button
                    className="btn mini"
                    disabled={environmentBusy || transferBusy || !persistenceReady}
                    title="이 변수를 봉인해 secret으로 저장"
                    onClick={() => {
                      if (v.value) {
                        startSecretSeal(currentEnv.id, v.key, v.value, v.value, false, true);
                      }
                    }}
                  >
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
              disabled={environmentBusy || transferBusy || !persistenceReady}
              onClick={() => {
                const name = `var${currentEnv.variables.length + 1}`;
                tryPersistEnvs(setVariable(envStoreRef.current, currentEnv.id, name, "", false));
              }}
            >
              + 변수
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}
