import { RegistryProjection } from "./registryProjection";
import { listen } from "@tauri-apps/api/event";
import CloseReview from "./CloseReview";
import { nativeCall } from "./native";
import { createContextTransition } from "./contextTransition";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";

import { nativeMode, productDataAvailable, currentDescription, type Description } from "@devbox/product-shell/api";
import { configureProductTransport } from "@devbox/workspace-features/transport";
import type { Registry } from "./RegistryGate";
const RegistryGate = lazy(() => import("./RegistryGate"));
import { componentCall } from "./native";
const ProjectDefinitions = lazy(() => import("./ProjectDefinitions"));
import { sourceFilePath } from "./sourceNavigation";
import type { RuntimeLogOpenRequest } from "@devbox/workspace-features/logs";
import type { RuntimeFocusRequest, RuntimeFocusTarget } from "./runtimeNavigation";
const TerminalLogBridge = lazy(() => import("./TerminalLogBridge"));

const Problems = lazy(() => import("./Problems"));
const ContextStatus = lazy(() => import("./ContextStatus"));
const TerminalManager = lazy(() => import("./Terminal"));
const AgentHub = lazy(() => import("./agents/AgentHub"));
const Overview = lazy(() => import("@devbox/workspace-features/overview"));
const Source = lazy(() => import("@devbox/workspace-features/source"));
const NativeSource = lazy(() => import("./Source"));
const Dependencies = lazy(() => import("@devbox/workspace-features/dependencies"));
const IncomingFileReview = lazy(() => import("./IncomingFileReview"));
const Files = lazy(() => import("@devbox/workspace-features/files"));
const NativeRuntimeRoutes = lazy(() => import("./NativeRuntimeRoutes"));
const Tasks = lazy(() => import("@devbox/workspace-features/tasks"));
const Runtime = lazy(() => import("@devbox/workspace-features/runtime"));
const Logs = lazy(() => import("@devbox/workspace-features/logs"));

