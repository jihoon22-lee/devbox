import { useEffect, useRef, useState } from "react";
import type { LogFilter, LogLine, LogSource, LogStream, SearchCursor } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import { VirtualList } from "../../ui/VirtualList";
import { sendToTransforms } from "../tools/transforms/handoff";
import { SelectionMenu } from "../tools/transforms/SelectionMenu";
import { LogBody, type LogLocation } from "./LogBody";
import { LogSources } from "./LogSources";
import s from "./LogView.module.css";
import { followState, retainLogLines, sourceKey } from "./model";
import { useLogLines } from "./useLogLines";

export interface LogViewProps {
  sources: LogSource[];
  projectId?: string;
  onPinSources?: (sources: LogSource[]) => void;
  onOpenLocation?: (location: LogLocation) => void | Promise<void>;
}
export function LogView({ sources, projectId, onOpenLocation, onPinSources }: LogViewProps) {
  const identity = JSON.stringify(sources);
  return (
    <LogSession
      key={`${projectId ?? ""}:${identity}`}
      sources={sources}
      onOpenLocation={onOpenLocation}
      onPinSources={onPinSources}
    />
  );
}
function LogSession({ sources: initialSources, onOpenLocation, onPinSources }: LogViewProps) {
  const { client, state, generation } = useRpcSession();
  const [sources, setSources] = useState(initialSources);
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [stream, setStream] = useState<LogStream | "">("");
  const [filter, setFilter] = useState<LogFilter>({});
  const [follow, setFollow] = useState(true);
  const [retry, setRetry] = useState(0);
  const live = useLogLines(sources, filter, retry);
  const [search, setSearch] = useState<{ lines: LogLine[]; next: SearchCursor | null } | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const searchRequest = useRef<{ controller: AbortController; id: number } | null>(null);
  const serial = useRef(0);
  const searchIdentity = JSON.stringify({ sources, query, regex, stream, generation, state });
  useEffect(() => {
    void searchIdentity;
    serial.current++;
    searchRequest.current?.controller.abort();
    searchRequest.current = null;
    setSearch(null);
    setSearching(false);
    setError(null);
    return () => {
      serial.current++;
      searchRequest.current?.controller.abort();
    };
  }, [searchIdentity]);
  const stopSearch = () => {
    serial.current++;
    searchRequest.current?.controller.abort();
    searchRequest.current = null;
    setSearching(false);
    setSearch(null);
  };
  const find = async (before?: SearchCursor) => {
    if (searchRequest.current || !sources.length || !query) return;
    const controller = new AbortController();
    const id = ++serial.current;
    searchRequest.current = { controller, id };
    setSearching(true);
    setError(null);
    try {
      const result = await client.call(
        "logs.search",
        {
          sources,
          query,
          regex,
          ...(before ? { before } : {}),
          limit: 200,
        },
        { signal: controller.signal },
      );
      if (serial.current !== id) return;
      // logs.search does not expose a stream filter; apply it to each returned page.
      const lines = result.items.filter((line) => !stream || line.stream === stream).reverse();
      setSearch((old) => ({
        lines: before ? retainLogLines(lines, old?.lines ?? []) : lines,
        next: result.nextCursor,
      }));
      setFollow(false);
    } catch {
      if (serial.current === id) setError("로그를 검색하지 못했습니다. 검색식과 소스를 확인해 주세요.");
    } finally {
      if (serial.current === id) {
        searchRequest.current = null;
        setSearching(false);
      }
    }
  };
  const openLocation = onOpenLocation
    ? (location: LogLocation) => {
        try {
          void Promise.resolve(onOpenLocation(location)).catch(() => setError("파일 위치를 열지 못했습니다."));
        } catch {
          setError("파일 위치를 열지 못했습니다.");
        }
      }
    : undefined;
  const lines = search?.lines ?? live.lines;
  return (
    <section className={s.view} aria-label="실행 로그">
      <LogSources
        sources={sources}
        onAdd={(source) =>
          setSources((old) => (old.some((s) => sourceKey(s) === sourceKey(source)) ? old : [...old, source]))
        }
        onRemove={(source) => setSources((old) => old.filter((s) => sourceKey(s) !== sourceKey(source)))}
      />
      {onPinSources && (
        <Button size="sm" disabled={!sources.length} onClick={() => onPinSources(sources)}>
          하단 로그에 고정
        </Button>
      )}
      <form
        className={s.toolbar}
        onSubmit={(event) => {
          event.preventDefault();
          stopSearch();
          setFilter({
            ...(query ? (regex ? { regex: query } : { text: query }) : {}),
            ...(stream ? { streams: [stream] } : {}),
          });
          setSearch(null);
          setFollow(true);
        }}
      >
        <input
          aria-label="로그 검색"
          placeholder="텍스트 또는 정규식"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <label>
          <input type="checkbox" checked={regex} onChange={(event) => setRegex(event.target.checked)} />
          정규식
        </label>
        <select
          aria-label="스트림"
          value={stream}
          onChange={(event) => setStream(event.target.value as LogStream | "")}
        >
          <option value="">모든 스트림</option>
          <option value="stdout">stdout</option>
          <option value="stderr">stderr</option>
          <option value="system">system</option>
        </select>
        <Button type="submit" size="sm">
          필터 적용
        </Button>
        <Button
          size="sm"
          disabled={!sources.length || !query || searching || !live.connected}
          onClick={() => void find()}
        >
          지난 로그 검색
        </Button>
      </form>
      {(error || live.error) && (
        <Banner
          tone="danger"
          actions={
            live.error ? (
              <Button size="sm" onClick={() => setRetry((n) => n + 1)}>
                다시 연결
              </Button>
            ) : undefined
          }
        >
          {error ?? live.error}
        </Banner>
      )}
      {!sources.length && <p>로그 소스를 추가해 주세요.</p>}
      {!live.connected && <p role="status">데몬 연결을 기다리는 중입니다.</p>}
      {searching && <p role="status">로그 검색 중…</p>}
      {search && (
        <div className={s.toolbar}>
          <span>검색 결과 {search.lines.length}줄</span>
          {search.next && (
            <Button size="sm" disabled={searching} onClick={() => void find(search.next ?? undefined)}>
              이전 결과
            </Button>
          )}
          <Button
            size="sm"
            onClick={() => {
              stopSearch();
              setFollow(true);
            }}
          >
            실시간 로그
          </Button>
        </div>
      )}
      <SelectionMenu
        label="로그 선택 작업"
        className={s.selection}
        getSelection={(element) => {
          const selection = window.getSelection();
          if (!selection || !element.contains(selection.anchorNode) || !element.contains(selection.focusNode))
            return "";
          return selection.toString();
        }}
        onSend={(text) => void sendToTransforms(text, "log")}
      >
        <VirtualList
          items={lines}
          follow={follow && search === null}
          label="로그 줄"
          onAtBottomChange={(atBottom) => setFollow((old) => followState(old, { type: "userScrolled", atBottom }))}
          renderItem={(line) => <LogBody line={line} onOpenLocation={openLocation} />}
        />
      </SelectionMenu>
      <div className={s.footer}>
        <span>
          {search ? "검색 결과" : follow ? "따라가는 중" : "따라가기 멈춤"} · {lines.length}줄
          {live.ended && !search ? " · 스트림 종료" : ""}
        </span>
        {!follow && !search && (
          <Button size="sm" onClick={() => setFollow(followState(false, { type: "follow" }))}>
            맨 아래로
          </Button>
        )}
      </div>
    </section>
  );
}
