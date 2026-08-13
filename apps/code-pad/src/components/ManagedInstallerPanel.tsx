import { useEffect, useMemo, useState } from "react";
import {
  installLsp,
  lspCatalog,
  lspInstalled,
  recoverInstalledLsp,
  uninstallLsp,
} from "../api";
import type {
  ManagedInstallState,
  ManagedInstallStatus,
  ManagedServerManifest,
} from "../types";

type PendingAction = {
  kind: "install" | "uninstall";
  manifest: ManagedServerManifest | null;
  status: ManagedInstallStatus;
};

type RuntimeMetadata = {
  kind: "native" | "node";
  executable: string;
  min_version: string | null;
};

function statusLabel(state: ManagedInstallState): string {
  switch (state) {
    case "installed": return "설치됨";
    case "needs_reinstall": return "재설치 필요";
    default: return "미설치";
  }
}

function keyFor(manifestId: string, version: string, platform: string): string {
  return `${manifestId}\u001f${version}\u001f${platform}`;
}

function formatSize(size: number | null): string {
  return size === null ? "카탈로그에서 확인되지 않음" : `${size.toLocaleString("en-US")} bytes`;
}

function displayRuntime(runtime: RuntimeMetadata): string {
  const minimum = runtime.min_version ? ` ${runtime.min_version}` : "";
  return `${runtime.kind} · ${runtime.executable}${minimum}`;
}

interface DisplayMetadata {
  sourceUrl: string;
  license: string;
  artifactUrl: string;
  sha256: string;
  size: number | null;
  runtime: string;
}

function metadataFor(
  manifest: ManagedServerManifest | null,
  status: ManagedInstallStatus,
  kind: PendingAction["kind"],
): DisplayMetadata {
  const indexed = status.installed;
  const catalogIsAuthority = kind === "install";
  return {
    sourceUrl: (catalogIsAuthority ? manifest?.source_url : indexed?.source_url)
      ?? indexed?.source_url
      ?? manifest?.source_url
      ?? "카탈로그 없음",
    license: (catalogIsAuthority ? manifest?.license : indexed?.license)
      ?? indexed?.license
      ?? manifest?.license
      ?? "카탈로그 없음",
    artifactUrl: (catalogIsAuthority ? manifest?.artifact.url : indexed?.artifact_url)
      ?? indexed?.artifact_url
      ?? manifest?.artifact.url
      ?? "카탈로그 없음",
    sha256: (catalogIsAuthority ? manifest?.artifact.sha256 : indexed?.sha256)
      ?? indexed?.sha256
      ?? manifest?.artifact.sha256
      ?? "카탈로그 없음",
    size: manifest?.artifact.size_bytes ?? null,
    runtime: indexed && !catalogIsAuthority
      ? displayRuntime(indexed.runtime)
      : manifest
        ? displayRuntime(manifest.runtime)
        : indexed
          ? displayRuntime(indexed.runtime)
          : "카탈로그 없음",
  };
}

interface CardProps {
  manifest: ManagedServerManifest | null;
  status: ManagedInstallStatus;
  hasOtherVersion: boolean;
  loading: boolean;
  busyKey: string | null;
  recoveryBusy: boolean;
  onInstall: (manifest: ManagedServerManifest, status: ManagedInstallStatus) => void;
  onUninstall: (manifest: ManagedServerManifest | null, status: ManagedInstallStatus) => void;
}

