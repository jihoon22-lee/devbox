import type { HistoryItem } from "../types";
import { confirmAction } from "@devbox/product-shell/confirm";
import { emptyHistoryStore, type HistoryStore } from "./persistence";
import { removeHistoryItem } from "./contextMenu";

export async function reviewedHistoryRemoval(
  item: HistoryItem,
  current: () => HistoryItem[] | null,
): Promise<HistoryStore | null> {
  const label = (item.name ?? item.request.url) || "(no url)";
  if (!(await confirmAction(`'${label}' 기록을 삭제할까요? 이 작업은 되돌릴 수 없습니다.`))) return null;
  const history = current();
  return history ? removeHistoryItem({ ...emptyHistoryStore(), history }, item.id) : null;
}
