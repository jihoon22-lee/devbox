/** 도구 목록: API 컬렉션 트리 · 웹훅 · 변환 도구 · 개발 환경 점검(01 §8.2). */
import type { CollectionRoot } from "../../rpc/gen/rpc";
import { Button } from "../../ui";
import s from "./api/Api.module.css";
import { CollectionTree } from "./api/CollectionTree";
import { EnvEditor } from "./api/EnvEditor";
import { useApiTabs } from "./api/store";
import type { ToolsSearch } from "./search";

/** 검색값 `root`(`personal` 또는 프로젝트 id) → 컬렉션 루트. */
export function rootOf(id: string | undefined): CollectionRoot {
  return id && id !== "personal" ? { kind: "project", projectId: id } : { kind: "personal" };
}

export function ToolsList({
  projectId,
  navigate,
}: {
  projectId: string | undefined;
  navigate: (search: ToolsSearch) => void;
}) {
  const personal: CollectionRoot = { kind: "personal" };
  const project: CollectionRoot | undefined = projectId ? { kind: "project", projectId } : undefined;
  const activeRoot = useApiTabs((state) => state.tabs.find((tab) => tab.key === state.active)?.root);
  const envRoot = activeRoot ?? project ?? personal;
  return (
    <div>
      <section className={s.group} aria-labelledby="tools-api">
        <h2 id="tools-api">API 컬렉션</h2>
        <div className={s.bar}>
          <Button
            size="sm"
            onClick={() => {
              useApiTabs.getState().openNew(project ?? personal);
              navigate({ view: "api" });
            }}
          >
            새 요청
          </Button>
          <Button size="sm" variant="ghost" onClick={() => navigate({ view: "api", tool: "import" })}>
            가져오기
          </Button>
        </div>
        <h3 className={s.muted}>개인</h3>
        <CollectionTree root={personal} label="개인 컬렉션" />
        <Button size="sm" variant="ghost" onClick={() => navigate({ view: "api", tool: "runner", root: "personal" })}>
          개인 컬렉션 실행
        </Button>
        {project && (
          <>
            <h3 className={s.muted}>프로젝트</h3>
            <CollectionTree root={project} label="프로젝트 컬렉션" />
            <Button
              size="sm"
              variant="ghost"
              onClick={() => navigate({ view: "api", tool: "runner", root: project.projectId })}
            >
              프로젝트 컬렉션 실행
            </Button>
          </>
        )}
        <details>
          <summary>환경</summary>
          {/* 편집 상태는 루트별로 EnvEditor store에 남으므로 루트가 바뀌어 다시 그려져도 잃지 않는다. */}
          <EnvEditor key={JSON.stringify(envRoot)} root={envRoot} />
        </details>
      </section>
      <section className={s.group} aria-labelledby="tools-webhooks">
        <h2 id="tools-webhooks">웹훅</h2>
        <Button size="sm" variant="ghost" onClick={() => navigate({ view: "webhooks" })}>
          웹훅 열기
        </Button>
      </section>
      <section className={s.group} aria-labelledby="tools-transforms">
        <h2 id="tools-transforms">변환 도구</h2>
        <Button size="sm" variant="ghost" onClick={() => navigate({ view: "transforms" })}>
          변환 도구 열기
        </Button>
      </section>
      <section className={s.group} aria-labelledby="tools-env">
        <h2 id="tools-env">개발 환경 점검</h2>
        <Button size="sm" variant="ghost" onClick={() => navigate({ view: "env" })}>
          점검 열기
        </Button>
      </section>
    </div>
  );
}
