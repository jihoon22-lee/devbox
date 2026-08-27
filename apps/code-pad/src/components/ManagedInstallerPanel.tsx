import { useEffect, useMemo, useRef, useState } from "react";
import {
  importLspArchives,
  installLsp,
  lspCatalog,
  lspInstalled,
  pickLspArchives,
  recoverInstalledLsp,
  uninstallLsp,
} from "../api";
import type {
  ManagedInstallState,
  ManagedInstallStatus,
  ManagedServerManifest,
} from "../types";

type PendingAction = {
  kind: "install" | "import" | "uninstall";
  manifest: ManagedServerManifest | null;
  status: ManagedInstallStatus;
  archivePaths?: string[];
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

function displayInstallSource(source: "network" | "archive_cache" | "local_archive" | "unknown"): string {
  switch (source) {
    case "network": return "network";
    case "archive_cache": return "archive cache";
    case "local_archive": return "local archive";
    default: return "legacy/unknown";
  }
}

interface DisplayMetadata {
  sourceUrl: string;
  license: string;
  artifactUrl: string;
  sha256: string;
  size: number | null;
  runtime: string;
  installSource: string;
  lastVerifiedAt: string;
}

function metadataFor(
  manifest: ManagedServerManifest | null,
  status: ManagedInstallStatus,
  kind: PendingAction["kind"],
): DisplayMetadata {
  const indexed = status.installed;
  const catalogIsAuthority = kind !== "uninstall";
  const installSource = indexed?.install_source ?? "unknown";
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
    installSource: catalogIsAuthority
      ? "사용자 확인 후 검증"
      : displayInstallSource(installSource),
    lastVerifiedAt: indexed?.last_verified_at ?? "확인 기록 없음",
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
  onImport: (manifest: ManagedServerManifest, status: ManagedInstallStatus) => void;
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
  onImport,
  onUninstall,
}: CardProps) {
  const actionKey = keyFor(status.manifest_id, status.version, status.platform);
  const busy = busyKey === actionKey;
  const blocked = loading || recoveryBusy || Boolean(busyKey);
  const knownManifest = manifest !== null;
  const installEnabled = knownManifest && status.state === "not_installed";
  const archiveImportEnabled = installEnabled;
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
        <div><dt>설치 source</dt><dd>{metadata.installSource}</dd></div>
        <div><dt>마지막 검증</dt><dd>{metadata.lastVerifiedAt}</dd></div>
      </dl>
      {status.archive_cached && status.state !== "installed" && (
        <p className="lsp-cache-state">검증된 archive cache를 오프라인에서 사용할 수 있습니다.</p>
      )}
      {!knownManifest && (
        <p className="lsp-warning">이 버전은 현재 검토된 catalog에 없습니다. 설치는 할 수 없고, indexed key를 확인한 뒤 제거만 할 수 있습니다.</p>
      )}
      {status.state === "needs_reinstall" && (
        <p className="lsp-warning">설치된 파일 또는 metadata 검증에 실패했습니다. 제거한 뒤 다시 설치하세요.</p>
      )}
      {knownManifest && manifest.runtime.kind === "node" && status.state === "not_installed" && (
        <p className="lsp-warning">Node 서버는 reviewed dependency closure 전체의 .tgz archive를 여러 개 선택해야 합니다. 각 archive는 native reviewed lock과 대조되며 cache와 결합할 수 있습니다.</p>
      )}
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
          disabled={blocked || !archiveImportEnabled}
          onClick={() => {
            if (manifest) onImport(manifest, status);
          }}
        >
          {busy ? "처리 중…" : "local archive 가져오기"}
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
  onChanged?: (
    catalog: ManagedServerManifest[],
    statuses: ManagedInstallStatus[],
  ) => void;
}

