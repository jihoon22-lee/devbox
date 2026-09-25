# P1-10 WSL helper 위치와 Markdown 미리보기 공용화 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** (1) `crates/knowledge-vault-engine`이 앱 폴더(`apps/devbox-workspace/native`)에 의존하는 거꾸로 된 방향을 바로잡는다: WSL helper crate를 `crates/wsl-helper`로 옮긴다. (2) Workspace·Knowledge에 따로 있는 Markdown+Mermaid 미리보기 렌더링을 `packages/markdown-view` 하나로 모은다(A4).

**Architecture:** helper는 디렉터리만 옮기고 패키지 이름(`workspace-wsl`)·lib 이름(`workspace_wsl`)·bin 이름(`devbox-workspace-wsl`)은 그대로 둔다. 그래서 CI·release의 `cargo build -p workspace-wsl`, artifact 스크립트, 제품 resources 경로는 바뀌지 않고 경로 참조만 바뀐다. 미리보기는 "살균된 HTML 넣기 + Mermaid 블록 렌더(블록별 마지막 성공 SVG 유지·오류 배지) + 링크 클릭 위임 + 제목 id 부여"를 `MarkdownBody` 컴포넌트와 `useMermaidBlocks` hook으로 만들고, 두 제품 컴포넌트는 자기 고유 동작(Knowledge 노트 링크 해석·wikilink, Workspace 단독 `.mmd` 미리보기)만 남긴다.

**Tech Stack:** Cargo, Python(CI scope), React 19, `@devbox/mermaid-renderer`, Vitest

**Spec:** `review.md` §6 A4

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- HTML은 계속 Rust `crates/markdown`(ammonia)이 살균한 값만 넣는다. `MarkdownBody`는 HTML을 가공하지 않는다. Mermaid는 공유 renderer의 strict 모드만 쓴다(기존 주석의 보안 전제 유지).
- 새 외부 의존성 없음.

## Review Focus

1. 옮긴 뒤 `cargo build -p workspace-wsl --target x86_64-unknown-linux-musl`와 artifact 스크립트가 같은 파일 이름·위치로 산출물을 만든다. (Task 1)
2. `resolve-ci-scope.py`가 새 경로 변경을 "helper 변경"으로 판정해 Knowledge·Workspace·vault engine을 함께 검증한다. (Task 1)
3. 문서를 바꾸는 사이 이전 문서의 Mermaid 렌더가 늦게 끝남 → 새 문서 DOM에 쓰지 않는다(`isConnected`·취소 플래그 유지). (Task 2)
4. Mermaid 문법 오류로 바뀐 블록 → 마지막 성공 SVG와 "⚠ 구문 오류" 배지가 함께 보인다(두 제품 기존 동작). (Task 2)
5. Knowledge 미리보기의 외부 링크·상대 링크·wikilink·앵커 이동이 옮긴 뒤에도 같다. (Task 2, 기존 테스트)

## Branch · PR

