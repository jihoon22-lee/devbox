import { storageFailureMessage } from "../../storage/documentStorage";
import { useRef } from "react";
import { sealSecret } from "../api";
import { addEnvironment, type EnvironmentStore, saveStore as saveEnvStore, setVariable } from "../lib/environments";
import type * as React from "react";

interface Props {
  environmentRevisionRef: React.RefObject<number>;
  environmentBusyRef: React.RefObject<boolean>;
  envStoreRef: React.RefObject<import("../lib/environments").EnvironmentStore>;
  setEnvStore: React.Dispatch<React.SetStateAction<import("../lib/environments").EnvironmentStore>>;
  environmentMutationBusyRef: React.RefObject<boolean>;
  mountedRef: React.RefObject<boolean>;
  setPersistenceReady: React.Dispatch<React.SetStateAction<boolean>>;
  setPersistenceWarning: React.Dispatch<React.SetStateAction<string | null>>;
  transferBusyRef: React.RefObject<boolean>;
  setEnvironmentBusy: React.Dispatch<React.SetStateAction<boolean>>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  persistenceReady: boolean;
  envName: string;
  setCurrentEnvId: React.Dispatch<React.SetStateAction<string>>;
  setEnvName: React.Dispatch<React.SetStateAction<string>>;
}

export function useEnvironmentPersistence({
  environmentRevisionRef,
  environmentBusyRef,
  envStoreRef,
  setEnvStore,
  environmentMutationBusyRef,
  mountedRef,
  setPersistenceReady,
  setPersistenceWarning,
  transferBusyRef,
  setEnvironmentBusy,
  setError,
  persistenceReady,
  envName,
  setCurrentEnvId,
  setEnvName,
}: Props) {
  const environmentWrites = useRef(Promise.resolve());
  const environmentWriteFailed = useRef(false);
  const persistEnvs = async (
    store: EnvironmentStore,
    expectedRevision = environmentRevisionRef.current,
    allowEnvironmentBusy = false,
  ): Promise<EnvironmentStore> => {
    if (
      expectedRevision !== environmentRevisionRef.current ||
      environmentWriteFailed.current ||
      (environmentBusyRef.current && !allowEnvironmentBusy)
    )
      throw new Error("environment mutation is stale or busy");
    // Keep typing responsive while native writes serialize in the same edit order.
    const revision = ++environmentRevisionRef.current;
    envStoreRef.current = store;
    setEnvStore(store);
    environmentMutationBusyRef.current = true;
    const action = environmentWrites.current.then(async () => {
      if (environmentWriteFailed.current) throw new Error("Environment 저장을 다시 확인해야 합니다.");
      try {
        const saved = await saveEnvStore(store);
        if (revision === environmentRevisionRef.current) {
          envStoreRef.current = saved;
          if (mountedRef.current) setEnvStore(saved);
        }
        return saved;
      } catch (cause) {
        environmentWriteFailed.current = true;
        if (mountedRef.current) setPersistenceReady(false);
        throw cause;
      }
    });
    environmentWrites.current = action.then(
      () => {},
      () => {},
    );
    try {
      return await action;
    } finally {
      if (revision === environmentRevisionRef.current) environmentMutationBusyRef.current = false;
    }
  };

  const tryPersistEnvs = async (
    store: EnvironmentStore,
    expectedRevision = environmentRevisionRef.current,
    allowEnvironmentBusy = false,
  ): Promise<EnvironmentStore | null> => {
    try {
      return await persistEnvs(store, expectedRevision, allowEnvironmentBusy);
    } catch (storageCause) {
      if (mountedRef.current)
        setPersistenceWarning(
          storageFailureMessage(storageCause, "Environment를 안전하게 저장하지 못했습니다. 기존 값은 유지됩니다."),
        );
      return null;
    }
  };

  const startSecretSeal = (
    environmentId: string,
    key: string,
    plain: string,
    expectedValue: string,
    expectedSecret: boolean,
    secret: boolean,
  ) => {
    if (environmentMutationBusyRef.current || environmentBusyRef.current || transferBusyRef.current) return;
    const revision = environmentRevisionRef.current;
    environmentBusyRef.current = true;
    setEnvironmentBusy(true);
    setError(null);
    void sealSecret(plain)
      .then(async (blob) => {
        if (!mountedRef.current) return;
        const current = envStoreRef.current.environments
          .find((environment) => environment.id === environmentId)
          ?.variables.find((variable) => variable.key === key);
        if (
          revision !== environmentRevisionRef.current ||
          !current ||
          current.value !== expectedValue ||
          current.secret !== expectedSecret
        ) {
          setPersistenceWarning("Environment가 변경되어 오래된 secret 저장 결과를 적용하지 않았습니다.");
          return;
        }
        await tryPersistEnvs(setVariable(envStoreRef.current, environmentId, key, blob, secret), revision, true);
      })
      .catch(() => {
        if (mountedRef.current) setError("secret 봉인에 실패했습니다. 데스크톱 앱에서 다시 시도하세요.");
      })
      .finally(() => {
        environmentBusyRef.current = false;
        if (mountedRef.current) setEnvironmentBusy(false);
      });
  };

  const onCreateEnv = async () => {
    if (environmentBusyRef.current || transferBusyRef.current || !persistenceReady) return;
    try {
      const next = addEnvironment(
        envStoreRef.current,
        envName,
        () => `e-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      );
      const saved = await persistEnvs(next);
      setCurrentEnvId(saved.environments[0]?.id ?? "");
      setEnvName("");
    } catch (storageCause) {
      setPersistenceWarning(
        storageFailureMessage(storageCause, "Environment를 안전하게 저장하지 못했습니다. 기존 값은 유지됩니다."),
      );
    }
  };
  return { persistEnvs, onCreateEnv, tryPersistEnvs, startSecretSeal };
}
