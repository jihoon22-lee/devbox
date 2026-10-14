import { useEffect, useRef, useState } from "react";
import { compareRev } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { useNativeRuntime } from "../../shell/nativeRuntime";
import { Button, EmptyState, ErrorState } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { ensureBottomSession, readBottomSession, rememberBottomSession } from "./bottomSession";
import { projectKey } from "./store";
import { TerminalView } from "./TerminalView";
import { useCreateSession, useSessions } from "./useSessions";

export function BottomTerminal() {
  const projectId = useCurrentProject((state) => state.id) ?? null;
  return <ProjectBottomTerminal key={projectKey(projectId)} projectId={projectId} />;
}
function ProjectBottomTerminal({ projectId }: { projectId: string | null }) {
  const focusRequest = useNativeRuntime((state) => state.bottomFocus);
  const client = useRpc();
  const project = projectKey(projectId);
  const list = useSessions(projectId);
  const create = useCreateSession();
  const createRef = useRef(create);
  createRef.current = create;
  const [id, setId] = useState(() => readBottomSession(client, project));
  const [error, setError] = useState(false);
  const [ended, setEnded] = useState(false);
  const [prepared, setPrepared] = useState(false);
  const snapshot = useRef(list);
  const createdRev = useRef<string | undefined>(undefined);
  const confirmed = useRef<string | undefined>(undefined);
  snapshot.current = list;
  const canPrepare = !!list.data && !list.isError && !list.isFetching;
  useEffect(() => {
    if (!canPrepare || prepared || ended || error) return;
    setPrepared(false);
    const remembered = readBottomSession(client, project);
    if (remembered && snapshot.current.items.some((session) => session.id === remembered && session.alive)) {
      confirmed.current = remembered;
      setId(remembered);
      setPrepared(true);
      return;
    }
    let disposed = false;
    createdRev.current = snapshot.current.data?.rev;
    void ensureBottomSession(client, project, async () => {
      const session = await createRef.current({
        projectId,
        cwd: null,
        title: "하단",
        command: null,
        attachZellij: null,
      });
      return session.id;
    })
      .then((created) => {
        if (!disposed) {
          setId(created);
          setPrepared(true);
        }
      })
      .catch(() => {
        if (!disposed) setError(true);
      });
    return () => {
      disposed = true;
    };
  }, [client, project, projectId, canPrepare, prepared, ended, error]);
  useEffect(() => {
    if (!prepared || !id || !list.data || list.isError || list.isFetching) return;
    const session = list.items.find((item) => item.id === id);
    if (session?.alive) {
      confirmed.current = id;
      return;
    }
    if (
      confirmed.current === id ||
      session ||
      (createdRev.current && compareRev(list.data.rev, createdRev.current) > 0)
    ) {
      rememberBottomSession(client, project);
      setId(undefined);
      setEnded(true);
    }
  }, [client, project, prepared, id, list.data, list.items, list.isError, list.isFetching]);
  const retryCreate = () => {
    setError(false);
    setEnded(false);
    setPrepared(false);
    setId(undefined);
  };
  if (list.isError)
    return (
      <ErrorState
        title="하단 터미널 목록을 불러오지 못했습니다"
        action={<Button onClick={() => void list.refetch()}>다시 시도</Button>}
      />
    );
  if (error || ended)
    return (
      <ErrorState
        title={ended ? "하단 터미널 세션이 끝났습니다" : "하단 터미널을 만들지 못했습니다"}
        action={<Button onClick={retryCreate}>다시 시도</Button>}
      />
    );
  if (id && prepared)
    return (
      <TerminalView
        focusRequest={focusRequest || undefined}
        id={id}
        onClose={() => {
          rememberBottomSession(client, project);
          setId(undefined);
          setEnded(true);
        }}
      />
    );
  return <EmptyState title="터미널을 준비하고 있습니다" />;
}