function ManagedInstallCard({
  manifest,
  status,
  hasOtherVersion,
  loading,
  busyKey,
  recoveryBusy,
  onInstall,
  onUninstall,
}: CardProps) {
  const actionKey = keyFor(status.manifest_id, status.version, status.platform);
  const busy = busyKey === actionKey;
  const blocked = loading || recoveryBusy || Boolean(busyKey);
  const knownManifest = manifest !== null;
  const installEnabled = knownManifest && status.state === "not_installed";
  const installLabel = !knownManifest
    ? "카탈로그 없음"
    : status.state === "needs_reinstall"
      ? "먼저 제거"
      : status.state === "installed"
        ? "최신 버전"
        : hasOtherVersion
          ? "업데이트"
          : "설치";
  const metadata = metadataFor(manifest, status, "uninstall");

  return (
    <article className="lsp-installer-card" data-testid={`managed-install-${actionKey}`}>
      <div className="lsp-installer-card-head">
        <div>
          <strong>{status.manifest_id}</strong>
          <span>{status.version} · {status.platform}</span>
        </div>
        <span className={`lsp-state ${status.state}`}>{statusLabel(status.state)}</span>
      </div>
      <dl className="lsp-installer-metadata">
        <div><dt>Source</dt><dd>{metadata.sourceUrl}</dd></div>
        <div><dt>License</dt><dd>{metadata.license}</dd></div>
        <div><dt>Artifact</dt><dd>{metadata.artifactUrl}</dd></div>
        <div><dt>SHA-256</dt><dd className="lsp-installer-digest">{metadata.sha256}</dd></div>
        <div><dt>Size</dt><dd>{formatSize(manifest?.artifact.size_bytes ?? null)}</dd></div>
        <div><dt>Runtime</dt><dd>{metadata.runtime}</dd></div>
      </dl>
      {!knownManifest && (
        <p className="lsp-warning">이 버전은 현재 검토된 catalog에 없습니다. 설치는 할 수 없고, indexed key를 확인한 뒤 제거만 할 수 있습니다.</p>
      )}
      {status.reason && <p className="lsp-warning">{status.reason}</p>}
      <div className="lsp-installer-actions">
        <button
          type="button"
          className="toolbar-button selected"
          disabled={blocked || !installEnabled}
          onClick={() => {
            if (manifest) onInstall(manifest, status);
          }}
        >
          {busy ? "처리 중…" : installLabel}
        </button>
        <button
          type="button"
          className="toolbar-button"
          disabled={blocked || status.state === "not_installed"}
          onClick={() => onUninstall(manifest, status)}
        >
          제거
        </button>
      </div>
    </article>
  );
}

interface Props {
  onError?: (message: string) => void;
  onChanged?: (
    catalog: ManagedServerManifest[],
    statuses: ManagedInstallStatus[],
  ) => void;
}

