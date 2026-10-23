/** 실행기 화면: 루트의 요청 파일을 모두 읽어 실행 항목으로 만든다(검토 표시는 `reviewNeeded`로 합친다). */
import { useEffect, useState } from "react";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import s from "./Api.module.css";
import { useApiBackend } from "./backend";
import { runEntryFromFile } from "./model/fromTemplate";
import { parseRequestFile } from "./model/requestFile";
import { RunnerPanel } from "./RunnerPanel";
import type { RunEntry } from "./runner/runner";
import { rootKey, useApiTabs } from "./store";
import { isRequestPath, useCollections } from "./useCollections";

export function RunnerView({ root }: { root: CollectionRoot }) {
  const backend = useApiBackend();
  const list = useCollections(root);
  const environment = useApiTabs((state) => state.environment);
  const session = useApiTabs((state) => state.session);
  const [entries, setEntries] = useState<RunEntry[]>();
  const [skipped, setSkipped] = useState<string[]>([]);
  const key = rootKey(root);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the root key identifies the root object.
  useEffect(() => {
    let live = true;
    const files = list.items.filter((item) => isRequestPath(item.path) && !item.unreadable);
    void (async () => {
      const out: RunEntry[] = [];
      const bad: string[] = [];
      for (const item of files.sort((a, b) => a.path.localeCompare(b.path))) {
        try {
          const read = await backend.read(root, item.path);
          out.push(runEntryFromFile(item.path, { reviewed: read.reviewed }, parseRequestFile(read.text)));
        } catch {
          bad.push(item.path);
        }
      }
      if (live) {
        setEntries(out);
        setSkipped(bad);
      }
    })();
    return () => {
      live = false;
    };
  }, [backend, key, list.items]);
  if (!entries) return <p className={s.muted}>요청 파일을 읽는 중입니다…</p>;
  return (
    <>
      {skipped.length > 0 && <p className={s.muted}>{`읽지 못한 파일은 뺐습니다: ${skipped.join(", ")}`}</p>}
      <RunnerPanel entries={entries} environment={environment?.variables ?? []} session={session} root={root} />
    </>
  );
}