export default function ManagedInstallerPanel({ onChanged }: Props) {
  const [catalog, setCatalog] = useState<ManagedServerManifest[]>([]);
  const [statuses, setStatuses] = useState<ManagedInstallStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [recoveryAvailable, setRecoveryAvailable] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const mountedRef = useRef(true);
  const refreshGenerationRef = useRef(0);
  // State updates do not synchronously change event-handler closures. Keep a
  // ref gate as the authority so picker, recovery, and confirmed mutations
  // cannot be started twice by rapid clicks.
  const operationInFlightRef = useRef(false);

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
  const refresh = async () => {
    if (!mountedRef.current) return;
    const generation = ++refreshGenerationRef.current;
    setLoading(true);
    setError(null);
    setRecoveryAvailable(false);
    try {
      const nextCatalog = await lspCatalog();
      if (!mountedRef.current || generation !== refreshGenerationRef.current) return;
      const nextStatuses = await lspInstalled();
      if (!mountedRef.current || generation !== refreshGenerationRef.current) return;
      setCatalog(nextCatalog);
      setStatuses(nextStatuses);
      onChanged?.(nextCatalog, nextStatuses);
    } catch (cause) {
      if (!mountedRef.current || generation !== refreshGenerationRef.current) return;
      const detail = cause instanceof Error ? cause.message : String(cause);
      const message = "관리형 서버 상태를 확인하지 못했습니다.";
      setRecoveryAvailable(detail === "관리형 서버 설치 목록 복구가 필요합니다");
      setError(message);
    } finally {
      if (mountedRef.current && generation === refreshGenerationRef.current) {
        setLoading(false);
      }
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
      refreshGenerationRef.current += 1;
    };
    // The panel owns one snapshot while it is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const runRecovery = async () => {
    if (operationInFlightRef.current) return;
    operationInFlightRef.current = true;
    setRecoveryBusy(true);
    setError(null);
    try {
      await recoverInstalledLsp();
      if (mountedRef.current) await refresh();
    } catch {
      if (mountedRef.current) setError("설치 목록을 복구하지 못했습니다.");
    } finally {
      operationInFlightRef.current = false;
      if (mountedRef.current) setRecoveryBusy(false);
    }
  };

  const chooseArchive = async (
    manifest: ManagedServerManifest,
    status: ManagedInstallStatus,
  ) => {
    if (operationInFlightRef.current) return;
    operationInFlightRef.current = true;
    const actionKey = keyFor(status.manifest_id, status.version, status.platform);
    setBusyKey(actionKey);
    setError(null);
    try {
      const archivePaths = await pickLspArchives();
      if (mountedRef.current && archivePaths.length > 0) {
        setPending({ kind: "import", manifest, status, archivePaths });
      }
    } catch {
      // Native picker/parser details (including the selected path) stay out
      // of the UI and IPC error channel.
      if (mountedRef.current) setError("local archive를 선택하지 못했습니다.");
    } finally {
      operationInFlightRef.current = false;
      if (mountedRef.current) setBusyKey(null);
    }
  };

  const confirmPending = async () => {
    if (!pending || operationInFlightRef.current) return;
    const { manifest, status, kind } = pending;
    if ((kind === "install" || kind === "import") && !manifest) return;
    operationInFlightRef.current = true;
    const actionKey = keyFor(status.manifest_id, status.version, status.platform);
    setBusyKey(actionKey);
    setError(null);
    try {
      if (kind === "install" && manifest) {
        await installLsp(manifest.id, manifest.version, manifest.platform);
      } else if (kind === "import" && manifest && pending.archivePaths) {
        await importLspArchives(
          manifest.id,
          manifest.version,
          manifest.platform,
          pending.archivePaths,
        );
      } else {
        // The backend resolves this exact indexed key. No manifest or URL is
        // accepted from the client, which keeps orphan removal recoverable and
        // prevents catalog data from becoming a deletion authority.
        await uninstallLsp(status.manifest_id, status.version, status.platform);
      }
      if (mountedRef.current) {
        setPending(null);
        await refresh();
      }
    } catch {
      const message = kind === "uninstall"
        ? "관리형 서버를 제거하지 못했습니다."
        : kind === "import"
          ? "local archive를 가져오지 못했습니다."
          : "관리형 서버를 설치하지 못했습니다.";
      if (mountedRef.current) setError(message);
    } finally {
      operationInFlightRef.current = false;
      if (mountedRef.current) setBusyKey(null);
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
          <span>{error}</span>
          {recoveryAvailable && (
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
          archive_cached: false,
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
            onImport={(nextManifest, nextStatus) => void chooseArchive(nextManifest, nextStatus)}
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
              onImport={() => undefined}
              onUninstall={(nextManifest, nextStatus) => setPending({ kind: "uninstall", manifest: nextManifest, status: nextStatus })}
            />
          );
        })}

      {pending && pendingMetadata && (
        <div className="lsp-confirmation-backdrop" role="presentation">
          <section className="lsp-confirmation" role="dialog" aria-modal="true" aria-label="관리형 서버 작업 확인">
            <h4>{pending.kind === "uninstall" ? "관리형 서버 제거 확인" : pending.kind === "import" ? "local archive 가져오기 확인" : "관리형 서버 설치 확인"}</h4>
            <p>{pending.kind === "import"
              ? pending.manifest?.runtime.kind === "node"
                ? "선택한 .tgz archive set은 reviewed dependency closure와 exact SHA-256·integrity가 모두 맞을 때만 app-owned cache에 복사됩니다."
                : "선택한 archive는 아래 SHA-256과 일치할 때만 app-owned cache에 복사됩니다."
              : "다음 metadata를 확인한 뒤 작업을 승인하세요. 제거는 정확한 indexed key에만 적용됩니다."}</p>
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
                {pending.kind === "uninstall" ? "제거 확인" : pending.kind === "import" ? "가져오기 확인" : "설치 확인"}
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