function NativeContent({ route, description, refreshContext, navigate }: ShellContentProps) {
  const installationId = description.handshake.installationId;
  const [transportOwner, setTransportOwner] = useState<string | null>(null);
  useEffect(() => {
    configureProductTransport(async <T,>(component: string, method: string, args: Record<string, unknown>) => {
      const snapshot = await currentDescription("workspace");
      const ownerRoute =
        component === "workspace.files" || component === "workspace.lsp"
          ? "files"
          : component === "workspace.source"
            ? "source"
            : component === "workspace.dependencies"
              ? "dependencies"
              : component === "workspace.terminal"
                ? "terminal"
                : component === "workspace.runtime"
                  ? "tasks"
                  : component === "workspace.logs"
                    ? "logs"
                    : component === "workspace.processes" || component === "workspace.process-actions"
                      ? "runtime"
                      : "overview";
      return componentCall<T>(snapshot, component, method, args, ownerRoute);
    }, installationId);
    setTransportOwner(installationId);
  }, [installationId]);
  const [terminalLogOpen, setTerminalLogOpen] = useState<RuntimeLogOpenRequest | null>(null);
  const [terminalLogConsumed, setTerminalLogConsumed] = useState<string | null>(null);
  const terminalReceipt = useRef<string | null>(null);
  const acceptTerminalLog = useCallback(
    (request: RuntimeLogOpenRequest) => {
      terminalReceipt.current = request.id;
      setTerminalLogOpen(request);
      navigate("logs");
    },
    [navigate],
  );
  const acceptProblemLog = useCallback(
    (request: RuntimeLogOpenRequest) => {
      setTerminalLogOpen(request);
      navigate("logs");
    },
    [navigate],
  );
  const consumeExternalLog = useCallback((id: string) => {
    if (terminalReceipt.current === id) {
      setTerminalLogConsumed(id);
      terminalReceipt.current = null;
    }
  }, []);
  const [runtimeFocus, setRuntimeFocus] = useState<RuntimeFocusRequest | null>(null);
  const acceptRuntimeFocus = (target: RuntimeFocusTarget) => {
    setRuntimeFocus({ id: crypto.randomUUID(), context: description.context, target });
    navigate(target.kind === "task" ? "tasks" : "runtime");
  };
  const consumeRuntimeFocus = useCallback(
    (id: string) => setRuntimeFocus((current) => (current?.id === id ? null : current)),
    [],
  );
  const [tasksDirty, setTasksDirty] = useState(false);
  const isRuntimeRoute = ["tasks", "runtime", "logs"].includes(route);
  const [runtimeVisited, setRuntimeVisited] = useState(isRuntimeRoute);
  useEffect(() => {
    if (isRuntimeRoute) setRuntimeVisited(true);
  }, [isRuntimeRoute]);
  const [ready, setReady] = useState(false);
  const [registry, setRegistry] = useState<Registry | null>(null);
  const [registryProjection] = useState(
    () => new RegistryProjection(() => nativeCall<Registry>("workspace.registry", "snapshot"), setRegistry),
  );
  const publishRegistry = useCallback(
    (snapshot: Registry) => {
      registryProjection.publish(snapshot);
    },
    [registryProjection],
  );
  const [dependenciesBusy, setDependenciesBusy] = useState(false);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [sourceDirty, setSourceDirty] = useState(false);
  const [sourceVisited, setSourceVisited] = useState(route === "source");
  useEffect(() => {
    if (route === "source") setSourceVisited(true);
  }, [route]);
  const [dependenciesVisited, setDependenciesVisited] = useState(route === "dependencies");
  useEffect(() => {
    if (route === "dependencies") setDependenciesVisited(true);
  }, [route]);
  const selectedTree = registry?.worktrees.find(
    (tree) =>
      tree.projectId === description.context?.projectId &&
      tree.id === description.context.worktreeId &&
      tree.revision === description.context.revision,
  );
  const [editing, setEditing] = useState(false);
  const [sessionRevision] = useState(0);
  const [definitionsEditing, setDefinitionsEditing] = useState(false);
  const filesCloseActions = useRef<{ flush(): Promise<void>; save(): Promise<void>; discard(): Promise<void> } | null>(
    null,
  );
  const setFilesCloseActions = useCallback((actions: typeof filesCloseActions.current) => {
    filesCloseActions.current = actions;
  }, []);
  const guardReasons = useRef<string[]>([]);
  guardReasons.current = [
    tasksDirty && "Tasks 편집",
    editing && "Files 편집 또는 저장",
    definitionsEditing && "프로젝트 정의 편집",
    dependenciesBusy && "의존성 검토",
    sourceBusy && "Source 작업",
    sourceDirty && "Source 초안",
  ].filter((reason): reason is string => Boolean(reason));
  const [transitionPending, setTransitionPending] = useState(false);
  const [transition] = useState(() =>
    createContextTransition(
      () => guardReasons.current,
      async () => {
        await filesCloseActions.current?.flush();
      },
      setTransitionPending,
    ),
  );
  const [closeRequest, setCloseRequest] = useState<{ nonce: string } | null>(null);
  useEffect(() => {
    let alive = true;
    const listener = listen<{ nonce: string }>("workspace-close-review", (event) => {
      if (alive) setCloseRequest(event.payload);
    });
    return () => {
      alive = false;
      void listener.then((unlisten) => unlisten());
    };
  }, []);
  const cancelClose = async () => {
    if (!closeRequest) return;
    await nativeCall("workspace.commands", "cancel_close", { nonce: closeRequest.nonce });
    setCloseRequest(null);
  };
  const finishClose = async (discard: boolean) => {
    if (!closeRequest) return;
    const nonFiles = guardReasons.current.filter((reason) => !reason.startsWith("Files"));
    if (nonFiles.length) throw new Error(`${nonFiles.join(", ")} 내용을 편집 화면에서 정리해 주세요.`);
    if (discard) await filesCloseActions.current?.discard();
    else await filesCloseActions.current?.save();
    await filesCloseActions.current?.flush();
    if (guardReasons.current.some((reason) => !reason.startsWith("Files")))
      throw new Error("종료 준비 중 새 편집이 발생했습니다. 편집 화면에서 확인해 주세요.");
    await nativeCall("workspace.commands", "confirm_close", { nonce: closeRequest.nonce });
  };
  const [registrySignal, setRegistrySignal] = useState(0);
  const refreshRegistry = useCallback(async () => {
    const snapshot = await registryProjection.refresh();
    setRegistrySignal((value) => value + 1);
    return snapshot;
  }, [registryProjection]);
  const [fileRequest, setFileRequest] = useState<{
    id: string;
    contextKey: string;
    path: string;
    line: number | null;
    column?: number | null;
    receivedReference?: string;
  } | null>(null);
  const [sourceNavigationError, setSourceNavigationError] = useState("");
  const [registrationRequest, setRegistrationRequest] = useState<{
    id: string;
    path: string;
    name: string;
    target: NonNullable<Description["context"]>["target"];
  } | null>(null);
  const proposeWorktree = (path: string) => {
    if (!description.context) return;
    setRegistrationRequest({
      id: crypto.randomUUID(),
      path,
      target: description.context.target,
      name: registry?.projects.find((project) => project.id === description.context?.projectId)?.name ?? "",
    });
    navigate("source");
  };
  const openSourceFile = (relative: string, line: number | null) => {
    if (!selectedTree) return;
    const path = sourceFilePath(selectedTree.binding.root, relative);
    if (!path || (line !== null && (!Number.isSafeInteger(line) || line < 1))) {
      setSourceNavigationError("Git에서 받은 파일 경로 또는 위치를 열 수 없습니다.");
      return;
    }
    setSourceNavigationError("");
    setFileRequest({ id: crypto.randomUUID(), contextKey: JSON.stringify(description.context), path, line });
    setFilesVisited(true);
    navigate("files");
  };
  const [filesVisited, setFilesVisited] = useState(route === "files");
  const openDiagnostic = (request: { id: string; relativePath: string; line: number; column: number | null }) => {
    if (!selectedTree) return;
    const path = sourceFilePath(selectedTree.binding.root, request.relativePath);
    if (!path) return;
    setFileRequest({
      id: request.id,
      contextKey: JSON.stringify(description.context),
      path,
      line: request.line,
      column: request.column,
    });
    setFilesVisited(true);
    navigate("files");
  };
  const markReady = useCallback(() => setReady(true), []);
  useEffect(() => {
    if (route === "files") setFilesVisited(true);
  }, [route]);
  if (transportOwner !== installationId) return <p role="status">Workspace를 준비하고 있습니다…</p>;
  if (!productDataAvailable(description))
    return description.deliveryState === "import" ? (
      <section aria-label="Workspace 준비">
        <p role="status">Control Center에서 설치 활성화를 완료해 주세요.</p>
        <Suspense fallback={null}>
          <RegistryGate
            setupOnly
            context={description.context}
            onContextChanged={refreshContext}
            onReady={markReady}
            editing={false}
          />
        </Suspense>
      </section>
    ) : (
      <p role="status">Control Center에서 제품 상태 확인 또는 복구를 완료해 주세요.</p>
    );
  return (
    <>
      {closeRequest && (
        <CloseReview
          reasons={guardReasons.current}
          filesDirty={editing}
          onSave={() => finishClose(false)}
          onDiscard={() => finishClose(true)}
          onCancel={cancelClose}
          onReturn={() => {
            void cancelClose().then(() =>
              navigate(sourceDirty || sourceBusy ? "source" : editing ? "files" : tasksDirty ? "tasks" : "overview"),
            );
          }}
        />
      )}
      <div hidden={ready && route === "files"}>
        <Suspense fallback={<p role="status">프로젝트 정보를 불러오고 있습니다…</p>}>
          <RegistryGate
            context={description.context}
            onContextChanged={refreshContext}
            onReady={markReady}
            editing={tasksDirty || editing || definitionsEditing || dependenciesBusy || sourceBusy || sourceDirty}
            transition={transition}
            refreshSignal={registrySignal}
            canonicalRegistry={registry}
            onSnapshot={publishRegistry}
            suggestedRoot={registrationRequest}
          />
        </Suspense>
      </div>
      {ready && selectedTree && (
        <Suspense fallback={null}>
          <ContextStatus
            description={description}
            name={
              registry?.projects.find((project) => project.id === description.context?.projectId)?.name ?? "프로젝트"
            }
            root={selectedTree.binding.root}
            navigate={navigate}
          />
        </Suspense>
      )}
      {ready && route === "problems" && (
        <Suspense fallback={<p role="status">문제 목록을 불러오고 있습니다…</p>}>
          <Problems
            description={description}
            onFile={openDiagnostic}
            onLog={acceptProblemLog}
            onRuntime={acceptRuntimeFocus}
            navigate={navigate}
          />
        </Suspense>
      )}
      {ready && description.context && (
        <div hidden={route !== "overview"} inert={transitionPending}>
          <Suspense fallback={<p role="status">프로젝트 설정을 불러오고 있습니다…</p>}>
            <ProjectDefinitions
              description={description}
              onDirtyChange={setDefinitionsEditing}
              onChanged={refreshRegistry}
            />
          </Suspense>
        </div>
      )}
      {ready && (sourceVisited || route === "source") && (
        <div className="workspace-feature-source" hidden={route !== "source"} inert={transitionPending}>
          {sourceNavigationError && <p role="alert">{sourceNavigationError}</p>}
          {!selectedTree ? (
            <p role="status">
              {description.context
                ? "프로젝트 정보를 동기화 중입니다. 목록을 다시 확인해 주세요."
                : "작업할 프로젝트를 선택해 주세요."}
            </p>
          ) : (
            <Suspense fallback={<p role="status">Source 화면을 불러오고 있습니다…</p>}>
              <NativeSource
                key={JSON.stringify(description.context)}
                description={description}
                root={selectedTree.binding.root}
                editorPending={editing}
                onBusyChange={setSourceBusy}
                onDirtyChange={setSourceDirty}
                onOpenFile={openSourceFile}
                onProposeWorktree={proposeWorktree}
              />
            </Suspense>
          )}
        </div>
      )}
      {ready && (dependenciesVisited || route === "dependencies") && (
        <div className="workspace-feature-source" hidden={route !== "dependencies"} inert={transitionPending}>
          {!selectedTree ? (
            <p role="status">
              {description.context
                ? "프로젝트 정보를 동기화 중입니다. 목록을 다시 확인해 주세요."
                : "분석할 프로젝트를 선택해 주세요."}
            </p>
          ) : (
            <Suspense fallback={<p role="status">의존성 화면을 불러오고 있습니다…</p>}>
              <Dependencies
                repo={{
                  path: selectedTree.binding.root,
                  canonicalKey: JSON.stringify(description.context),
                  hasWorktrees: false,
                }}
                onBusyChange={setDependenciesBusy}
              />
            </Suspense>
          )}
        </div>
      )}
      {ready && (
        <Suspense fallback={null}>
          <TerminalLogBridge description={description} consumedId={terminalLogConsumed} onOpen={acceptTerminalLog} />
        </Suspense>
      )}
      {ready && route === "agents" && (
        <Suspense fallback={<p role="status">에이전트 작업을 불러오고 있습니다…</p>}>
          <AgentHub
            description={description}
            registry={registry}
            navigate={navigate}
            refreshContext={refreshContext}
            refreshRegistry={refreshRegistry}
            transition={transition}
          />
        </Suspense>
      )}
      {ready && route === "terminal" && (
        <Suspense fallback={<p role="status">터미널 목록을 불러오고 있습니다…</p>}>
          <TerminalManager description={description} registry={registry} />
        </Suspense>
      )}
      {ready && (runtimeVisited || isRuntimeRoute) && (
        <Suspense fallback={<p role="status">실행 화면을 불러오고 있습니다…</p>}>
          <NativeRuntimeRoutes
            focusRequest={runtimeFocus}
            onFocusConsumed={consumeRuntimeFocus}
            route={route}
            description={description}
            navigate={navigate}
            tasksDirty={tasksDirty}
            onDirtyChange={setTasksDirty}
            onDiagnostic={openDiagnostic}
            externalLogOpen={terminalLogOpen}
            onExternalLogConsumed={consumeExternalLog}
          />
        </Suspense>
      )}
      {ready && (filesVisited || route === "files") && (
        <div className="workspace-feature-files" hidden={route !== "files"} inert={transitionPending}>
          <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}>
            <div>
              <IncomingFileReview description={description} onOpen={setFileRequest} />
              <Files
                key={sessionRevision}
                contextKey={JSON.stringify(description.context)}
                active={route === "files"}
                onCloseActions={setFilesCloseActions}
                onDirtyChange={setEditing}
                openRequest={fileRequest}
              />
            </div>
          </Suspense>
        </div>
      )}
    </>
  );
}

