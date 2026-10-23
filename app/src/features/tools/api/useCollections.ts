/** 컬렉션 목록 읽기. 저장·가져오기 뒤와 `api.collections_changed`에서 `bumpCollections()`로 다시 읽는다. */
import { useEffect, useState } from "react";
import { create } from "zustand";
import type { CollectionFile, CollectionRoot } from "../../../rpc/gen/rpc";
import { useApiBackend } from "./backend";
import { REQUEST_FILE_SUFFIX } from "./model/requestFile";
import { rootKey } from "./store";

const useRevision = create<{ revision: number }>(() => ({ revision: 0 }));
export const bumpCollections = () => useRevision.setState((state) => ({ revision: state.revision + 1 }));

export interface CollectionState {
  items: CollectionFile[];
  truncated: boolean;
  error?: { code: string; detail?: unknown };
  loaded: boolean;
}

export function useCollections(root: CollectionRoot): CollectionState {
  const backend = useApiBackend();
  const revision = useRevision((state) => state.revision);
  const [state, setState] = useState<CollectionState>({ items: [], truncated: false, loaded: false });
  const key = rootKey(root);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the root key identifies the root object.
  useEffect(() => {
    let live = true;
    backend.list(root).then(
      (list) => live && setState({ items: list.items, truncated: list.truncated, loaded: true }),
      (error: { code?: string; detail?: unknown }) =>
        live &&
        setState((s) => ({ ...s, loaded: true, error: { code: error.code ?? "internal", detail: error.detail } })),
    );
    return () => {
      live = false;
    };
  }, [backend, key, revision]);
  return state;
}

export const isRequestPath = (path: string) => path.endsWith(REQUEST_FILE_SUFFIX);
export const requestStem = (path: string) => path.slice(0, -REQUEST_FILE_SUFFIX.length);