- 묶음: **B5** — 브랜치 `refactor/crates/shared-platform-crates`, PR 제목 `refactor(crates): share process-tree, DPAPI, WSL helper and Markdown preview`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(crates): move the WSL helper and share the Markdown preview`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: WSL helper를 `crates/wsl-helper`로

**Files:** `apps/devbox-workspace/native/` → `crates/wsl-helper/`, 루트 `Cargo.toml`, `crates/knowledge-vault-engine/Cargo.toml:25`, `apps/devbox-workspace/src-tauri/Cargo.toml:22`, `.github/scripts/{resolve-ci-scope.py,test-ci-scope.py}`, workflow paths

- [ ] **Step 1: 실패하는 테스트** — `.github/scripts/test-ci-scope.py:87-91`의 경로를 새 위치로 바꾼다(기대 패키지 목록은 그대로).

```python
native_helper = resolve("crates/wsl-helper/src/engine.rs")
assert native_helper.frontend_scope == "none"
assert native_helper.rust_packages == ["devbox-knowledge", "devbox-knowledge-vault-engine", "devbox-workspace", "workspace-wsl"]
helper_manifest = resolve("crates/wsl-helper/Cargo.toml")
assert helper_manifest.dependency_scope == "all"
```

  Run: `python3 .github/scripts/test-ci-scope.py` → FAIL(파일 없음).

- [ ] **Step 2: 옮기기**

```bash
git mv apps/devbox-workspace/native crates/wsl-helper
sed -i 's#"apps/devbox-workspace/native"#"crates/wsl-helper"#' Cargo.toml
sed -i 's#path = "../../apps/devbox-workspace/native"#path = "../wsl-helper"#' crates/knowledge-vault-engine/Cargo.toml
sed -i 's#workspace-wsl = { path = "../native"#workspace-wsl = { path = "../../../crates/wsl-helper"#' apps/devbox-workspace/src-tauri/Cargo.toml
```

  `crates/wsl-helper/Cargo.toml`과 소스에서 상대 경로(`path = "../../../crates/…"`, `include_str!("../…")`, 테스트 fixture 경로)를 `rg -n '\.\./' crates/wsl-helper`로 찾아 새 깊이에 맞게 고친다(앱 폴더 기준 `../../../crates/x` → crates 기준 `../x`).

- [ ] **Step 3: CI 규칙** — `resolve-ci-scope.py:403` 부근의 `parts[2] in {"src-tauri", "native"}` 규칙에서 `native`를 지우고, `crates/wsl-helper/`를 "Knowledge·Workspace가 같이 쓰는 helper" 소비자 규칙(현재 `apps/devbox-workspace/native`에 주던 것)과 같게 처리한다. `product-foundation.yml`의 `paths`에 `apps/devbox-workspace/native/**`가 있으면 `crates/wsl-helper/**`로 바꾼다. `check-source-cutover.py`의 앱 폴더 검사(`apps/`의 하위 폴더 = 네 제품)는 영향이 없다.

- [ ] **Step 4: 확인** — Run: `source ~/.cargo/env && python3 .github/scripts/test-ci-scope.py && cargo check --workspace --all-targets && cargo test -p workspace-wsl --lib && cargo build --locked --release -p workspace-wsl --target x86_64-unknown-linux-musl && ls -l target/x86_64-unknown-linux-musl/release/devbox-workspace-wsl` → PASS, 바이너리 존재.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(crates): move the WSL helper under crates"`

---

### Task 2: `packages/markdown-view`

**Files:** Create `packages/markdown-view/{package.json,tsconfig.json,vitest.config.ts,src/index.ts,src/MarkdownBody.tsx,src/useMermaidBlocks.ts,src/svgCache.ts,src/MarkdownBody.test.tsx}`; Modify `packages/workspace-features/src/files/components/PreviewPane.tsx`, `packages/knowledge-features/src/notes/components/MarkdownPreview.tsx`, 두 패키지 `package.json`

**Interfaces (Produces):**
- `applySvgResult(cache: Map<string, string>, key: string, result: {ok: true; svg: string} | {ok: false}): {svg: string; hasError: boolean}` (Workspace `lib/previewState.ts`에서 옮김)
- `useMermaidBlocks(container: RefObject<HTMLElement | null>, sources: readonly string[], docKey: string, version: unknown, idPrefix: string): void`
- `MarkdownBody(props: { html: string; mermaid: readonly string[]; docKey: string; idPrefix: string; className?: string; assignHeadingIds?: boolean; onClick?: (event: React.MouseEvent<HTMLDivElement>) => void; bodyRef?: RefObject<HTMLDivElement | null> })`

- [ ] **Step 1: 패키지 뼈대** — `packages/markdown-view/package.json`은 `packages/diff-view/package.json`을 복사해 `name`을 `@devbox/markdown-view`로, 의존성에 `"@devbox/mermaid-renderer": "workspace:*"`를 두고 `exports`를 `{ ".": "./src/index.ts" }`로 한다. `tsconfig.json`·`vitest.config.ts`도 `packages/diff-view`와 같게 만든다.

- [ ] **Step 2: 실패하는 테스트** — `src/MarkdownBody.test.tsx`

```tsx
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mermaid = vi.hoisted(() => ({ render: vi.fn() }));
vi.mock("@devbox/mermaid-renderer", () => ({ getMermaidRenderer: async () => mermaid }));
import { MarkdownBody } from "./MarkdownBody";

afterEach(() => { cleanup(); mermaid.render.mockReset(); });

const html = '<h2>Intro</h2><div class="mermaid-block" data-idx="0"></div><a href="notes/b.md">b</a>';

describe("MarkdownBody", () => {
  it("renders sanitized html and mermaid blocks", async () => {
    mermaid.render.mockResolvedValue({ svg: "<svg id='ok'></svg>" });
    const { container } = render(<MarkdownBody html={html} mermaid={["graph TD; A-->B"]} docKey="a.md" idPrefix="test" assignHeadingIds />);
    await waitFor(() => expect(container.querySelector("svg#ok")).not.toBeNull());
    expect(screen.getByRole("heading", { name: "Intro" }).id).toBe("intro");
  });

  it("keeps the last good diagram and shows an error badge on a broken edit", async () => {
    mermaid.render.mockResolvedValueOnce({ svg: "<svg id='good'></svg>" }).mockRejectedValueOnce(new Error("syntax"));
    const view = render(<MarkdownBody html={html} mermaid={["graph TD; A-->B"]} docKey="a.md" idPrefix="test" />);
    await waitFor(() => expect(view.container.querySelector("svg#good")).not.toBeNull());
    view.rerender(<MarkdownBody html={html} mermaid={["graph TD; A-->"]} docKey="a.md" idPrefix="test" />);
    await waitFor(() => expect(view.container.textContent).toContain("⚠ 구문 오류"));
    expect(view.container.querySelector("svg#good")).not.toBeNull();
  });

  it("does not write a late diagram into another document", async () => {
    let resolve!: (value: { svg: string }) => void;
    mermaid.render.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    const view = render(<MarkdownBody html={html} mermaid={["graph TD; A-->B"]} docKey="a.md" idPrefix="test" />);
    view.rerender(<MarkdownBody html="<p>other</p>" mermaid={[]} docKey="b.md" idPrefix="test" />);
    await act(async () => resolve({ svg: "<svg id='late'></svg>" }));
    expect(view.container.querySelector("svg#late")).toBeNull();
  });

  it("delegates clicks to the owner", () => {
    const onClick = vi.fn();
    render(<MarkdownBody html={html} mermaid={[]} docKey="a.md" idPrefix="test" onClick={onClick} />);
    act(() => screen.getByText("b").click());
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm install && pnpm --filter @devbox/markdown-view exec vitest run` → FAIL.

- [ ] **Step 4: 구현**

`src/svgCache.ts`: Workspace `packages/workspace-features/src/files/lib/previewState.ts`의 `PreviewSvgResult`·`applySvgResult`를 그대로 옮긴다(Workspace 쪽 파일은 이 패키지를 다시 export하거나 import를 바꾼다).

`src/useMermaidBlocks.ts`:

```ts
import { getMermaidRenderer } from "@devbox/mermaid-renderer";
import { type RefObject, useEffect, useRef } from "react";
import { applySvgResult } from "./svgCache";

const BADGE = '<span class="mermaid-error-badge" title="mermaid 구문 오류">⚠ 구문 오류</span>';

/** Render `.mermaid-block[data-idx]` placeholders inside `container`.
 * A block keeps its last successful SVG when a later edit fails. */