export default function ManagedInstallerPanel({ onError, onChanged }: Props) {
  const [catalog, setCatalog] = useState<ManagedServerManifest[]>([]);
  const [statuses, setStatuses] = useState<ManagedInstallStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);

  const statusByKey = useMemo(
    () => new Map(statuses.map((status) => [
      keyFor(status.manifest_id, status.version, status.platform),
      status,
    ])),
    [statuses],
  );
  const catalogByKey = useMemo(
    () => new Map(catalog.map((manifest) => [
      keyFor(manifest.id, manifest.version, manifest.platform),
      manifest,
    ])),
    [catalog],
  );
  const canRecover = error?.includes("installed server index is corrupt") ?? false;

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      const nextCatalog = await lspCatalog();
      setCatalog(nextCatalog);
      const nextStatuses = await lspInstalled();
      setStatuses(nextStatuses);
      onChanged?.(nextCatalog, nextStatuses);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onError?.(message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
    // The panel owns one snapshot while it is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const runRecovery = async () => {
    if (recoveryBusy) return;
    setRecoveryBusy(true);
    setError(null);
    try {
      await recoverInstalledLsp();
      await refresh();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onError?.(message);
    } finally {
      setRecoveryBusy(false);
    }
  };

  const confirmPending = async () => {
    if (!pending || busyKey) return;
    const { manifest, status, kind } = pending;
    if (kind === "install" && !manifest) return;
    const actionKey = keyFor(status.manifest_id, status.version, status.platform);
    setBusyKey(actionKey);
    setError(null);
    try {
      if (kind === "install" && manifest) {
        await installLsp(manifest.id, manifest.version, manifest.platform);
      } else {
        // The backend resolves this exact indexed key. No manifest or URL is
        // accepted from the client, which keeps orphan removal recoverable and
        // prevents catalog data from becoming a deletion authority.
        await uninstallLsp(status.manifest_id, status.version, status.platform);
      }
      setPending(null);
      await refresh();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onError?.(message);
    } finally {
      setBusyKey(null);
    }
  };

  const pendingMetadata = pending ? metadataFor(pending.manifest, pending.status, pending.kind) : null;

  return (
    <section className="lsp-installer-section" aria-label="관리형 언어 서버 설치">
      <div className="lsp-installer-heading">
        <div>
          <h3>검토된 관리형 서버</h3>
          <p>정확한 버전만 표시합니다. 다운로드와 삭제는 확인 후에만 실행됩니다.</p>
        </div>
        <span className="lsp-installer-state">자동 설치 안 함</span>
      </div>

      {error && (
        <div className="lsp-installer-error" role="alert">
          <span>설치 목록을 읽지 못했습니다: {error}</span>
          {canRecover && (
            <button
              type="button"
              className="toolbar-button"
              disabled={recoveryBusy || Boolean(busyKey)}
              onClick={() => void runRecovery()}
            >
              {recoveryBusy ? "복구 중…" : "설치 목록 명시적 복구"}
            </button>
          )}
        </div>
      )}

      {loading && <p className="lsp-empty">관리형 서버 목록을 읽는 중…</p>}
      {!loading && catalog.map((manifest) => {
        const actionKey = keyFor(manifest.id, manifest.version, manifest.platform);
        const status = statusByKey.get(actionKey) ?? {
          manifest_id: manifest.id,
          version: manifest.version,
          platform: manifest.platform,
          state: "not_installed" as const,
          reason: null,
          installed: null,
        };
        const hasOtherVersion = statuses.some((item) => (
          item.manifest_id === manifest.id
          && item.platform === manifest.platform
          && item.version !== manifest.version
          && item.installed !== null
        ));
        return (
          <ManagedInstallCard
            key={actionKey}
            manifest={manifest}
            status={status}
            hasOtherVersion={hasOtherVersion}
            loading={loading}
            busyKey={busyKey}
            recoveryBusy={recoveryBusy}
            onInstall={(nextManifest, nextStatus) => setPending({ kind: "install", manifest: nextManifest, status: nextStatus })}
            onUninstall={(nextManifest, nextStatus) => setPending({ kind: "uninstall", manifest: nextManifest, status: nextStatus })}
          />
        );
      })}
      {!loading && statuses
        .filter((status) => !catalogByKey.has(keyFor(status.manifest_id, status.version, status.platform)))
        .map((status) => {
          const actionKey = keyFor(status.manifest_id, status.version, status.platform);
          return (
            <ManagedInstallCard
              key={actionKey}
              manifest={null}
              status={status}
              hasOtherVersion={false}
              loading={loading}
              busyKey={busyKey}
              recoveryBusy={recoveryBusy}
              onInstall={() => undefined}
              onUninstall={(nextManifest, nextStatus) => setPending({ kind: "uninstall", manifest: nextManifest, status: nextStatus })}
            />
          );
        })}

      {pending && pendingMetadata && (
        <div className="lsp-confirmation-backdrop" role="presentation">
          <section className="lsp-confirmation" role="dialog" aria-modal="true" aria-label="관리형 서버 작업 확인">
            <h4>{pending.kind === "install" ? "관리형 서버 설치 확인" : "관리형 서버 제거 확인"}</h4>
            <p>다음 metadata를 확인한 뒤 작업을 승인하세요. 제거는 정확한 indexed key에만 적용됩니다.</p>
            <dl className="lsp-confirmation-metadata">
              <div><dt>이름 / 버전</dt><dd>{pending.status.manifest_id} · {pending.status.version}</dd></div>
              <div><dt>Source / License</dt><dd>{pendingMetadata.sourceUrl} · {pendingMetadata.license}</dd></div>
              <div><dt>Artifact URL</dt><dd>{pendingMetadata.artifactUrl}</dd></div>
              <div><dt>SHA-256 / Size</dt><dd>{pendingMetadata.sha256} · {formatSize(pendingMetadata.size)}</dd></div>
              <div><dt>Runtime</dt><dd>{pendingMetadata.runtime}</dd></div>
            </dl>
            <div className="lsp-confirmation-actions">
              <button type="button" className="toolbar-button" disabled={Boolean(busyKey)} onClick={() => setPending(null)}>취소</button>
              <button type="button" className="toolbar-button selected" disabled={Boolean(busyKey)} onClick={() => void confirmPending()}>
                {pending.kind === "install" ? "설치 확인" : "제거 확인"}
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
