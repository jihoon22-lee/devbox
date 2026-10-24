/**
 * 환경 고르기·편집(`env/<이름>.json`). 고른 환경의 변수는 보내기·실행기·코드 생성이 쓴다.
 * 편집 상태는 루트별로 화면 메모리(store)에 두어 다시 그려져도 남고, 저장하지 않은 변경이 있으면
 * 다른 환경을 고르거나 새 환경을 만들기 전에 묻는다.
 * 검토하지 않은 프로젝트 환경 파일은 [검토하고 사용]으로 검토를 기록하기 전까지 송신·실행에 쓰지 않는다
 * (값 전체가 `{{secret:X}}`인 변수는 데몬이 비밀로 풀므로 저장소 파일이 사용자 비밀을 끌어낼 수 있다).
 */
import { useLayoutEffect, useRef, useState } from "react";
import { create } from "zustand";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { Banner, Button, ConfirmDialog } from "../../../ui";
import s from "./Api.module.css";
import { SendFailure, useApiBackend } from "./backend";
import { type Row, row } from "./model/draft";
import { type EnvFile, envFilePath, isEnvPath, parseEnvFile, serializeEnvFile } from "./model/envFile";
import { ParamTable } from "./ParamTable";
import { DirtyMark } from "./RequestTabs";
import { rootKey, useApiTabs } from "./store";
import { bumpCollections, useCollections } from "./useCollections";

interface Editing {
  path: string | null;
  digest: string | null;
  name: string;
  rows: Row[];
  /** 읽거나 저장한 때의 내용(dirty 판정 기준). */
  baseline: string;
  /** 이 PC에서 검토하지 않은 프로젝트 환경 파일. 검토 확인 전에는 송신·실행에 쓰지 않는다. */
  unreviewed?: boolean;
}
interface RootDraft {
  editing?: Editing;
  notice?: string;
}

const useEnvDrafts = create<{ byRoot: Record<string, RootDraft> }>(() => ({ byRoot: {} }));
export const resetEnvDrafts = () => useEnvDrafts.setState({ byRoot: {} });

const snapshot = (name: string, rows: Row[]) => JSON.stringify([name, rows.map((r) => [r.enabled, r.name, r.value])]);
const isDirtyEnv = (editing: Editing | undefined) =>
  !!editing && snapshot(editing.name, editing.rows) !== editing.baseline;
const pairs = (rows: Row[]) => rows.filter((r) => r.enabled && r.name).map((r) => ({ name: r.name, value: r.value }));
const errorText = (error: unknown, fallback: string) =>
  error instanceof SendFailure ? (messageFor(error.code as never, error.detail)?.title ?? fallback) : fallback;

