import { documentSession, type DocumentStorage, type Stored } from "../../storage/documentStorage";
interface Change {
  kind: "collections" | "environments";
  before: Stored | null;
  emptyBody: string;
  body: string;
}
const stale = () => new Error("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
/** Separate documents use conditional compensation; never overwrite another writer. */
export async function applyImportDocuments(storage: DocumentStorage, changes: Change[]): Promise<() => Promise<void>> {
  const write = async (items: Change[]) => {
    for (const item of items) {
      if ((await storage.load(item.kind))?.revision !== item.before?.revision) throw stale();
    }
    const completed: { item: Change; revision: number }[] = [];
    try {
      for (const item of items) {
        const revision = await documentSession(item.kind, storage).save(item.body, item.before?.revision ?? null);
        completed.push({ item, revision });
      }
    } catch (cause) {
      let compensationFailed = false;
      for (const { item, revision } of completed.reverse()) {
        try {
          await documentSession(item.kind, storage).save(item.before?.body ?? item.emptyBody, revision);
        } catch {
          compensationFailed = true;
        }
      }
      if (compensationFailed) throw new Error("일부 문서의 저장 상태가 달라졌습니다. 현재 내용을 다시 확인하세요.");
      throw cause;
    }
    return completed;
  };
  const completed = await write(changes);
  let used = false;
  return async () => {
    if (used) throw stale();
    used = true;
    await write(
      completed.map(({ item, revision }) => ({
        ...item,
        before: { revision, body: item.body },
        body: item.before?.body ?? item.emptyBody,
      })),
    );
  };
}
