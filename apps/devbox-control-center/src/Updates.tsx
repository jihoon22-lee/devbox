import type { UpdateCall } from "@devbox/control-center-features/generated/UpdateCall";
import { deliveryCall } from "./delivery";
import { useEffect, useRef, useState } from "react";
import type { ShellContentProps } from "@devbox/product-shell";
import { makeRequest, nativeMode } from "@devbox/product-shell/api";
type Review = import("@devbox/control-center-features/generated/UpdateReview").UpdateReview;
export default function Updates({ description, route }: Pick<ShellContentProps, "description" | "route">) {
  const [review, setReview] = useState<Review | null>(null),
    [busy, setBusy] = useState(false),
    [issue, setIssue] = useState(""),
    [confirmed, setConfirmed] = useState(false),
    [launching, setLaunching] = useState(false);
  const sequence = useRef(0);
  useEffect(
    () => () => {
      sequence.current++;
    },
    [],
  );
  const request = async <M extends UpdateCall["method"]>(
    method: M,
    args: Extract<UpdateCall, { method: M }> extends { args: infer A } ? A : never,
  ) => {
    const header = makeRequest(description.handshake, route, Date.now(), description.context);
    header.deadlineMs += 24000;
    const result = await deliveryCall(header, method, args);
    return result.value;
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    if (review?.state !== "downloading" || !review.id) return;
    let active = true;
    const id = review.id;
    const timer = setTimeout(() => {
      void request("suite_update_status", { id })
        .then((value) => {
          if (active) setReview(value);
        })
        .catch(() => {
          if (active) {
            setIssue("다운로드 상태를 읽지 못했습니다. 상태를 다시 확인할 수 있습니다.");
            setReview((previous) => (previous?.id === id ? { ...previous, state: "statusUnknown" } : previous));
          }
        });
    }, 700);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [review, description, route]);
  const run = async (method: UpdateCall["method"]) => {
    if (busy) return;
    const current = ++sequence.current;
    setBusy(true);
    setIssue("");
    try {
      if (method === "launch_suite_update") {
        if (!confirmed || !review?.id) return;
        await request("launch_suite_update", { id: review.id });
        if (current === sequence.current) setLaunching(true);
      } else {
        const value =
          method === "check_suite_update" ? await request(method, {}) : await request(method, { id: review?.id ?? "" });
        if (current === sequence.current) {
          setReview(value);
          setConfirmed(false);
        }
      }
    } catch (error) {
      if (current === sequence.current)
        setIssue(
          (error instanceof Error ? error.message : undefined) ??
            "업데이트 요청을 완료하지 못했습니다. 현재 설치는 유지됩니다.",
        );
    } finally {
      if (current === sequence.current) setBusy(false);
    }
  };
  return (
    <section aria-label="Suite 업데이트">
      <h2>Suite 업데이트</h2>
      <p>공식 정식 릴리스의 네 제품을 함께 업데이트합니다. 다운로드만으로 설치를 시작하지 않습니다.</p>
      <p>
        이전 버전으로 되돌리기 전에는 데이터 보존·복원 화면에서 현재 데이터를 보존하고 필요한 자료를 내보내세요. 새로
        작성한 데이터를 구 버전 형식으로 자동 역변환하거나 덮어쓰지 않습니다.
      </p>
      <button
        disabled={!nativeMode || busy || review?.state === "downloading" || launching}
        onClick={() => void run("check_suite_update")}
      >
        업데이트 확인
      </button>
      {issue && <p role="alert">{issue}</p>}
      {review && !review.available && <p>설치된 버전보다 새로운 정식 릴리스가 없습니다. 공개 버전: {review.version}</p>}
      {review?.available && (
        <>
          <p>
            새 버전 {review.version} · {((review.bytes ?? 0) / 1024 / 1024).toFixed(1)} MiB
          </p>
          {review.state === "downloading" ? (
            <>
              <progress aria-label="설치 파일 다운로드" max={review.bytes} value={review.received} />
              <button disabled={busy} onClick={() => void run("cancel_suite_update")}>
                다운로드 취소
              </button>
            </>
          ) : (
            review.state !== "ready" && (
              <button
                disabled={busy || launching}
                onClick={() =>
                  void run(review.state === "statusUnknown" ? "suite_update_status" : "download_suite_update")
                }
              >
                {review.state === "statusUnknown" ? "다운로드 상태 다시 확인" : "검토한 설치 파일 다운로드"}
              </button>
            )
          )}
          {review.issue && (
            <p role="alert">
              {messages[review.issue] ?? "다운로드를 완료하지 못했습니다. 현재 설치는 변경되지 않았습니다."}
            </p>
          )}
          {review.state === "ready" && (
            <div>
              <p>
                공식 크기와 SHA-256을 확인했습니다. 네 제품의 작업을 저장하고 닫은 뒤 설치를 진행하세요. 기존 패키지와
                데이터는 복구용으로 보존됩니다.
              </p>
              <label>
                <input
                  type="checkbox"
                  disabled={busy || launching}
                  checked={confirmed}
                  onChange={(event) => setConfirmed(event.target.checked)}
                />
                버전과 제품 종료를 확인했습니다.
              </label>{" "}
              <button disabled={!confirmed || busy || launching} onClick={() => void run("launch_suite_update")}>
                Control Center를 닫고 설치 프로그램 열기
              </button>
            </div>
          )}
        </>
      )}
      {launching && <p role="status">설치 프로그램을 열었습니다. Control Center가 닫힙니다.</p>}
    </section>
  );
}
