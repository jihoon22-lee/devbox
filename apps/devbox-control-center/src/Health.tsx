import { useEffect, useRef, useState } from "react";
import type { ShellContentProps } from "@devbox/product-shell";
import { makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation } from "@devbox/product-shell/operation";
import { invoke } from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
interface Observation {
  nativeStoreReady: boolean;
  report: {
    store: { owner: string; suiteVersion: string; busy: boolean; reviewRequired: boolean; setupSelected: boolean };
  };
}
interface Row {
  owner: string;
  value: Observation | null;
}
export default function Health({ description, route }: ShellContentProps) {
  const [rows, setRows] = useState<Row[]>([]),
    [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );
  const refresh = async () => {
    if (busy || !nativeMode) return;
    const current = ++generation.current;
    setBusy(true);
    setRows([]);
    const next = await Promise.all(
      catalog.products.map(async (product) => {
        try {
          const header = makeRequest(description.handshake, route, Date.now(), description.context);
          header.deadlineMs += 24000;
          const result = await invoke<{ operation: unknown; value: Observation }>("plugin:suite|connection", {
            request: { header, method: { kind: "readHealthStatus", product: product.id } },
          });
          if (
            !isOperation(result.operation, {
              product: "control-center",
              component: "control-center.commands",
              requestId: header.requestId,
              revision: catalog.catalogRevision,
            }) ||
            result.operation.outcome.state !== "succeeded" ||
            result.value.report.store.owner !== product.id ||
            typeof result.value.nativeStoreReady !== "boolean"
          )
            throw new Error("unavailable");
          return { owner: product.id, value: result.value };
        } catch {
          return { owner: product.id, value: null };
        }
      }),
    );
    if (current === generation.current) {
      setRows(next);
      setBusy(false);
    }
  };
  const record = async () => {
    if (busy || !nativeMode) return;
    const current = ++generation.current;
    setBusy(true);
    setRows([]);
    const next: Row[] = [];
    for (const product of catalog.products) {
      try {
        const header = makeRequest(description.handshake, route, Date.now(), description.context);
        header.deadlineMs += 24000;
        const result = await invoke<{ operation: unknown; value: Observation & { recorded: boolean } }>(
          "plugin:control-center|execute",
          { request: { header, method: "record_suite_health", args: { product: product.id } } },
        );
        if (
          !isOperation(result.operation, {
            product: "control-center",
            component: "control-center.delivery",
            requestId: header.requestId,
            revision: catalog.catalogRevision,
          }) ||
          result.operation.outcome.state !== "succeeded" ||
          result.value.recorded !== true ||
          result.value.report.store.owner !== product.id
        )
          throw new Error("unavailable");
        next.push({ owner: product.id, value: result.value });
      } catch {
        next.push({ owner: product.id, value: null });
      }
      if (current !== generation.current) return;
      setRows([...next]);
    }
    if (current === generation.current) setBusy(false);
  };
  return (
    <section aria-label="제품 상태 확인">
      <h2>제품 응답·저장소 확인</h2>
      <p>
        현재 설치의 네 제품을 열고 같은 설치의 제품 연결을 확인한 뒤 확인할 수 있습니다. 사용자 작업이나 예약 실행은
        시작하지 않습니다.
      </p>
      <button disabled={busy || !nativeMode} onClick={() => void refresh()}>
        {busy ? "제품 확인 중…" : "네 제품 상태 확인"}
      </button>
      {["import", "health"].includes(description.deliveryState ?? "") && (
        <button disabled={busy || !nativeMode} onClick={() => void record()}>
          상태 기록
        </button>
      )}
      {!!rows.length && (
        <table>
          <thead>
            <tr>
              <th>제품</th>
              <th>버전</th>
              <th>상태</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.owner}>
                <td>{catalog.products.find((product) => product.id === row.owner)?.label}</td>
                <td>{row.value?.report.store.suiteVersion ?? "확인 필요"}</td>
                <td>
                  {!row.value
                    ? "응답 또는 저장소를 확인하지 못함"
                    : row.value.nativeStoreReady
                      ? "응답·저장소 선택 확인됨"
                      : row.value.report.store.busy
                        ? "저장소 작업 진행 중"
                        : row.value.report.store.reviewRequired
                          ? "저장소 준비 확인 필요"
                          : "저장소 선택 필요"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p>이 확인만으로 설치가 완료되지는 않습니다. 최종 활성화와 복구 준비 상태를 별도로 확인합니다.</p>
    </section>
  );
}
