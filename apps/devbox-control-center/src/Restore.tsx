import { useEffect, useState } from "react";
import type { ShellContentProps } from "@devbox/product-shell";
import { makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation } from "@devbox/product-shell/operation";
import { invoke } from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";

interface Checkpoint {
  id: string;
  bytes: number;
  files: number;
}
interface RestoreOperation {
  id: string;
  phase: string;
  checkpointId?: string;
  preparedMs?: number;
}
interface UpdateOperation {
  id: string;
  state: string;
  previousVersion: string;
  version: string;
  checkpointId: string;
}
interface Inventory {
  update?: UpdateOperation | null;
  checkpoints: Checkpoint[];
  operations: RestoreOperation[];
  activeOperation: string | null;
  installation?: {
    phase: string;
    committed: boolean;
    recordedOwners: number;
    clean: boolean;
    reinstall?: boolean;
    freshHealth: boolean;
  };
}
type Action =
  | "commitReinstall"
  | "updateResume"
  | "updateCommit"
  | "updateRollback"
  | "activateClean"
  | "commitClean"
  | "snapshot"
  | "restore"
  | "resume"
  | "commit"
  | "rollback";
const labels: Record<string, string> = {
  prepared: "복원 준비됨",
  applying: "복원 적용 중단",
  health: "제품 상태 확인 대기",
  committing: "확정 재개 필요",
  committed: "복원 확정됨",
  rollingBack: "원본 복귀 재개 필요",
  rolledBack: "원본 복귀 완료",
  preparationInterrupted: "준비 중단 · 보존된 파일 유지",
};
const actionLabels: Record<Action, string> = {
  commitReinstall: "보존된 데이터로 재설치 확정",
  updateResume: "업데이트 재개",
  updateCommit: "업데이트 확정",
  updateRollback: "이전 버전과 데이터로 복귀",
  activateClean: "신규 설치 활성화 준비",
  commitClean: "신규 설치 확정",
  snapshot: "현재 데이터 보존",
  restore: "선택한 보존본으로 복원",
  resume: "복원 재개",
  commit: "복원 확정",
  rollback: "복원 전 원본으로 복귀",
};

