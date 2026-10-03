import { useEffect, type Dispatch, type SetStateAction } from "react";
import type { Doc, DocId } from "../types";

export function usePendingCloseQueue(docs: Doc[], setPendingCloseDocIds: Dispatch<SetStateAction<DocId[]>>) {
  useEffect(() => {
    setPendingCloseDocIds((current) => {
      const next = current.filter((docId) => docs.some((doc) => doc.id === docId));
      return next.length === current.length ? current : next;
    });
  }, [docs, setPendingCloseDocIds]);
}
