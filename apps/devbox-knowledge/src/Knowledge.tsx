import { recoveryLazy } from "./recoveryLazy";
import { NoteSessionProvider } from "@devbox/knowledge-features/notes-lifecycle";
import RecoveryBoundary from "./RecoveryBoundary";
import { useIncomingReview } from "@devbox/product-shell/incoming";
import type { SavedSearchInput } from "@devbox/knowledge-features/search";
import { Suspense, useCallback, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";
import { productDataAvailable } from "@devbox/product-shell/api";
import QuitGuard from "./QuitGuard";
import { Startup } from "./Startup";
import { localDateKey } from "./dates";
const LifecycleSettings = recoveryLazy(() => import("./LifecycleSettings"));
const VaultSettings = recoveryLazy(() => import("./VaultSettings"));
const IncomingSessionSummary = recoveryLazy(() => import("./IncomingSessionSummary"));
const Daily = recoveryLazy(() => import("./Daily"));
const IncomingResultDraft = recoveryLazy(() => import("./IncomingResultDraft"));
const Notes = recoveryLazy(() => import("@devbox/knowledge-features/notes"));
const Activity = recoveryLazy(() => import("@devbox/knowledge-features/activity"));
const IncomingSearchReview = recoveryLazy(() => import("./IncomingSearchReview"));
const Search = recoveryLazy(() => import("@devbox/knowledge-features/search"));
function Content({ route, navigate }: ShellContentProps) {
  const { review: incoming } = useIncomingReview();
  const captureRequest =
    incoming?.route === "notes" &&
    incoming.target.kind === "entity" &&
    incoming.target.entity === "capture" &&
    incoming.target.id === "inbox"
      ? incoming.operationId
      : undefined;
  const [savedSearch, setSavedSearch] = useState<SavedSearchInput>();
  const [showSettings, setShowSettings] = useState<"vault" | null>(null);
  const activateNotes = useCallback(() => {
    setShowSettings(null);
    navigate("notes");
  }, [navigate]);
  const activateDaily = useCallback(() => navigate("daily"), [navigate]);
  const activateActivity = useCallback(() => navigate("activity"), [navigate]);
  const [date, setDate] = useState(() => localDateKey());
  const [projectRevision, setProjectRevision] = useState(0);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const stop = await listen("devbox://project-context", () => {
          if (!cancelled) setProjectRevision((value) => value + 1);
        });
        if (cancelled) stop();
        else {
          unlisten = stop;
          setProjectRevision((value) => value + 1);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => setShowSettings(null), [route]);
  const [openRequest, setOpenRequest] = useState<{ id: number; path: string }>();
  const openNote = useCallback(
    (path: string) => {
      setOpenRequest((previous) => ({ id: (previous?.id ?? 0) + 1, path }));
      navigate("notes");
    },
    [navigate],
  );
  const group = ["activity", "search", "daily"].includes(route) ? route : "notes";
  const [visited, setVisited] = useState(() => new Set([group]));
  useEffect(() => {
    setVisited((previous) => (previous.has(group) ? previous : new Set([...previous, group])));
  }, [group]);
  return (
    <>
      {group === "notes" && showSettings && (
        <div className="knowledge-settings-page">
          <button type="button" onClick={() => setShowSettings(null)}>
            노트로 돌아가기
          </button>
          <RecoveryBoundary name="기능">
            <Suspense fallback={<p role="status">설정을 불러오고 있습니다…</p>}>
              <VaultSettings />
            </Suspense>
          </RecoveryBoundary>
        </div>
      )}
      {group === "daily" && (
        <RecoveryBoundary name="기능">
          <Suspense fallback={<p role="status">일일 기록을 불러오고 있습니다…</p>}>
            <IncomingSessionSummary onNotes={activateNotes} />
            <Daily date={date} onDateChange={setDate} onOpen={openNote} onActivity={activateActivity} />
          </Suspense>
        </RecoveryBoundary>
      )}
      {(visited.has("notes") || group === "notes") && (
        <div className="knowledge-feature-notes" hidden={group !== "notes" || showSettings !== null}>
          <RecoveryBoundary name="노트" recoverNote>
            <Suspense fallback={<p role="status">노트를 불러오고 있습니다…</p>}>
              <IncomingResultDraft />
              <Notes
                active={group === "notes" && !showSettings}
                onActivate={activateNotes}
                onDaily={activateDaily}
                onVaultSettings={() => setShowSettings("vault")}
                openRequest={openRequest}
                captureRequest={captureRequest}
              />
            </Suspense>
          </RecoveryBoundary>
        </div>
      )}
      {(visited.has("activity") || group === "activity") && (
        <div className="knowledge-feature-activity" hidden={group !== "activity"}>
          <RecoveryBoundary name="기능">
            <Suspense fallback={<p role="status">활동을 불러오고 있습니다…</p>}>
              <Activity
                projectRevision={projectRevision}
                active={group === "activity"}
                selectedDate={date}
                onDateChange={setDate}
                onDaily={activateDaily}
                onDraft={activateNotes}
                lifecycleSettings={
                  <RecoveryBoundary name="기능">
                    <Suspense fallback={<p role="status">종료 설정을 불러오고 있습니다…</p>}>
                      <LifecycleSettings />
                    </Suspense>
                  </RecoveryBoundary>
                }
              />
            </Suspense>
          </RecoveryBoundary>
        </div>
      )}
      {(visited.has("search") || group === "search") && (
        <div className="knowledge-feature-search" hidden={group !== "search"}>
          <RecoveryBoundary name="기능">
            <Suspense fallback={<p role="status">검색을 불러오고 있습니다…</p>}>
              <IncomingSearchReview onNoteOpen={activateNotes} onSaved={setSavedSearch} />
              <Search savedSearch={savedSearch} projectRevision={projectRevision} onNoteOpen={activateNotes} />
            </Suspense>
          </RecoveryBoundary>
        </div>
      )}
    </>
  );
}
export default function Knowledge() {
  return (
    <NoteSessionProvider>
      <QuitGuard />
      <RecoveryBoundary name="Knowledge" recoverNote>
        <ProductShell
          product="knowledge"
          renderContent={(props) => {
            const available = productDataAvailable(props.description);
            const pending = <p role="status">저장소 준비를 마친 뒤 Control Center에서 Suite 활성화를 완료해 주세요.</p>;
            if (!available && props.description.deliveryState !== "import") return pending;
            return <Startup>{available ? <Content {...props} /> : pending}</Startup>;
          }}
        />
      </RecoveryBoundary>
    </NoteSessionProvider>
  );
}
