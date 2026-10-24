/** 이전 기록 본문과 지금 응답 본문의 줄 비교(`lineDiff.ts` → `ui/DiffView`). */
import { useEffect, useState } from "react";
import { DiffView } from "../../../ui";
import type { DiffFile } from "../../../ui/diff";
import { useApiBackend } from "./backend";
import { lineDiff } from "./lib/lineDiff";

/** 둘 다 JSON이면 같은 모양으로 펼쳐 비교한다(공백 차이 제외). */
function pretty(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

export function ResponseCompare({ historyId, current }: { historyId: string; current: string }) {
  const backend = useApiBackend();
  const [diff, setDiff] = useState<DiffFile>();
  const [error, setError] = useState(false);
  useEffect(() => {
    let live = true;
    setDiff(undefined);
    setError(false);
    backend.historyBody(historyId).then(
      (before) =>
        live && setDiff(lineDiff(pretty(before), pretty(current), { oldLabel: "이전 응답", newLabel: "지금 응답" })),
      () => live && setError(true),
    );
    return () => {
      live = false;
    };
  }, [backend, historyId, current]);
  if (error) return <p role="alert">이전 응답 본문을 읽지 못했습니다.</p>;
  if (!diff) return null;
  if (!diff.hunks.length) return <p>이전 응답과 같습니다.</p>;
  return (
    <section aria-label="응답 비교">
      <DiffView file={diff} />
    </section>
  );
}