function Content({ route, description }: ShellContentProps) {
  const group = ["tasks", "runtime", "logs"].includes(route)
    ? route
    : route === "files"
      ? "files"
      : ["source", "dependencies"].includes(route)
        ? "source"
        : route === "overview"
          ? "overview"
          : "unavailable";
  const [visited, setVisited] = useState(() => new Set([group]));
  useEffect(() => {
    setVisited((previous) => (previous.has(group) ? previous : new Set([...previous, group])));
  }, [group]);
  return (
    <>
      {group === "unavailable" && (
        <section>
          <h1>{description.features.find((feature) => feature.route === route)?.label}</h1>
          <p role="status">이 화면의 기능 연결을 준비하고 있습니다.</p>
        </section>
      )}
      {(visited.has("overview") || group === "overview") && (
        <div className="workspace-feature-overview" hidden={group !== "overview"}>
          <Suspense fallback={<p role="status">프로젝트를 불러오고 있습니다…</p>}>
            <Overview />
          </Suspense>
        </div>
      )}
      {(visited.has("source") || group === "source") && (
        <div className="workspace-feature-source" hidden={group !== "source"}>
          <Suspense fallback={<p role="status">저장소를 불러오고 있습니다…</p>}>
            <Source />
          </Suspense>
        </div>
      )}
      {(visited.has("tasks") || group === "tasks") && (
        <div className="workspace-feature-tasks" hidden={group !== "tasks"} inert={group !== "tasks"}>
          <Suspense fallback={<p role="status">작업과 서비스를 불러오고 있습니다…</p>}>
            <Tasks active={group === "tasks"} />
          </Suspense>
        </div>
      )}
      {(visited.has("runtime") || group === "runtime") && (
        <div className="workspace-feature-runtime" hidden={group !== "runtime"} inert={group !== "runtime"}>
          <Suspense fallback={<p role="status">프로세스와 포트를 불러오고 있습니다…</p>}>
            <Runtime active={group === "runtime"} />
          </Suspense>
        </div>
      )}
      {(visited.has("logs") || group === "logs") && (
        <div className="workspace-feature-logs" hidden={group !== "logs"} inert={group !== "logs"}>
          <Suspense fallback={<p role="status">로그 화면을 불러오고 있습니다…</p>}>
            <Logs active={group === "logs"} />
          </Suspense>
        </div>
      )}
      {(visited.has("files") || group === "files") && (
        <div className="workspace-feature-files" hidden={group !== "files"}>
          <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}>
            <Files active={group === "files"} />
          </Suspense>
        </div>
      )}
    </>
  );
}

export default function Workspace() {
  return (
    <ProductShell
      product="workspace"
      renderContent={(props) => (nativeMode ? <NativeContent {...props} /> : <Content {...props} />)}
    />
  );
}