export default function Restore({ description, route }: Pick<ShellContentProps, "description" | "route">) {
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [revision, setRevision] = useState(0),
    [error, setError] = useState("");
  const [busy, setBusy] = useState(false),
    [accepted, setAccepted] = useState(false);
  const [review, setReview] = useState<{ action: Action; id: string } | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    let active = true;
    setInventory(null);
    setError("");
    setReview(null);
    setConfirmed(false);
    if (!nativeMode) return;
    const header = makeRequest(description.handshake, route, Date.now(), description.context);
    void invoke<{ operation: unknown; value: Inventory }>("plugin:control-center|execute", {
      request: { header, method: "restore_inventory", args: {} },
    })
      .then((response) => {
        if (
          !isOperation(response.operation, {
            product: "control-center",
            component: "control-center.delivery",
            requestId: header.requestId,
            revision: catalog.catalogRevision,
          }) ||
          response.operation.outcome.state !== "succeeded"
        )
          throw new Error("unavailable");
        if (active) setInventory(response.value);
      })
      .catch(() => {
        if (active) setError("이 설치의 복원 목록을 읽지 못했습니다. 설치와 보존본은 변경되지 않았습니다.");
      });
    return () => {
      active = false;
    };
  }, [description, route, revision]);
  const select = (action: Action, id = "") => {
    setReview({ action, id });
    setConfirmed(false);
    setError("");
  };
  const execute = async () => {
    if (!review || !confirmed || busy) return;
    setBusy(true);
    setError("");
    const header = makeRequest(description.handshake, route, Date.now(), description.context);
    header.deadlineMs += 24000;
    try {
      const response = await invoke<{ operation: unknown; value: { accepted: boolean } }>(
        "plugin:control-center|execute",
        { request: { header, method: "restore_action", args: review } },
      );
      if (
        !isOperation(response.operation, {
          product: "control-center",
          component: "control-center.delivery",
          requestId: header.requestId,
          revision: catalog.catalogRevision,
        }) ||
        response.operation.outcome.state !== "succeeded" ||
        !response.value.accepted
      )
        throw new Error("unavailable");
      setAccepted(true);
    } catch {
      setError("복구 도우미를 시작하지 못했습니다. 작업은 완료되지 않았습니다.");
      setBusy(false);
    }
  };
  return (
    <section aria-label="데이터 보존과 복원">
      <h2>데이터 보존·복원</h2>
      <p>
        현재 설치의 네 제품 데이터를 함께 보존하거나 이전 보존본으로 되돌립니다. 외부 문서 저장소와 Git 작업 폴더는
        변경하지 않습니다.
      </p>
      {!nativeMode ? (
        <p>데스크톱 설치에서 사용할 수 있습니다.</p>
      ) : (
        <>
          {error && <p role="alert">{error}</p>}
          {accepted && (
            <p role="status">복구 도우미를 시작했습니다. Control Center가 닫힙니다. 나머지 제품도 닫아 주세요.</p>
          )}
          <button disabled={busy} onClick={() => setRevision((value) => value + 1)}>
            목록 새로고침
          </button>
          {inventory && (
            <>
              {inventory.update && (
                <div>
                  <h3>Suite 업데이트</h3>
                  <p>
                    {inventory.update.previousVersion} → {inventory.update.version} ·{" "}
                    {labels[inventory.update.state] ?? "상태 확인 필요"}
                  </p>
                  <p>이전 패키지와 데이터는 함께 보존됩니다. 네 제품의 상태를 기록한 뒤 업데이트를 확정하세요.</p>
                  {["applying"].includes(inventory.update.state) && (
                    <button disabled={busy} onClick={() => select("updateResume", inventory.update!.id)}>
                      업데이트 재개
                    </button>
                  )}
                  {["health", "committing", "committed"].includes(inventory.update.state) && (
                    <button disabled={busy} onClick={() => select("updateCommit", inventory.update!.id)}>
                      업데이트 확정
                    </button>
                  )}
                  {["applying", "health", "rollingBack", "rolledBack"].includes(inventory.update.state) && (
                    <button disabled={busy} onClick={() => select("updateRollback", inventory.update!.id)}>
                      이전 버전과 데이터로 복귀
                    </button>
                  )}
                </div>
              )}
              {!inventory.installation?.committed && inventory.installation && (
                <div>
                  <h3>설치 활성화</h3>
                  <p>제품별 저장소 준비 기록: {inventory.installation.recordedOwners}/4</p>
                  {inventory.installation.reinstall ? (
                    <>
                      <p>기존 데이터를 보존한 재설치입니다. 네 제품의 상태를 기록한 뒤 확정하세요.</p>
                      <button
                        disabled={
                          busy ||
                          !inventory.installation.freshHealth ||
                          !!inventory.activeOperation ||
                          !!inventory.update
                        }
                        onClick={() => select("commitReinstall")}
                      >
                        보존된 데이터로 재설치 확정
                      </button>
                    </>
                  ) : inventory.installation.clean ? (
                    <>
                      {["snapshot", "import", "validate", "quiesce", "activate"].includes(
                        inventory.installation.phase,
                      ) && (
                        <button
                          disabled={busy || !!inventory.activeOperation || !!inventory.update}
                          onClick={() => select("activateClean")}
                        >
                          신규 설치 활성화 준비
                        </button>
                      )}
                      {["health", "commit"].includes(inventory.installation.phase) && (
                        <>
                          <p>아래에서 네 제품의 활성화용 상태를 기록하고 목록을 새로고침한 뒤 확정할 수 있습니다.</p>
                          <button
                            disabled={
                              busy ||
                              !!inventory.activeOperation ||
                              !!inventory.update ||
                              !inventory.installation.freshHealth
                            }
                            onClick={() => select("commitClean")}
                          >
                            신규 설치 확정
                          </button>
                        </>
                      )}
                    </>
                  ) : (
                    <p>네 제품의 저장소 준비 상태를 기록한 뒤 목록을 새로고침해 주세요.</p>
                  )}
                </div>
              )}
              <button
                disabled={busy || !!inventory.activeOperation || !!inventory.update}
                onClick={() => select("snapshot")}
              >
                현재 데이터 보존
              </button>
              <h3>제품 데이터 보존본</h3>
              {inventory.checkpoints.length ? (
                <ul>
                  {inventory.checkpoints.map((checkpoint) => (
                    <li key={checkpoint.id}>
                      <code>{checkpoint.id}</code> · {checkpoint.files.toLocaleString()}개 파일 ·{" "}
                      {(checkpoint.bytes / 1024 / 1024).toFixed(1)} MiB{" "}
                      <button
                        disabled={busy || !!inventory.activeOperation || !!inventory.update}
                        onClick={() => select("restore", checkpoint.id)}
                      >
                        이 보존본으로 복원
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p>기록된 제품 데이터 보존본이 없습니다.</p>
              )}
              <h3>복원 작업</h3>
              {inventory.operations.length ? (
                <ul>
                  {inventory.operations.map((operation) => (
                    <li key={operation.id}>
                      <code>{operation.id}</code> · {labels[operation.phase] ?? "상태 확인 필요"}{" "}
                      {operation.preparedMs && (
                        <time dateTime={new Date(operation.preparedMs).toISOString()}>
                          {new Date(operation.preparedMs).toLocaleString()}
                        </time>
                      )}
                      {(!inventory.activeOperation || inventory.activeOperation === operation.id) && (
                        <>
                          {["prepared", "applying"].includes(operation.phase) && (
                            <button disabled={busy} onClick={() => select("resume", operation.id)}>
                              복원 재개
                            </button>
                          )}
                          {(["health", "committing"].includes(operation.phase) ||
                            (operation.phase === "committed" && inventory.activeOperation === operation.id)) && (
                            <button disabled={busy} onClick={() => select("commit", operation.id)}>
                              복원 확정
                            </button>
                          )}
                          {(["prepared", "applying", "health", "rollingBack"].includes(operation.phase) ||
                            (operation.phase === "rolledBack" && inventory.activeOperation === operation.id)) && (
                            <button disabled={busy} onClick={() => select("rollback", operation.id)}>
                              원본으로 복귀
                            </button>
                          )}
                        </>
                      )}
                    </li>
                  ))}
                </ul>
              ) : (
                <p>진행한 복원 작업이 없습니다.</p>
              )}
            </>
          )}
          {review && (
            <div role="region" aria-label="복구 작업 검토">
              <h3>{actionLabels[review.action]}</h3>
              {review.id && (
                <p>
                  선택한 항목: <code>{review.id}</code>
                </p>
              )}
              <p>
                먼저 다른 제품의 작업을 저장하고 닫아 주세요. 실행하면 Control Center도 닫히며, 모든 제품이 닫힌 뒤
                작업합니다. 다른 프로세스를 강제로 종료하지 않습니다.
              </p>
              {review.action === "activateClean" && (
                <p>
                  준비 상태가 확인된 신규 설치를 활성화합니다. 데이터 보존 후 네 제품의 상태 확인 단계로 이동하며, 아직
                  일반 작업은 활성화하지 않습니다.
                </p>
              )}
              {review.action === "restore" && (
                <p>
                  현재 데이터 전체를 먼저 보존한 뒤 선택한 보존본을 적용합니다. 복원 후 네 제품의 상태를 확인하고
                  확정하기 전까지 일반 작업은 차단됩니다.
                </p>
              )}
              {["commit", "commitClean", "updateCommit"].includes(review.action) && (
                <p>
                  네 제품을 열어 활성화용 상태 확인·기록을 마친 뒤 닫아 주세요. 확정 후에는 복원 전 원본으로 즉시 복귀할
                  수 없으며, 별도 보존본 복원을 검토해야 합니다.
                </p>
              )}
              {["rollback", "updateRollback"].includes(review.action) && (
                <p>
                  복원 전 데이터를 다시 사용합니다. 복원한 데이터와 상태 확인 중 변경된 데이터도 삭제하지 않고 별도로
                  보존합니다.
                </p>
              )}
              <label>
                <input
                  type="checkbox"
                  checked={confirmed}
                  disabled={busy}
                  onChange={(event) => setConfirmed(event.target.checked)}
                />
                선택한 작업과 제품 종료를 확인했습니다.
              </label>{" "}
              <button disabled={!confirmed || busy} onClick={() => void execute()}>
                {busy ? "도우미 시작 중…" : "Control Center를 닫고 실행"}
              </button>{" "}
              <button disabled={busy} onClick={() => setReview(null)}>
                취소
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
