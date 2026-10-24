/** 열린 요청 탭(초안은 화면 메모리에만), 고른 환경, 실행기 세션. */
import { create } from "zustand";
import type { CollectionRoot, Pair } from "../../../rpc/gen/rpc";
import type { SentResponse } from "./backend";
import { emptyDraft, type RequestDraft } from "./model/draft";
import { draftFromFile, fileFromDraft } from "./model/fileDraft";
import { type ParsedRequestFile, parseRequestFile } from "./model/requestFile";
import { Session } from "./runner/session";
import type { ResponseOriginInfo } from "./sendState";

export interface SendResult {
  response: SentResponse;
  origin: ResponseOriginInfo;
}

export interface ApiTab {
  /** 탭이 열려 있는 동안 바뀌지 않는 id(송신·화면 key). `key`는 저장 경로가 바뀌면 같이 바뀐다. */
  id: string;
  key: string;
  root: CollectionRoot;
  path: string | null;
  name: string;
  draft: RequestDraft;
  digest: string | null;
  parsed: ParsedRequestFile | null;
  /** 마지막 저장(또는 읽기) 때의 내용. 지금 초안과 다르면 dirty. */
  saved: string;
  result?: SendResult;
  failure?: { code: string; detail?: unknown };
  notice?: string;
  sending?: boolean;
  /** 프로젝트 파일을 이 PC에서 그 digest로 검토했는지(개인 루트는 늘 참). 거짓이면 첫 저장 전에 확인한다. */
  reviewed: boolean;
  /** 마지막 저장이 `api.collection_conflict`였음([다시 읽기]를 보인다). */
  conflict?: boolean;
  /** 보내기 전에 검토 확인 창을 띄운다(검토하지 않은 프로젝트 파일·재검토 표시가 있는 파일). */
  askSendReview?: boolean;
}

/** 실행기 `reviewNeeded`와 같은 기준: 이 PC에서 digest를 검토하지 않았거나 파일에 재검토 표시가 있음. */
export function sendReviewNeeded(tab: ApiTab): boolean {
  return !tab.reviewed || tab.parsed?.file.requiresSecretReview === true;
}

/** 고른 환경. `unreviewed`면 검토하지 않은 프로젝트 환경 파일이라 송신·실행에 쓰지 않는다. */
export interface ActiveEnvironment {
  name: string;
  variables: Pair[];
  unreviewed?: boolean;
}

const print = (draft: RequestDraft, name: string) => JSON.stringify(fileFromDraft(draft, name));
export const rootKey = (root: CollectionRoot) => (root.kind === "project" ? `project:${root.projectId}` : "personal");
export const tabKey = (root: CollectionRoot, path: string) => `${rootKey(root)}:${path}`;
export function isDirty(tab: ApiTab): boolean {
  return tab.path === null || print(tab.draft, tab.name) !== tab.saved;
}

interface ApiTabs {
  tabs: ApiTab[];
  active?: string;
  environment: ActiveEnvironment | null;
  session: Session;
  openFile(root: CollectionRoot, path: string, text: string, digest: string, reviewed?: boolean): string;
  openNew(root: CollectionRoot): string;
  select(key: string): void;
  close(key: string): void;
  update(key: string, patch: Partial<Omit<ApiTab, "key" | "id">>): void;
  updateById(id: string, patch: Partial<Omit<ApiTab, "key" | "id">>): void;
  /** 탭 id로 초안을 바꾼다(비동기 작업이 끝났을 때 그 사이 key가 바뀌었어도 같은 탭에 반영). */
  setDraftById(id: string, draft: RequestDraft): void;
  /** 파일을 다시 읽어 초안을 바꾼다(저장 충돌 뒤 [다시 읽기]). */
  reload(id: string, file: { text: string; digest: string; reviewed: boolean }): void;
  setDraft(key: string, draft: RequestDraft): void;
  /** 저장 완료. `saved`는 실제로 쓴 초안으로 계산한다(쓰는 동안 고친 내용은 dirty로 남는다). */
  markSaved(id: string, saved: { path: string; name: string; digest: string; text: string; draft: RequestDraft }): void;
  setEnvironment(environment: ActiveEnvironment | null): void;
  reset(): void;
}

