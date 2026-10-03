import { useEffect, type RefObject, type Dispatch, type SetStateAction } from "react";
import { isProductHosted } from "../../transport";
import { saveSession } from "../api";
import { safeCodePadError } from "../lib/documentPresentation";
import { stateToSession } from "../store/documentStore";
import type { EditorState, SessionState } from "../types";

interface SessionPersistenceOptions {
  contextKey: string;
  hydrated: boolean;
  sessionPersistenceAllowed: boolean;
  state: EditorState;
  hydratedRef: RefObject<boolean>;
  sessionWriteBlockedRef: RefObject<boolean>;
  sessionSaveTimerRef: RefObject<ReturnType<typeof setTimeout> | null>;
  pendingSessionRef: RefObject<SessionState | null>;
  contextRef: RefObject<string>;
  sessionSaveInFlightRef: RefObject<Promise<void> | null>;
  sessionRevisionRef: RefObject<string | undefined>;
  persistenceAllowedRef: RefObject<boolean>;
  setSessionPersistenceAllowed: Dispatch<SetStateAction<boolean>>;
  setError: Dispatch<SetStateAction<string | null>>;
}

export function useSessionPersistence({
  contextKey,
  hydrated,
  sessionPersistenceAllowed,
  state,
  hydratedRef,
  sessionWriteBlockedRef,
  sessionSaveTimerRef,
  pendingSessionRef,
  contextRef,
  sessionSaveInFlightRef,
  sessionRevisionRef,
  persistenceAllowedRef,
  setSessionPersistenceAllowed,
  setError,
}: SessionPersistenceOptions) {
  // A single debounced, serialized writer means a slow save cannot let an old
  // request finish after a newer request and overwrite the newest session.
  useEffect(() => {
    if (!hydrated || !hydratedRef.current || !sessionPersistenceAllowed || sessionWriteBlockedRef.current) return;
    if (sessionSaveTimerRef.current) clearTimeout(sessionSaveTimerRef.current);
    pendingSessionRef.current = stateToSession(state);
    const startDrain = () => {
      if (contextRef.current !== contextKey) return;
      if (sessionSaveInFlightRef.current) {
        void sessionSaveInFlightRef.current.finally(() => {
          if (contextRef.current === contextKey && pendingSessionRef.current) startDrain();
        });
        return;
      }
      const drain = async () => {
        while (contextRef.current === contextKey && pendingSessionRef.current && !sessionWriteBlockedRef.current) {
          const next = pendingSessionRef.current;
          pendingSessionRef.current = null;
          try {
            const revision = await saveSession(next, sessionRevisionRef.current);
            if (contextRef.current === contextKey) sessionRevisionRef.current = revision;
          } catch (cause) {
            if (contextRef.current !== contextKey) return;
            // A failed product write may be a stale revision or a committed
            // write whose reply was lost. Never retry with the old snapshot.
            if (isProductHosted() || sessionRevisionRef.current !== undefined) {
              sessionWriteBlockedRef.current = true;
              pendingSessionRef.current = null;
              persistenceAllowedRef.current = false;
              setSessionPersistenceAllowed(false);
            }
            setError(safeCodePadError(cause, "편집 세션을 저장하지 못했습니다."));
          }
        }
      };
      const inFlight = drain();
      sessionSaveInFlightRef.current = inFlight;
      void inFlight.finally(() => {
        if (sessionSaveInFlightRef.current === inFlight) sessionSaveInFlightRef.current = null;
        if (contextRef.current === contextKey && pendingSessionRef.current && !sessionSaveTimerRef.current) {
          // Keep the same quiet debounce for edits that arrived while the
          // previous native write was in flight.
          sessionSaveTimerRef.current = setTimeout(() => {
            sessionSaveTimerRef.current = null;
            startDrain();
          }, 1_000);
        }
      });
    };
    sessionSaveTimerRef.current = setTimeout(() => {
      sessionSaveTimerRef.current = null;
      startDrain();
    }, 1_000);
    return () => {
      if (sessionSaveTimerRef.current) {
        clearTimeout(sessionSaveTimerRef.current);
        sessionSaveTimerRef.current = null;
      }
    };
  }, [
    contextKey,
    hydrated,
    sessionPersistenceAllowed,
    state,
    hydratedRef,
    sessionWriteBlockedRef,
    sessionSaveTimerRef,
    pendingSessionRef,
    contextRef,
    sessionSaveInFlightRef,
    sessionRevisionRef,
    persistenceAllowedRef,
    setSessionPersistenceAllowed,
    setError,
  ]);
}
