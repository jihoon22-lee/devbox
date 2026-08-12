import type { Doc, DocId, EditorState, SessionState, ViewId } from "../types";

export type EditorAction =
  | { type: "addDoc"; doc: Doc; view?: ViewId }
  | { type: "removeDoc"; docId: DocId }
  | { type: "activateView"; view: ViewId }
  | { type: "activateDoc"; view: ViewId; docId: DocId }
  | { type: "moveDoc"; docId: DocId; toView: ViewId }
  | { type: "setDocText"; docId: DocId; text: string }
  | {
      type: "saveDoc";
      docId: DocId;
      mtimeNanos: string;
      size: number;
      contentHash?: string;
      lossy?: boolean;
      durabilityWarning?: string | null;
      /** Snapshot submitted to the native save command. */
      submittedRevision?: number;
      submittedText?: string;
      /** Compatibility aliases for callers that use the shorter names. */
      revision?: number;
      text?: string;
    }
  | { type: "toggleSplit" };

export const EMPTY_VIEWS: [DocId[], DocId[]] = [[], []];

export function createInitialEditorState(): EditorState {
  return {
    docs: [],
    views: [[], []],
    activeView: 0,
    activeDocByView: [null, null],
    workspaceFolder: null,
    recentFiles: [],
    split: false,
  };
}

function viewWithId(state: EditorState, docId: DocId): ViewId | null {
  if (state.views[0].includes(docId)) return 0;
  if (state.views[1].includes(docId)) return 1;
  return null;
}

function lastId(ids: DocId[]): DocId | null {
  return ids.length > 0 ? ids[ids.length - 1] : null;
}

function activeDocAfterRemoval(
  ids: DocId[],
  removedId: DocId,
  current: DocId | null,
): DocId | null {
  const oldIndex = ids.indexOf(removedId);
  const remaining = ids.filter((id) => id !== removedId);
  if (current !== removedId) return current && remaining.includes(current) ? current : lastId(remaining);
  return remaining[Math.min(Math.max(oldIndex, 0), remaining.length - 1)] ?? null;
}

function uniqueIds(ids: DocId[]): DocId[] {
  return [...new Set(ids)];
}

function knownIds(state: EditorState): Set<DocId> {
  return new Set(state.docs.map((doc) => doc.id));
}

function cleanViews(state: EditorState): [DocId[], DocId[]] {
  const ids = knownIds(state);
  return [
    uniqueIds(state.views[0].filter((id) => ids.has(id))),
    uniqueIds(state.views[1].filter((id) => ids.has(id))),
  ];
}

function completeViews(state: EditorState, views: [DocId[], DocId[]], preferredView: ViewId): [DocId[], DocId[]] {
  const known = knownIds(state);
  const result: [DocId[], DocId[]] = [
    uniqueIds(views[0].filter((id) => known.has(id))),
    uniqueIds(views[1].filter((id) => known.has(id))),
  ];
  const placed = new Set([...result[0], ...result[1]]);
  for (const doc of state.docs) {
    if (placed.has(doc.id)) continue;
    result[preferredView].push(doc.id);
    placed.add(doc.id);
  }
  return result;
}

/**
 * Pure editor transitions. The docs registry only appends or filters, while
 * view arrays contain IDs and are the only structures that change on a move.
 */
