import type { ChangeEvent, Dispatch, RefObject, SetStateAction } from "react";
import * as api from "../api";
import { readJsonFile, saveJsonFile } from "../api";
import { apiMessages } from "../../issues/catalog";
import type { ApiIssue } from "../../generated/ApiIssue";
import { documentSession, documentStorage, storageFailureMessage } from "../../storage/documentStorage";
import type { ImportPreview } from "../ImportDialog";
import { applyImportDocuments } from "../lib/importTransaction";
import {
  emptyStore as emptyCollectionStore,
  migrateCollections,
  sanitizeStore as sanitizeCollectionStore,
  saveStore,
  type CollectionStore,
} from "../lib/collections";
import { emptyStore as emptyEnvStore, loadStore as loadEnvStore, type EnvironmentStore } from "../lib/environments";
import {
  mergeImportedCollections,
  mergeImportedEnvironments,
  MAX_TRANSFER_BYTES,
  parseCollectionExport,
  parseEnvironmentExport,
  readTransferFile,
  serializeCollectionExport,
  serializeEnvironmentExport,
} from "../lib/transfer";
import { downloadJson } from "../lib/requestPresentation";
import { isTauri } from "../lib/isTauri";
type Setter<T> = Dispatch<SetStateAction<T>>;
interface Context {
  collectionStoreRef: RefObject<CollectionStore>;
  collectionRevisionRef: RefObject<number>;
  collectionMutationBusyRef: RefObject<boolean>;
  envStoreRef: RefObject<EnvironmentStore>;
  environmentRevisionRef: RefObject<number>;
  environmentMutationBusyRef: RefObject<boolean>;
  environmentBusyRef: RefObject<boolean>;
  transferBusyRef: RefObject<boolean>;
  mountedRef: RefObject<boolean>;
  browserImportInputRef: RefObject<HTMLInputElement | null>;
  browserImportKindRef: RefObject<"collection" | "environment" | null>;
  setCollections: Setter<CollectionStore>;
  setEnvStore: Setter<EnvironmentStore>;
  setPersistenceReady: Setter<boolean>;
  setPersistenceWarning: Setter<string | null>;
  setTransferBusy: Setter<boolean>;
  setSelectedCollectionId: Setter<string | null>;
  setMigrationNotice: Setter<string | null>;
  setCurrentEnvId: Setter<string>;
  setBrowserImportKind: Setter<"collection" | "environment" | null>;
  persistenceReady: boolean;
  browserImportKind: "collection" | "environment" | null;
  sending: boolean;
  collSaving: boolean;
  contextActionBusy: boolean;
  currentEnvId: string;
  sanitizeForPersistence: (serialized: string) => Promise<string>;
  persistEnvs: (store: EnvironmentStore, expectedRevision?: number) => Promise<EnvironmentStore>;
}
/** Request document transfers share revision guards and conditional undo. */
export function createRequestTransferActions(context: Context) {
  const {
    collectionStoreRef,
    collectionRevisionRef,
    collectionMutationBusyRef,
    envStoreRef,
    environmentRevisionRef,
    environmentMutationBusyRef,
    environmentBusyRef,
    transferBusyRef,
    mountedRef,
    browserImportInputRef,
    browserImportKindRef,
    setCollections,
    setEnvStore,
    setPersistenceReady,
    setPersistenceWarning,
    setTransferBusy,
    setSelectedCollectionId,
    setMigrationNotice,
    setCurrentEnvId,
    setBrowserImportKind,
    persistenceReady,
    browserImportKind,
    sending,
    collSaving,
    contextActionBusy,
    currentEnvId,
    sanitizeForPersistence,
    persistEnvs,
  } = context;
  const persistCollections = async (
    store: ReturnType<typeof emptyCollectionStore>,
    expectedRevision = collectionRevisionRef.current,
  ) => {
    if (expectedRevision !== collectionRevisionRef.current || collectionMutationBusyRef.current) {
      throw new Error("collection mutation is stale or busy");
    }
    collectionMutationBusyRef.current = true;
    try {
      const safe = await saveStore(
        store,
        sanitizeForPersistence,
        undefined,
        () => expectedRevision === collectionRevisionRef.current,
      );
      if (expectedRevision !== collectionRevisionRef.current) throw new Error("collection mutation is stale");
      collectionStoreRef.current = safe;
      collectionRevisionRef.current += 1;
      if (mountedRef.current) setCollections(safe);
      return safe;
    } catch (cause) {
      if (cause instanceof Error && cause.name === "store_revision_conflict") {
        const reloaded = await migrateCollections(sanitizeForPersistence);
        if (!reloaded.failed) {
          collectionStoreRef.current = reloaded.store;
          collectionRevisionRef.current += 1;
          if (mountedRef.current) setCollections(reloaded.store);
        }
      }
      throw cause;
    } finally {
      collectionMutationBusyRef.current = false;
    }
  };

  const applyImportPreview = async (preview: ImportPreview) => {
    const available = () =>
      persistenceReady &&
      !transferBusyRef.current &&
      !collectionMutationBusyRef.current &&
      !environmentMutationBusyRef.current &&
      !environmentBusyRef.current;
    if (!available()) throw new Error("저장 작업이 진행 중입니다.");
    const storage = documentStorage();
    const reload = async () => {
      const collections = await migrateCollections(sanitizeForPersistence);
      const environments = await loadEnvStore();
      if (collections.failed) throw new Error("저장 상태를 확인하지 못했습니다.");
      collectionStoreRef.current = collections.store;
      envStoreRef.current = environments;
      collectionRevisionRef.current++;
      environmentRevisionRef.current++;
      if (mountedRef.current) {
        setCollections(collections.store);
        setEnvStore(environments);
      }
    };
    const run = async <T>(operation: () => Promise<T>): Promise<T> => {
      if (!available()) throw new Error("저장 작업이 진행 중입니다.");
      transferBusyRef.current = true;
      setTransferBusy(true);
      try {
        return await operation();
      } finally {
        try {
          await reload();
        } catch {
          if (mountedRef.current) {
            setPersistenceReady(false);
            setPersistenceWarning("저장 상태를 확인하지 못했습니다. 앱을 다시 열어 확인하세요.");
          }
        }
        transferBusyRef.current = false;
        if (mountedRef.current) setTransferBusy(false);
      }
    };
    const undo = await run(async () => {
      let sequence = 0;
      const makeId = () => `import-${Date.now()}-${sequence++}`;
      const collections = mergeImportedCollections(collectionStoreRef.current, preview.collections, makeId);
      const environments = mergeImportedEnvironments(envStoreRef.current, preview.environments, makeId);
      if (!collections || !environments) throw new Error("가져오기 한도를 초과했습니다.");
      const changes = [];
      if (preview.collections.collections.length) {
        const before = await documentSession("collections", storage).snapshot();
        const safe = await sanitizeCollectionStore(collections, sanitizeForPersistence);
        changes.push({
          kind: "collections" as const,
          before,
          emptyBody: JSON.stringify(emptyCollectionStore()),
          body: JSON.stringify(safe),
        });
      }
      if (preview.environments.environments.length) {
        const before = await documentSession("environments", storage).snapshot();
        changes.push({
          kind: "environments" as const,
          before,
          emptyBody: JSON.stringify(emptyEnvStore()),
          body: JSON.stringify(environments),
        });
      }
      return applyImportDocuments(storage, changes);
    });
    return () => run(undo);
  };

  const applyImportedTransfer = async (
    kind: "collection" | "environment",
    raw: string,
    expectedCollectionRevision: number,
    expectedEnvironmentRevision: number,
  ) => {
    if (kind === "collection") {
      const imported = parseCollectionExport(raw);
      if (!imported) throw new Error("컬렉션 JSON 형식이 올바르지 않습니다");
      if (expectedCollectionRevision !== collectionRevisionRef.current) {
        throw new Error("오래된 컬렉션 가져오기로 현재 상태를 덮어쓰지 않았습니다");
      }
      let sequence = 0;
      const merged = mergeImportedCollections(
        collectionStoreRef.current,
        imported,
        () => `c-import-${Date.now()}-${sequence++}`,
      );
      if (!merged) throw new Error("컬렉션 가져오기를 한 번에 적용할 수 없습니다");
      const previousCount = collectionStoreRef.current.collections.length;
      const safe = await persistCollections(merged, expectedCollectionRevision);
      if (!mountedRef.current) return;
      const added = Math.max(0, safe.collections.length - previousCount);
      setSelectedCollectionId(safe.collections[0]?.id ?? null);
      setMigrationNotice(`컬렉션 ${added}건을 추가했습니다. 기존 항목은 덮어쓰지 않았습니다.`);
      return;
    }

    const imported = parseEnvironmentExport(raw);
    if (!imported) throw new Error("환경 JSON 형식이 올바르지 않습니다");
    if (expectedEnvironmentRevision !== environmentRevisionRef.current) {
      throw new Error("오래된 환경 가져오기로 현재 상태를 덮어쓰지 않았습니다");
    }
    let sequence = 0;
    const next = mergeImportedEnvironments(envStoreRef.current, imported, () => `e-import-${Date.now()}-${sequence++}`);
    if (!next) throw new Error("환경 가져오기를 한 번에 적용할 수 없습니다");
    const previousCount = envStoreRef.current.environments.length;
    const saved = await persistEnvs(next, expectedEnvironmentRevision);
    if (!mountedRef.current) return;
    const added = Math.max(0, saved.environments.length - previousCount);
    if (!currentEnvId) setCurrentEnvId(saved.environments[0]?.id ?? "");
    setMigrationNotice(`환경 ${added}건을 추가했습니다. secret 값은 보안상 다시 입력해야 합니다.`);
  };

  const onImportTransfer = (kind: "collection" | "environment") => {
    if (
      !persistenceReady ||
      transferBusyRef.current ||
      browserImportKind ||
      environmentBusyRef.current ||
      sending ||
      collSaving ||
      contextActionBusy
    )
      return;
    const expectedCollectionRevision = collectionRevisionRef.current;
    const expectedEnvironmentRevision = environmentRevisionRef.current;
    if (isTauri()) {
      transferBusyRef.current = true;
      setTransferBusy(true);
      setPersistenceWarning(null);
      void (async () => {
        try {
          const raw = await readJsonFile();
          if (raw !== null) {
            await applyImportedTransfer(kind, raw, expectedCollectionRevision, expectedEnvironmentRevision);
          }
        } catch (storageCause) {
          if (mountedRef.current)
            setPersistenceWarning(
              storageFailureMessage(storageCause, "JSON 파일을 가져오지 않았습니다. 파일 선택과 schema를 확인하세요."),
            );
        } finally {
          transferBusyRef.current = false;
          if (mountedRef.current) setTransferBusy(false);
        }
      })();
      return;
    }
    const input = browserImportInputRef.current;
    if (!input) return;
    browserImportKindRef.current = kind;
    input.value = "";
    setBrowserImportKind(kind);
    input.click();
  };

  const onBrowserImportFile = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0];
    const kind = browserImportKindRef.current;
    event.currentTarget.value = "";
    browserImportKindRef.current = null;
    setBrowserImportKind(null);
    if (!file || !kind) return;
    if (file.size > MAX_TRANSFER_BYTES) {
      setPersistenceWarning("JSON 파일이 허용된 크기(1 MiB)를 초과해 가져오지 않았습니다.");
      return;
    }
    transferBusyRef.current = true;
    setTransferBusy(true);
    setPersistenceWarning(null);
    const expectedCollectionRevision = collectionRevisionRef.current;
    const expectedEnvironmentRevision = environmentRevisionRef.current;
    void readTransferFile(file)
      .then(async (raw) => {
        try {
          await applyImportedTransfer(kind, raw, expectedCollectionRevision, expectedEnvironmentRevision);
        } catch {
          if (mountedRef.current) {
            setPersistenceWarning(
              `${kind === "collection" ? "컬렉션" : "환경"} JSON을 가져오지 않았습니다. schema, 크기와 secret 정책을 확인하세요.`,
            );
          }
        }
      })
      .catch((storageCause) => {
        if (mountedRef.current)
          setPersistenceWarning(storageFailureMessage(storageCause, "JSON 파일을 읽지 못해 가져오지 않았습니다."));
      })
      .finally(() => {
        transferBusyRef.current = false;
        if (mountedRef.current) setTransferBusy(false);
      });
  };

  const onExportCollectionFolder = async () => {
    if (
      !persistenceReady ||
      transferBusyRef.current ||
      collectionMutationBusyRef.current ||
      environmentMutationBusyRef.current ||
      environmentBusyRef.current
    )
      return;
    transferBusyRef.current = true;
    setTransferBusy(true);
    setPersistenceWarning(null);
    try {
      const grant = await api.pickCollectionFolder();
      if (!grant || !mountedRef.current) return;
      const { serializeFileCollection } = await import("../lib/fileCollection");
      const safe = await sanitizeCollectionStore(collectionStoreRef.current, sanitizeForPersistence);
      if (!mountedRef.current) return;
      const result = await api.writeCollectionFolder(grant.grant_id, serializeFileCollection(grant.name, safe));
      if (!mountedRef.current) return;
      const stale = result.stale.length
        ? ` 이 폴더에 더 이상 없는 요청 파일 ${result.stale.length}개가 남아 있습니다: ${result.stale.slice(0, 20).join(", ")}${result.stale.length > 20 ? " …" : ""}`
        : "";
      setMigrationNotice(`요청 ${safe.collections.length}개를 폴더에 저장했습니다.${stale}`);
    } catch (cause) {
      if (mountedRef.current)
        setPersistenceWarning(
          cause instanceof Error && Object.prototype.hasOwnProperty.call(apiMessages, cause.name)
            ? apiMessages[cause.name as ApiIssue]
            : "파일 컬렉션을 저장하지 못했습니다. 폴더의 현재 내용을 확인하세요.",
        );
    } finally {
      transferBusyRef.current = false;
      if (mountedRef.current) setTransferBusy(false);
    }
  };

  const onExportTransfer = (kind: "collection" | "environment") => {
    if (
      !persistenceReady ||
      transferBusyRef.current ||
      environmentBusyRef.current ||
      sending ||
      collSaving ||
      contextActionBusy
    )
      return;
    transferBusyRef.current = true;
    setTransferBusy(true);
    setPersistenceWarning(null);
    try {
      const content =
        kind === "collection"
          ? serializeCollectionExport(collectionStoreRef.current)
          : serializeEnvironmentExport(envStoreRef.current);
      if (new TextEncoder().encode(content).byteLength > MAX_TRANSFER_BYTES) {
        throw new Error("transfer too large");
      }
      const fileName = kind === "collection" ? "api-playground-collections.json" : "api-playground-environments.json";
      if (isTauri()) {
        void saveJsonFile(content, fileName)
          .then((saved) => {
            if (saved && mountedRef.current) {
              setMigrationNotice(`${kind === "collection" ? "컬렉션" : "환경"} JSON 내보내기를 완료했습니다.`);
            }
          })
          .catch((storageCause) => {
            if (mountedRef.current)
              setPersistenceWarning(
                storageFailureMessage(storageCause, "JSON 파일을 저장하지 않았습니다. native 저장 위치를 확인하세요."),
              );
          })
          .finally(() => {
            transferBusyRef.current = false;
            if (mountedRef.current) setTransferBusy(false);
          });
      } else {
        downloadJson(content, fileName);
        setMigrationNotice(`${kind === "collection" ? "컬렉션" : "환경"} JSON 다운로드를 시작했습니다.`);
        transferBusyRef.current = false;
        setTransferBusy(false);
      }
    } catch (storageCause) {
      setPersistenceWarning(
        storageFailureMessage(storageCause, "JSON 내보내기를 생성하지 못했습니다. 항목 수와 크기를 확인하세요."),
      );
      transferBusyRef.current = false;
      setTransferBusy(false);
    }
  };

  return {
    persistCollections,
    applyImportPreview,
    onImportTransfer,
    onBrowserImportFile,
    onExportCollectionFolder,
    onExportTransfer,
  };
}