let untitled = 0;
let nextTabId = 0;
const newTabId = () => `tab-${++nextTabId}`;

/** 진행 중인 송신. 편집기가 다시 그려지거나 탭 key가 바뀌어도 잃지 않게 탭 id로 store 밖(모듈)에 둔다. */
const inflight = new Map<string, AbortController>();
export function beginSend(id: string): AbortController | null {
  if (inflight.has(id)) return null;
  const controller = new AbortController();
  inflight.set(id, controller);
  return controller;
}
export function endSend(id: string, controller: AbortController): void {
  if (inflight.get(id) === controller) inflight.delete(id);
}
export function abortSend(id: string): void {
  inflight.get(id)?.abort();
}
export const isSending = (id: string) => inflight.has(id);
export const useApiTabs = create<ApiTabs>((set, get) => ({
  tabs: [],
  environment: null,
  session: new Session(),
  openFile(root, path, text, digest, reviewed = true) {
    const key = tabKey(root, path);
    if (!get().tabs.some((tab) => tab.key === key)) {
      const parsed = parseRequestFile(text);
      const draft = draftFromFile(parsed);
      const tab: ApiTab = {
        id: newTabId(),
        key,
        root,
        path,
        name: parsed.file.name,
        draft,
        digest,
        parsed,
        saved: print(draft, parsed.file.name),
        reviewed: root.kind === "personal" || reviewed,
      };
      set({ tabs: [...get().tabs, tab] });
    }
    set({ active: key });
    return key;
  },
  openNew(root) {
    untitled += 1;
    const key = `${rootKey(root)}:#new-${untitled}`;
    const draft = emptyDraft();
    const name = `새 요청 ${untitled}`;
    set({
      tabs: [
        ...get().tabs,
        { id: newTabId(), key, root, path: null, name, draft, digest: null, parsed: null, saved: "", reviewed: true },
      ],
      active: key,
    });
    return key;
  },
  select: (key) => set({ active: key }),
  close(key) {
    const closing = get().tabs.find((tab) => tab.key === key);
    if (closing) abortSend(closing.id);
    const tabs = get().tabs.filter((tab) => tab.key !== key);
    set({ tabs, active: get().active === key ? tabs.at(-1)?.key : get().active });
  },
  update: (key, patch) => set({ tabs: get().tabs.map((tab) => (tab.key === key ? { ...tab, ...patch } : tab)) }),
  updateById: (id, patch) => set({ tabs: get().tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) }),
  setDraft: (key, draft) => get().update(key, { draft, notice: undefined }),
  setDraftById: (id, draft) => get().updateById(id, { draft, notice: undefined }),
  reload(id, file) {
    const parsed = parseRequestFile(file.text);
    const draft = draftFromFile(parsed);
    get().updateById(id, {
      draft,
      parsed,
      name: parsed.file.name,
      digest: file.digest,
      saved: print(draft, parsed.file.name),
      reviewed: get().tabs.find((tab) => tab.id === id)?.root.kind === "personal" || file.reviewed,
      conflict: false,
      notice: undefined,
    });
  },
  markSaved(id, saved) {
    const parsed = parseRequestFile(saved.text);
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab) return;
    const newKey = tabKey(tab.root, saved.path);
    set({
      tabs: get().tabs.map((t) =>
        t.id === id
          ? {
              ...t,
              key: newKey,
              path: saved.path,
              name: saved.name,
              digest: saved.digest,
              parsed,
              saved: print(saved.draft, saved.name),
              reviewed: true,
              conflict: false,
            }
          : t,
      ),
      active: get().active === tab.key ? newKey : get().active,
    });
  },
  setEnvironment: (environment) => set({ environment }),
  reset() {
    for (const tab of get().tabs) abortSend(tab.id);
    set({ tabs: [], active: undefined, environment: null, session: new Session() });
  },
}));

export function useActiveTab(): ApiTab | undefined {
  return useApiTabs((state) => state.tabs.find((tab) => tab.key === state.active));
}