export function editorReducer(state: EditorState, action: EditorAction): EditorState {
  switch (action.type) {
    case "addDoc": {
      if (state.docs.some((doc) => doc.id === action.doc.id)) {
        const existingView = viewWithId(state, action.doc.id);
        if (existingView !== null) {
          // A duplicate open must not replace the in-memory buffer. Repair a
          // malformed state that placed the ID in both views while retaining
          // the first placement and its normal activation behavior.
          const views = completeViews(state, cleanViews(state), existingView);
          views[1 - existingView] = views[1 - existingView].filter((id) => id !== action.doc.id);
          views[existingView] = [
            action.doc.id,
            ...views[existingView].filter((id) => id !== action.doc.id),
          ];
          return {
            ...state,
            views,
            activeView: existingView,
            activeDocByView: [
              views[0].includes(state.activeDocByView[0] ?? "") ? state.activeDocByView[0] : lastId(views[0]),
              views[1].includes(state.activeDocByView[1] ?? "") ? state.activeDocByView[1] : lastId(views[1]),
            ],
          };
        }

        // A document can only be registered once, but a recovery path may
        // encounter a registry entry that lost its view placement. Put it
        // back exactly once instead of leaving the state invalid.
        const view = action.view ?? state.activeView;
        const views = completeViews(state, cleanViews(state), view);
        if (!views[0].includes(action.doc.id) && !views[1].includes(action.doc.id)) {
          views[view] = [...views[view], action.doc.id];
        }
        const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
        activeDocByView[view] = action.doc.id;
        return {
          ...state,
          views,
          activeView: view,
          activeDocByView,
          split: state.split || view === 1,
        };
      }

      const view = action.view ?? state.activeView;
      const doc = {
        ...action.doc,
        revision: action.doc.revision ?? 0,
      };
      const docs = [...state.docs, doc];
      const views = completeViews({ ...state, docs }, cleanViews(state), view);
      if (!views[0].includes(doc.id) && !views[1].includes(doc.id)) views[view] = [...views[view], doc.id];
      const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
      activeDocByView[0] = views[0].includes(activeDocByView[0] ?? "") ? activeDocByView[0] : lastId(views[0]);
      activeDocByView[1] = views[1].includes(activeDocByView[1] ?? "") ? activeDocByView[1] : lastId(views[1]);
      activeDocByView[view] = action.doc.id;
      return {
        ...state,
        docs,
        views,
        activeView: view,
        activeDocByView,
        split: state.split || view === 1,
        recentFiles: state.recentFiles.includes(doc.path)
          ? state.recentFiles
          : [doc.path, ...state.recentFiles].slice(0, 20),
      };
    }

    case "removeDoc": {
      if (!state.docs.some((doc) => doc.id === action.docId)) return state;
      const views: [DocId[], DocId[]] = [
        state.views[0].filter((id) => id !== action.docId),
        state.views[1].filter((id) => id !== action.docId),
      ];
      const nextDocs = state.docs.filter((doc) => doc.id !== action.docId);
      const nextState = { ...state, docs: nextDocs };
      const completedViews = completeViews(nextState, views, state.activeView);
      const activeAfterFirstRemoval = activeDocAfterRemoval(
        state.views[0],
        action.docId,
        state.activeDocByView[0],
      );
      const activeAfterSecondRemoval = activeDocAfterRemoval(
        state.views[1],
        action.docId,
        state.activeDocByView[1],
      );
      const activeDocByView: [DocId | null, DocId | null] = [
        activeAfterFirstRemoval && completedViews[0].includes(activeAfterFirstRemoval)
          ? activeAfterFirstRemoval
          : lastId(completedViews[0]),
        activeAfterSecondRemoval && completedViews[1].includes(activeAfterSecondRemoval)
          ? activeAfterSecondRemoval
          : lastId(completedViews[1]),
      ];
      const activeView = activeDocByView[state.activeView] !== null
        ? state.activeView
        : activeDocByView[1 - state.activeView] !== null
          ? (1 - state.activeView) as ViewId
          : state.activeView;
      return {
        ...state,
        docs: nextDocs,
        views: completedViews,
        activeDocByView,
        activeView,
      };
    }

    case "activateView": {
      const views = completeViews(state, cleanViews(state), action.view);
      const activeDoc = state.activeDocByView[action.view] && views[action.view].includes(state.activeDocByView[action.view] as DocId)
        ? state.activeDocByView[action.view]
        : lastId(views[action.view]);
      const activeDocByView: [DocId | null, DocId | null] = [
        views[0].includes(state.activeDocByView[0] ?? "") ? state.activeDocByView[0] : lastId(views[0]),
        views[1].includes(state.activeDocByView[1] ?? "") ? state.activeDocByView[1] : lastId(views[1]),
      ];
      activeDocByView[action.view] = activeDoc;
      return {
        ...state,
        views,
        activeView: action.view,
        activeDocByView,
      };
    }

    case "activateDoc": {
      if (!state.views[action.view].includes(action.docId)) return state;
      const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
      activeDocByView[action.view] = action.docId;
      return { ...state, activeView: action.view, activeDocByView };
    }

    case "moveDoc": {
      const source = viewWithId(state, action.docId);
      if (source === null) {
        if (!state.docs.some((doc) => doc.id === action.docId)) return state;
        const views = completeViews(state, cleanViews(state), action.toView);
        if (!views[0].includes(action.docId) && !views[1].includes(action.docId)) {
          views[action.toView] = [...views[action.toView], action.docId];
        }
        const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
        activeDocByView[action.toView] = action.docId;
        return {
          ...state,
          views,
          activeView: action.toView,
          activeDocByView,
          split: true,
        };
      }
      if (source === action.toView) {
        const views = completeViews(state, cleanViews(state), action.toView);
        views[action.toView] = [...views[action.toView].filter((id) => id !== action.docId), action.docId];
        const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
        activeDocByView[action.toView] = action.docId;
        return { ...state, views, activeView: action.toView, activeDocByView };
      }
      const views: [DocId[], DocId[]] = [
        state.views[0].filter((id) => id !== action.docId),
        state.views[1].filter((id) => id !== action.docId),
      ];
      views[action.toView] = [...views[action.toView], action.docId];
      const completedViews = completeViews(state, views, action.toView);
      const activeDocByView: [DocId | null, DocId | null] = [...state.activeDocByView];
      activeDocByView[source] = activeDocAfterRemoval(
        state.views[source],
        action.docId,
        state.activeDocByView[source],
      );
      if (!activeDocByView[source] || !completedViews[source].includes(activeDocByView[source]!)) {
        activeDocByView[source] = lastId(completedViews[source]);
      }
      activeDocByView[action.toView] = action.docId;
      return {
        ...state,
        views: completedViews,
        activeView: action.toView,
        activeDocByView,
        split: true,
      };
    }

    case "setDocText":
      return state.docs.some((doc) => doc.id === action.docId)
        ? {
            ...state,
            docs: state.docs.map((doc) =>
              doc.id === action.docId && doc.text !== action.text
                ? { ...doc, text: action.text, dirty: true, revision: (doc.revision ?? 0) + 1 }
                : doc,
            ),
          }
        : state;

    case "saveDoc":
      return state.docs.some((doc) => doc.id === action.docId)
        ? {
            ...state,
            docs: state.docs.map((doc) =>
              doc.id !== action.docId
                ? doc
                : (() => {
                    const submittedRevision = action.submittedRevision ?? action.revision;
                    const submittedText = action.submittedText ?? action.text;
                    const matchesSubmittedSnapshot =
                      submittedRevision === undefined ||
                      (doc.revision === submittedRevision &&
                        submittedText !== undefined &&
                        doc.text === submittedText);
                    return {
                      ...doc,
                      // The disk snapshot is valid even when a newer local
                      // edit arrived while the save was in flight. Only the
                      // matching submitted buffer may clear dirty state.
                      dirty: matchesSubmittedSnapshot ? false : true,
                      mtimeNanos: action.mtimeNanos,
                      size: action.size,
                      ...(action.contentHash !== undefined ? { contentHash: action.contentHash } : {}),
                      ...(action.lossy !== undefined ? { lossy: action.lossy } : {}),
                      ...(action.durabilityWarning !== undefined
                        ? { durabilityWarning: action.durabilityWarning }
                        : {}),
                    };
                  })(),
            ),
          }
        : state;

    case "toggleSplit": {
      if (state.split) {
        const ids = knownIds(state);
        const merged = uniqueIds([...state.views[0], ...state.views[1]].filter((id) => ids.has(id)));
        for (const doc of state.docs) {
          if (!merged.includes(doc.id)) merged.push(doc.id);
        }
        const currentActive = state.activeDocByView[state.activeView];
        const activeDoc = currentActive && merged.includes(currentActive) ? currentActive : lastId(merged);
        return {
          ...state,
          split: false,
          views: [merged, []],
          activeView: 0,
          activeDocByView: [activeDoc, null],
        };
      }
      return { ...state, split: true };
    }
  }
}

export function docIdForPath(path: string): DocId {
  return `doc:${encodeURIComponent(path)}`;
}

export function stateToSession(state: EditorState): SessionState {
  return {
    version: 1,
    workspace_folder: state.workspaceFolder,
    docs: state.docs.map(({ id, path, cursor, bookmarks }) => ({ id, path, cursor, bookmarks })),
    views: [state.views[0].slice(), state.views[1].slice()],
    active_view: state.activeView,
    active_doc_by_view: [...state.activeDocByView],
    recent_files: state.recentFiles.slice(),
  };
}

export function isEditorStateInvariantValid(state: EditorState): boolean {
  const ids = new Set(state.docs.map((doc) => doc.id));
  if (ids.size !== state.docs.length) return false;
  const placed = [...state.views[0], ...state.views[1]];
  if (placed.length !== ids.size || new Set(placed).size !== placed.length) return false;
  if (placed.some((id) => !ids.has(id))) return false;
  if ([...ids].some((id) => !placed.includes(id))) return false;
  return state.activeDocByView.every((id, view) => id === null || state.views[view].includes(id));
}