export function EnvEditor({ root }: { root: CollectionRoot }) {
  const backend = useApiBackend();
  const list = useCollections(root);
  const key = rootKey(root);
  const draft = useEnvDrafts((state) => state.byRoot[key]) ?? {};
  const { editing, notice } = draft;
  const [ask, setAsk] = useState<() => void>();
  const [saving, setSaving] = useState(false);
  const writing = useRef(false);
  const files = list.items.filter((item) => isEnvPath(item.path));
  const dirty = isDirtyEnv(editing);

  const patch = (next: RootDraft | ((current: RootDraft) => RootDraft)) =>
    useEnvDrafts.setState((state) => {
      const current = state.byRoot[key] ?? {};
      return {
        byRoot: { ...state.byRoot, [key]: typeof next === "function" ? next(current) : { ...current, ...next } },
      };
    });
  // 활성 환경은 지금 보이는 루트의 초안에서만 발행한다. 이전 루트의 비동기 작업은
  // 그 루트의 초안만 갱신하므로 현재 보내기·실행기·코드 생성 입력을 덮어쓰지 않는다.
  useLayoutEffect(() => {
    useApiTabs
      .getState()
      .setEnvironment(
        editing
          ? { name: editing.name, variables: pairs(editing.rows), unreviewed: editing.unreviewed === true }
          : null,
      );
  }, [editing]);
  const apply = (next: Editing | undefined) => patch({ editing: next });
  /** 저장하지 않은 변경이 있으면 먼저 묻는다. */
  const guarded = (action: () => void) => (dirty ? setAsk(() => action) : action());

  const load = async (path: string) => {
    patch({ notice: undefined });
    if (!path) return apply(undefined);
    try {
      const read = await backend.read(root, path);
      const file = parseEnvFile(read.text);
      const rows = file.variables.map((v) => row(v.name, v.value));
      const unreviewed = root.kind === "project" && !read.reviewed;
      apply({ path, digest: read.digest, name: file.name, rows, baseline: snapshot(file.name, rows), unreviewed });
    } catch (error) {
      patch({ notice: errorText(error, "환경 파일을 읽지 못했습니다.") });
    }
  };
  const confirmReview = async () => {
    if (!editing?.path || !editing.digest) return;
    const target = editing;
    try {
      if (root.kind === "project") await backend.review(root, target.path!, target.digest!);
    } catch (error) {
      patch({ notice: errorText(error, "검토를 기록하지 못했습니다.") });
      return;
    }
    const latest = useEnvDrafts.getState().byRoot[key]?.editing;
    if (latest && latest.path === target.path && latest.digest === target.digest)
      apply({ ...latest, unreviewed: false });
  };
  const createEnv = () =>
    apply({ path: null, digest: null, name: "새 환경", rows: [], baseline: snapshot("새 환경", []) });

  const save = async () => {
    if (!editing || writing.current) return;
    writing.current = true;
    setSaving(true);
    // 쓰는 내용을 지금 고정한다. 쓰는 동안 고친 행은 dirty로 남는다.
    const target = editing;
    const file: EnvFile = {
      version: 1,
      name: target.name,
      variables: target.rows.map((r) => ({ name: r.name, value: r.value })),
    };
    try {
      const { text, cleared } = serializeEnvFile(file);
      const path = target.path ?? envFilePath(target.name, new Set(list.items.map((item) => item.path.toLowerCase())));
      const expect = target.digest ? { kind: "digest" as const, digest: target.digest } : { kind: "absent" as const };
      const written = await backend.write(root, path, text, expect);
      if (root.kind === "project") await backend.review(root, path, written.digest);
      const latest = useEnvDrafts.getState().byRoot[key]?.editing;
      // 저장은 사용자가 본 내용을 쓴 것이고 프로젝트 루트는 그 digest로 검토를 기록했다.
      if (latest && latest.path === target.path)
        apply({
          ...latest,
          path,
          digest: written.digest,
          baseline: snapshot(target.name, target.rows),
          unreviewed: false,
        });
      patch({
        notice: cleared.length
          ? `비밀 값 ${cleared.length}개는 파일에 저장하지 않았습니다: ${cleared.join(", ")}. {{secret:이름}} 참조로 바꿔 주세요.`
          : "환경을 저장했습니다.",
      });
      bumpCollections();
    } catch (error) {
      patch({ notice: errorText(error, "환경 형식이 올바르지 않습니다.") });
    } finally {
      writing.current = false;
      setSaving(false);
    }
  };

  return (
    <div className={s.table}>
      <div className={s.bar}>
        <label className={s.bar}>
          환경
          <select
            className={s.select}
            value={editing?.path ?? ""}
            onChange={(event) => {
              const path = event.target.value;
              guarded(() => void load(path));
            }}
          >
            <option value="">환경 없음</option>
            {files.map((item) => (
              <option key={item.path} value={item.path}>
                {item.path.slice(4, -5)}
              </option>
            ))}
          </select>
        </label>
        {dirty && <DirtyMark />}
        <Button size="sm" variant="ghost" onClick={() => guarded(createEnv)}>
          새 환경
        </Button>
      </div>
      {editing?.unreviewed && (
        <Banner
          tone="warn"
          actions={
            <Button size="sm" onClick={() => void confirmReview()}>
              검토하고 사용
            </Button>
          }
        >
          이 PC에서 검토하지 않은 프로젝트 환경입니다. 값의 비밀 참조를 확인하기 전에는 보내기·실행에 쓰지 않습니다.
        </Banner>
      )}
      {editing && (
        <>
          {!editing.path && (
            <input
              className={s.input}
              aria-label="환경 이름"
              value={editing.name}
              onChange={(event) => apply({ ...editing, name: event.target.value })}
            />
          )}
          <ParamTable label="변수" rows={editing.rows} onChange={(rows) => apply({ ...editing, rows })} />
          <div>
            <Button size="sm" disabled={!editing.name.trim() || saving} onClick={() => void save()}>
              환경 저장
            </Button>
          </div>
        </>
      )}
      {notice && (
        <p role="status" className={s.muted}>
          {notice}
        </p>
      )}
      <ConfirmDialog
        open={!!ask}
        title="환경 변경을 버릴까요?"
        body="저장하지 않은 환경 변경은 사라집니다."
        confirmLabel="변경 버리기"
        onCancel={() => setAsk(undefined)}
        onConfirm={() => {
          const action = ask;
          setAsk(undefined);
          action?.();
        }}
      />
    </div>
  );
}
