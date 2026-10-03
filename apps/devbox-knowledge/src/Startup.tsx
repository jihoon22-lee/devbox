import { recoveryLazy } from "./recoveryLazy";
import { Suspense, useEffect, useState, type ReactNode } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import type { NoteJournalView } from "@devbox/knowledge-features/notes/api";
import { setupCall } from "@devbox/knowledge-features/setup/api";

const VaultSettings = recoveryLazy(() => import("./VaultSettings"));
const VaultSetup = recoveryLazy(() => import("./VaultSetup"));
export function Startup({ children }: { children: ReactNode }) {
  const [recovery, setRecovery] = useState<NoteJournalView | null>(null);
  const [recoveryError, setRecoveryError] = useState("");
  const [active, setActive] = useState(!nativeMode);
  const [loading, setLoading] = useState(nativeMode);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasExisting, setHasExisting] = useState(false);
  const [autoStarted, setAutoStarted] = useState(false);
  const [showVault, setShowVault] = useState(false);
  const [canReconnect, setCanReconnect] = useState(false);
  const [reconnect, setReconnect] = useState(false);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true;
    void setupCall("status", {})
      .then((value) => {
        if (alive) {
          setActive(value.active === true || value.prepared === true);
          setCanReconnect(value.bindingUnavailable === true);
          if (value.bindingUnavailable)
            setError("현재 노트 폴더에 연결할 수 없습니다. 폴더를 복구하거나 다른 폴더를 선택해 주세요.");
          setShowVault(value.vaultChange === true && !value.active);
          setHasExisting(value.hasExisting === true);
        }
      })
      .catch((error) => {
        if (alive) {
          setError(error instanceof Error ? error.message : "저장소를 확인하지 못했습니다.");
          setBlocked(
            error instanceof Error &&
              [
                "future_schema",
                "store_invalid",
                "restart_required",
                "import_schema_unsupported",
                "vault_change_invalid",
              ].includes(error.name),
          );
        }
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);
  const start = async () => {
    setLoading(true);
    setError(null);
    try {
      const value = await setupCall(hasExisting ? "continue_existing" : "start_empty", {});
      setActive(value.active === true || value.prepared === true);
    } catch (error) {
      setError(error instanceof Error ? error.message : "저장소를 준비하지 못했습니다.");
      setBlocked(
        error instanceof Error &&
          [
            "future_schema",
            "store_invalid",
            "restart_required",
            "import_schema_unsupported",
            "vault_change_invalid",
          ].includes(error.name),
      );
    } finally {
      setLoading(false);
    }
  };
  const loadRecovery = async () => {
    try {
      setRecovery(await setupCall("load_recovery", {}));
      setRecoveryError("");
    } catch {
      setRecoveryError(
        "신뢰할 수 있는 복구 범위를 확인하지 못했습니다. 원본 폴더 연결을 복구한 뒤 다시 시도해 주세요.",
      );
    }
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    if (!nativeMode || loading || active || autoStarted || error || showVault || reconnect || blocked) return;
    setAutoStarted(true);
    void start();
  }, [loading, active, autoStarted, error, showVault, reconnect, blocked]);
  if (active) return children;
  if (reconnect)
    return (
      <Suspense fallback={<p role="status">노트 폴더 설정을 불러오고 있습니다…</p>}>
        <VaultSettings
          onScheduled={() => {
            setReconnect(false);
            setShowVault(true);
          }}
        />
      </Suspense>
    );
  if (showVault)
    return (
      <Suspense fallback={<p role="status">노트 폴더 설정을 불러오고 있습니다…</p>}>
        <VaultSetup onActivated={() => setActive(true)} />
      </Suspense>
    );
  return (
    <section className="knowledge-startup" aria-labelledby="knowledge-start-title">
      <h2 id="knowledge-start-title">저장소를 준비하고 있습니다…</h2>
      <p>노트와 활동, 파일 검색을 위한 저장소를 준비합니다. 활동 수집은 직접 켜기 전까지 시작하지 않습니다.</p>
      {loading && <p role="status">저장소를 확인하고 있습니다…</p>}
      {error && <p role="alert">{error}</p>}
      {error && (
        <button type="button" disabled={loading || blocked} onClick={() => void start()}>
          다시 시도
        </button>
      )}
      {canReconnect && (
        <section aria-label="연결이 끊긴 노트 복구">
          <p>
            노트 폴더에 연결하기 전에도 이 설치에서 검증한 복구본을 읽고 복사할 수 있습니다. 저장과 재생성은 연결 복구
            후에 가능합니다.
          </p>
          <button type="button" onClick={() => void loadRecovery()}>
            로컬 복구본 확인
          </button>
          {recoveryError && <p role="alert">{recoveryError}</p>}
          {recovery?.entries.map((entry) => (
            <article key={entry.path}>
              <h3>{entry.path}</h3>
              <pre>{entry.content}</pre>
              <button
                onClick={() => {
                  void navigator.clipboard
                    .writeText(entry.content)
                    .catch(() => setRecoveryError("복구본을 복사하지 못했습니다."));
                }}
              >
                복구본 복사
              </button>
            </article>
          ))}
          {!!recovery?.otherVaultCount && (
            <p>다른 노트 폴더의 복구본 {recovery.otherVaultCount}개는 해당 폴더를 다시 연결해서 확인합니다.</p>
          )}
        </section>
      )}
      {canReconnect && (
        <button type="button" disabled={loading || blocked} onClick={() => setReconnect(true)}>
          다른 노트 폴더 선택
        </button>
      )}
    </section>
  );
}
