import type { DocKey } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import s from "./notes.module.css";
import { saveLabel } from "./saveLabel";

export function SaveStatus({ docKey }: { docKey: DocKey }) {
  const { state } = useRpcSession();
  const doc = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(docKey)));
  const label = doc ? saveLabel(doc, state === "ready") : null;
  return (
    <span className={s.saveStatus} role="status" aria-label="저장 상태" data-state={doc?.state}>
      {label}
    </span>
  );
}