export function useMermaidBlocks(
  container: RefObject<HTMLElement | null>,
  sources: readonly string[],
  docKey: string,
  version: unknown,
  idPrefix: string,
): void {
  const lastGood = useRef(new Map<string, string>());
  const sequence = useRef(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new document starts a new cache.
  useEffect(() => { lastGood.current.clear(); }, [docKey]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` changes whenever the html is replaced.
  useEffect(() => {
    const root = container.current;
    if (!root) return;
    let cancelled = false;
    const blocks = Array.from(root.querySelectorAll<HTMLElement>(".mermaid-block[data-idx]"));
    if (blocks.length === 0) return;
    const canApply = (element: HTMLElement) => !cancelled && container.current === root && element.isConnected;
    const fail = (element: HTMLElement, key: string) => {
      if (!canApply(element)) return;
      element.innerHTML = `${applySvgResult(lastGood.current, key, { ok: false }).svg}${BADGE}`;
    };
    void (async () => {
      let renderer: Awaited<ReturnType<typeof getMermaidRenderer>>;
      try {
        renderer = await getMermaidRenderer();
      } catch {
        for (const element of blocks) if (sources[Number(element.dataset.idx)] !== undefined) fail(element, element.dataset.idx ?? "");
        return;
      }
      await Promise.all(blocks.map(async (element) => {
        const key = element.dataset.idx ?? "";
        const source = sources[Number(key)];
        if (source === undefined) return;
        try {
          const { svg } = await renderer.render(`${idPrefix}-${key}-${sequence.current++}`, source);
          if (!canApply(element)) return;
          element.innerHTML = applySvgResult(lastGood.current, key, { ok: true, svg }).svg;
        } catch {
          fail(element, key);
        }
      }));
    })();
    return () => { cancelled = true; };
  }, [container, sources, docKey, version, idPrefix]);
}
```

`src/MarkdownBody.tsx`:

```tsx
import { type MouseEvent, type RefObject, useEffect, useRef } from "react";
import { useMermaidBlocks } from "./useMermaidBlocks";

export interface MarkdownBodyProps {
  /** HTML already sanitized by the native markdown crate. */
  html: string;
  mermaid: readonly string[];
  docKey: string;
  idPrefix: string;
  className?: string;
  assignHeadingIds?: boolean;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
  bodyRef?: RefObject<HTMLDivElement | null>;
}

function assignIds(root: HTMLElement): void {
  const used = new Set(Array.from(root.querySelectorAll("[id]")).map((node) => node.id));
  for (const heading of root.querySelectorAll("h1,h2,h3,h4,h5,h6")) {
    if (heading.id) continue;
    const base = (heading.textContent ?? "").trim().toLowerCase().replace(/[^\p{L}\p{N}_\s-]/gu, "").replace(/\s/g, "-") || "section";
    let id = base;
    let index = 1;
    while (used.has(id)) id = `${base}-${index++}`;
    heading.id = id;
    used.add(id);
  }
}

export function MarkdownBody({ html, mermaid, docKey, idPrefix, className = "preview-body md-body", assignHeadingIds = false, onClick, bodyRef }: MarkdownBodyProps) {
  const ownRef = useRef<HTMLDivElement>(null);
  const ref = bodyRef ?? ownRef;
  useMermaidBlocks(ref, mermaid, docKey, html, idPrefix);
  useEffect(() => {
    if (assignHeadingIds && ref.current) assignIds(ref.current);
  }, [assignHeadingIds, html, ref]);
  // The native crate sanitized `html`; this is the only intentional HTML sink.
  return <div ref={ref} className={className} onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />;
}
```

`src/index.ts`: `export { MarkdownBody, type MarkdownBodyProps } from "./MarkdownBody"; export { useMermaidBlocks } from "./useMermaidBlocks"; export { applySvgResult, type PreviewSvgResult } from "./svgCache";`

- [ ] **Step 5: 소비자 교체**
  - Knowledge `MarkdownPreview.tsx`: Mermaid effect·제목 id effect·`lastGoodSvg`·`renderSeq`를 지우고 본문 `<div …dangerouslySetInnerHTML…/>`를 `<MarkdownBody html={doc.html} mermaid={doc.mermaid} docKey={baseRel} idPrefix="mermaid-preview" assignHeadingIds onClick={handleClick} bodyRef={containerRef} />`로 바꾼다. `resolveNoteLink`, `handleClick`, 앵커 스크롤 effect, 메타(title·tags)는 남긴다.
  - Workspace `PreviewPane.tsx`: `response.kind === "markdown"`이면 `<MarkdownBody html={response.html} mermaid={response.mermaid} docKey={docPath} idPrefix="code-pad-mermaid" className="preview-body" />`. 단독 Mermaid 파일(`response.kind === "mermaid"`)은 `html={'<div class="mermaid-block" data-idx="0"></div>'}`와 `useMemo(() => [response.source], [response.source])`로 만든 배열을 `mermaid`로 넘겨 같은 컴포넌트를 쓴다(렌더마다 새 배열을 만들면 effect가 매번 다시 돈다. 기존 "standalone" 키는 `"0"`이 된다).
  - 두 패키지 `package.json`에 `"@devbox/markdown-view": "workspace:*"`를 추가하고, Workspace `lib/previewState.ts`는 지우고 import를 새 패키지로 바꾼다.

- [ ] **Step 6: 통과 확인** — Run: `pnpm install && pnpm --filter @devbox/markdown-view --filter @devbox/workspace-features --filter @devbox/knowledge-features exec vitest run && pnpm --filter @devbox/markdown-view --filter @devbox/workspace-features --filter @devbox/knowledge-features exec tsc --noEmit && pnpm exec biome ci .` → PASS.

- [ ] **Step 7: 커밋** — `git add -A && git commit -m "refactor(frontend): share one Markdown and Mermaid preview body"`

---

### Task 3: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. `resolve-ci-scope.py` 변경이 있으므로 `pnpm verify:all`.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Knowledge 노트 미리보기(Mermaid 포함)·wikilink 이동, Workspace `.md`·`.mmd` 미리보기, WSL 프로젝트 파일 열기(helper 경로 변경 영향 확인).
