import { useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useRpcQuery } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button, ConfirmDialog } from "../../ui";
import { useSecretMutation } from "./secretMutation";

export function SecretsSettings() {
  const { generation } = useRpcSession();
  return <SecretControls key={generation} />;
}
function SecretControls() {
  const { client, state } = useRpcSession();
  const mutateSecret = useSecretMutation();
  const list = useRpcQuery("secrets.list", {});
  const status = useRpcQuery("secrets.status", {}, { refetchInterval: 5000 });
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [recovery, setRecovery] = useState("");
  const [shownRecovery, setShownRecovery] = useState("");
  const [importing, setImporting] = useState(false);
  const [confirm, setConfirm] = useState<{ name: string } | "reset" | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const pending = useRef(false);
  const disabled = busy || state !== "ready";
  const refresh = () => {
    void list.refetch();
    void status.refetch();
  };
  // Sensitive inputs/results bypass the query and mutation caches entirely.
  const perform = async (operation: () => Promise<unknown>, message: string) => {
    if (pending.current || state !== "ready") return;
    pending.current = true;
    setBusy(true);
    setNotice("");
    try {
      await operation();
      setNotice(message);
      setConfirm(null);
      refresh();
    } catch (error) {
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = messageFor(failure.code, failure.detail);
      setNotice(`${message.title} ${message.action ?? ""}`);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  const save = () => {
    if (disabled || !name.trim()) return;
    const input = { name: name.trim(), value };
    setValue("");
    void perform(() => mutateSecret("secrets.set", input), "값을 저장했습니다");
  };
  const restore = () => {
    if (disabled || !recovery.trim()) return;
    const input = { recovery };
    setRecovery("");
    void perform(async () => {
      await mutateSecret("secrets.import_key", input);
      setImporting(false);
    }, "비밀 키를 복구했습니다");
  };
  const destroy = () => {
    if (!confirm) return;
    setValue("");
    setRecovery("");
    setShownRecovery("");
    const target = confirm;
    void perform(
      () => (target === "reset" ? mutateSecret("secrets.reset", {}) : mutateSecret("secrets.delete", target)),
      target === "reset" ? "이 PC의 비밀 값을 초기화했습니다. 값을 다시 입력하세요." : "이 PC의 값을 삭제했습니다",
    );
  };
  const queryError = list.error ?? status.error;
  return (
    <section aria-label="비밀 설정">
      <h3>비밀</h3>
      {queryError ? (
        <p role="alert">
          비밀 정보를 불러오지 못했습니다 <Button onClick={refresh}>다시 시도</Button>
        </p>
      ) : !list.data || !status.data ? (
        <p role="status">비밀 정보를 불러옵니다</p>
      ) : (
        <>
          <p>키 상태: {status.data.keyMismatch ? "불일치" : status.data.locked ? "잠김" : "열림"}</p>
          {status.data.keyMismatch && (
            <p role="alert">비밀 키가 다릅니다 · 복구 문자열을 입력하거나 비밀 값을 다시 입력하세요.</p>
          )}
          {status.data.locked && <p>Windows 앱에서 키를 풀어 연결하거나 복구 문자열을 입력하세요.</p>}
          <ul aria-label="비밀 이름 목록">
            {list.data.map((secret) => (
              <li key={secret.name}>
                <span>
                  {secret.name} · 이 PC에 값 {secret.hasValue ? "있음" : "없음"}
                </span>{" "}
                <Button
                  disabled={disabled || status.data.locked}
                  onClick={() => {
                    setName(secret.name);
                    setValue("");
                  }}
                  aria-label={`${secret.name} 값 입력`}
                >
                  값 입력
                </Button>{" "}
                <Button
                  disabled={disabled || !secret.hasValue}
                  onClick={() => setConfirm({ name: secret.name })}
                  aria-label={`${secret.name} 삭제`}
                >
                  삭제
                </Button>
              </li>
            ))}
          </ul>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              save();
            }}
          >
            <label>
              비밀 이름{" "}
              <input
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={disabled || status.data.locked}
                autoComplete="off"
              />
            </label>{" "}
            <label>
              비밀 값{" "}
              <input
                type="password"
                value={value}
                onChange={(event) => setValue(event.target.value)}
                disabled={disabled || status.data.locked}
                autoComplete="new-password"
              />
            </label>{" "}
            <Button type="submit" disabled={disabled || status.data.locked || !name.trim()}>
              값 저장
            </Button>
          </form>
          <p>
            <Button
              disabled={disabled || status.data.locked}
              onClick={() =>
                void perform(async () => {
                  const result = await client.call("secrets.export_key", {});
                  setShownRecovery(result.recovery);
                }, "복구 문자열을 안전한 곳에 보관하세요.")
              }
            >
              복구 문자열 보기
            </Button>{" "}
            <Button
              disabled={disabled}
              onClick={() => {
                setImporting(true);
                setShownRecovery("");
              }}
            >
              복구 문자열 입력
            </Button>{" "}
            <Button disabled={disabled} onClick={() => setConfirm("reset")}>
              비밀 다시 입력
            </Button>
          </p>
        </>
      )}
      {shownRecovery && (
        <div role="region" aria-label="복구 문자열">
          <p>
            <code>{shownRecovery}</code>
          </p>
          <Button onClick={() => setShownRecovery("")}>복구 문자열 닫기</Button>
        </div>
      )}
      {importing && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            restore();
          }}
        >
          <label>
            복구 문자열{" "}
            <input
              type="password"
              autoComplete="new-password"
              value={recovery}
              disabled={disabled}
              onChange={(event) => setRecovery(event.target.value)}
            />
          </label>{" "}
          <Button type="submit" disabled={disabled || !recovery.trim()}>
            키 복구
          </Button>{" "}
          <Button
            disabled={busy}
            onClick={() => {
              setRecovery("");
              setImporting(false);
            }}
          >
            복구 입력 취소
          </Button>
        </form>
      )}
      {notice && <p role="status">{notice}</p>}
      <ConfirmDialog
        open={confirm !== null}
        title={confirm === "reset" ? "비밀 다시 입력" : "비밀 값 삭제"}
        body={
          confirm === "reset"
            ? "이 PC에 저장된 모든 비밀 값을 삭제하고 새 키를 만듭니다. 이름 목록은 유지되며 값은 다시 입력해야 합니다. 되돌릴 수 없습니다."
            : `${confirm?.name ?? ""}의 이 PC 값을 삭제합니다. 이름은 유지됩니다. 되돌릴 수 없습니다.`
        }
        confirmLabel={confirm === "reset" ? "모든 값 초기화" : "삭제"}
        disabled={disabled}
        onConfirm={destroy}
        onCancel={() => {
          if (!pending.current) setConfirm(null);
        }}
      />
    </section>
  );
}
