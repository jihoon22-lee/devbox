# S1 에이전트·터미널·프로젝트 (2/2: 화면·Windows 통합) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획 · 미착수 · 시작 조건: [04-s1-agents-terminal](04-s1-agents-terminal.md) 묶음 F–J push(같은 S1 브랜치에서 이어감)

**Goal:** S1 데몬 기능을 화면에 올린다.
- 터미널 섹션: 탭·분할·하단 패널·분리 창
- 에이전트 섹션: 긴급순 목록·상세 검토·지시 작성·격자 보기·알림 센터
- 프로젝트 전환기 카드와 시작 구성
- Windows 통합: 토스트·딥 링크·작업 표시줄 배지·트레이·전역 단축키·자동 시작

이것으로 S1 완료 조건(SC3·SC4)을 확인한다.

**Architecture:**
- 터미널 화면은 xterm.js가 `terminal.attach` 스트림을 읽고, write 콜백에서 신용을 돌려준다(04 Task 2의 `openStream`).
- 연결이 끊기면 입력을 버리고 덮개를 띄운다. 다시 연결되면 같은 세션에 다시 붙는다(tmux가 상태를 가짐).
- Tauri 전송은 `invoke` 순서를 보장하는 직렬 송신기를 쓴다(v0.9.0 `orderedInput` 이식, 01-design §9.1-11).
- 에이전트 화면은 `agents.*` 조회와 `agents.changed`·`agents.attention` 주제로 갱신한다.
- Windows 알림·배지는 메인 창의 화면 코드가 `agents.attention`을 받아 Tauri 명령으로 넘긴다. 메인 창은 닫아도 숨겨질 뿐이라 계속 받는다.

**Tech Stack:** S0b 화면 스택 + `@xterm/xterm` 6, `@xterm/addon-{fit,webgl,unicode11,search,web-links}`, `@tauri-apps/api` 2, `tauri-plugin-{deep-link,global-shortcut,autostart,opener}`(Tauri 알림 플러그인은 쓰지 않음, 01-design R12), WinRT `ToastNotification`(`windows` crate).

**Spec:** [01-design.md](01-design.md) §2.1·§5.5·§6.1·§6.5·§8(전체)·§9.1-11·§9.2, [04-s1-agents-terminal](04-s1-agents-terminal.md)의 메서드·주제

## Global Constraints

- S0b 화면 규칙을 그대로 따른다: 모든 서버 데이터는 `useRpcQuery`·`useRpcMutation`·`useTopicInvalidation`으로만 다룬다. 생성 파일 `app/src/rpc/gen/rpc.ts`는 손으로 고치지 않는다.
- 타입은 생성된 `Methods`에서 꺼낸다(예: `type AgentTask = Methods["agents.get"]["result"]`). 같은 모양을 손으로 다시 선언하지 않는다.
- 확인 정책은 `confirmPolicy(method)`로 정한다. `always`는 대상이 적힌 `ConfirmDialog`, 기본 포커스는 취소다.
- 문구는 한국어, 단축키는 `Ctrl`로 적는다. `docs/ui-terms.md`의 용어(에이전트 작업·작업 폴더·실행)를 쓴다. 금지어 검사(`scripts/banned-words.sh`)를 통과해야 한다.
- 터미널·편집기 포커스에서는 Ctrl+Shift·Ctrl+Alt·Ctrl+숫자·F키만 앱이 가져간다(`allowedWhileFocused`). 터미널 자체 키(복사·붙여넣기·글꼴 크기)는 xterm의 사용자 키 처리기에서 다룬다.
- 컴포넌트 300줄, 파일 600줄을 넘으면 나눈다.
- 분리 창은 같은 경로를 `/popout/...` 아래에서 프레임 없이 연다. Tauri 창 이름은 `popout-<종류>-<id>`(capabilities의 `popout-*`).
- 개발 빌드의 전역 단축키는 v0.9.0과 겹치지 않게 Ctrl+Alt+Shift 조합을 쓴다(01-design §8.5).

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| Tauri에서 빠르게 붙여넣기 + Enter | 순서가 바뀌지 않음. 전송 하나가 실패하면 뒤 입력은 버리고 "입력 전송 안 됨" 표시 | Task 14 |
| 데몬 재시작 중 터미널에 타자 | 입력은 버려지고 덮개가 뜸. 다시 연결되면 같은 세션 화면이 기록과 함께 돌아옴 | Task 14·16 |
| 한글 조합 중 Ctrl+Enter | 조합이 끝난 글자까지 포함해 한 번만 보냄 | Task 18 |
| 같은 에이전트의 입력 대기가 연달아 옴 | 토스트는 하나로 교체, 알림 센터에는 기록, 배지 수는 입력 대기 작업 수 | Task 19·22 |
| 병합하지 않은 커밋이 있는 작업 폐기 | 개수가 적힌 확인을 거쳐야만 폐기 | Task 18 |

---

## 작업 묶음

S1 전체가 브랜치 `v1/s1-agents-terminal` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s1-agents-terminal`다.
묶음은 그 안의 중간 지점이다. 묶음이 끝나면 로컬 검사(`pnpm check`, 그 시점에 없으면 있는 검사만) → `PROGRESS.md` 묶음 행 갱신 커밋 → push 한다. PR·CI는 없다. 세션 인계 지점이 된다.
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S1 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| K | Task 13–16 | push |
| L | Task 17–19 | push |
| M | Task 20–21 | push |
| N | Task 22–23 | push |
| 마무리 | Task 24 | **S1 PR 열기** → CI·설치 파일 → 사용자 확인 → rebase 머지 |

---

### Task 13: 디자인 시스템 보강·섹션 배치·하단 패널

**Files:**
- Create: `app/src/ui/{Tabs,SplitPane,Badge,Menu,Toast,TextArea,Select}.tsx`와 각 `.module.css`
- Create: `app/src/shell/SectionLayout.tsx`, `app/src/shell/BottomPanel.tsx`, `app/src/shell/layout.ts`
- Modify: `app/src/ui/index.ts`, `app/src/shell/Frame.tsx`·`Frame.module.css`, `app/src/shell/router.tsx`, `app/src/App.tsx`
- Test: `app/src/ui/Toast.test.tsx`, `app/src/shell/layout.test.ts`

**Interfaces:**
- Consumes: S0b `Button`·`IconButton`·`Kbd`·`Tooltip`, `useKeymap`, `buildRouter`
- Produces:
  - `Tabs({ items: { id, label, badge?, closable? }[], active, onSelect, onClose?, actions? })`
  - `SplitPane({ dir: "row"|"column", ratio, onRatio, min?: number, children: [a, b] })`(끌어서 크기 조절, 키보드 ←/→ 5%)
  - `Badge({ tone: "neutral"|"warn"|"danger"|"info", children })`
  - `Menu({ trigger, items: { label, onSelect, danger?, disabled?, shortcut? }[] })`(Radix DropdownMenu)
  - `TextArea`(자동 높이, 최대 행 수 `maxRows`)
  - `Select({ value, onChange, options: { value, label }[], label })`(Radix Select)
  - `useToasts`(Zustand): `push({ tone, title, body?, undo?: () => void })`(되돌리기 있으면 10초, 없으면 5초), `<ToastHost/>`
  - `useLayout`(Zustand, `localStorage["devbox.layout"]`): `{ listWidth, listCollapsed, bottomOpen, bottomHeight, toggleList(), toggleBottom(), setListWidth(), setBottomHeight() }`
  - `autoCollapse(width, height) -> { list: boolean, bottom: boolean }`(1100px 미만 목록, 760px 미만 하단 패널)
  - `SectionLayout({ list, children })`: 목록 영역(기본 260px, 크기 조절) + 본문
  - `BottomPanel`: 탭 `터미널`만(S2가 로그·문제를 더함). 내용은 등록식 `registerBottomTab({ id, label, render })`
  - `buildRouter({ frame, pages, settings, popouts })`: 섹션별 화면을 받는다. `/popout/*` 경로는 프레임 없이 그린다.
  - 단축키: Ctrl+Shift+B(목록 접기), Ctrl+\`(하단 패널)

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/shell/layout.test.ts
import { autoCollapse, useLayout } from "./layout";

test("narrow or short windows collapse the list and the bottom panel", () => {
  expect(autoCollapse(1280, 800)).toEqual({ list: false, bottom: false });
  expect(autoCollapse(1000, 800)).toEqual({ list: true, bottom: false });
  expect(autoCollapse(1280, 700)).toEqual({ list: false, bottom: true });
});

test("toggles persist in the layout store", () => {
  useLayout.getState().toggleBottom();
  expect(useLayout.getState().bottomOpen).toBe(true);
  useLayout.getState().toggleList();
  expect(useLayout.getState().listCollapsed).toBe(true);
  expect(JSON.parse(localStorage.getItem("devbox.layout") ?? "{}").state.bottomOpen).toBe(true);
});
```

```tsx
// app/src/ui/Toast.test.tsx
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ToastHost, useToasts } from "./Toast";

test("undo toasts stay 10 seconds and run undo once", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const undo = vi.fn();
  render(<ToastHost />);
  act(() => useToasts.getState().push({ tone: "info", title: "서비스를 중지했습니다", undo }));
  await userEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  expect(undo).toHaveBeenCalledOnce();
  expect(screen.queryByText("서비스를 중지했습니다")).toBeNull();
  act(() => useToasts.getState().push({ tone: "info", title: "알림" }));
  act(() => vi.advanceTimersByTime(5_100));
  expect(screen.queryByText("알림")).toBeNull();
  vi.useRealTimers();
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/shell/layout.test.ts src/ui/Toast.test.tsx`

- [ ] **Step 3: 구현 — 배치 상태와 토스트**

```ts
// app/src/shell/layout.ts
import { create } from "zustand";
import { persist } from "zustand/middleware";

interface Layout {
  listWidth: number;
  listCollapsed: boolean;
  bottomOpen: boolean;
  bottomHeight: number;
  toggleList(): void;
  toggleBottom(): void;
  setListWidth(px: number): void;
  setBottomHeight(px: number): void;
}

export const useLayout = create<Layout>()(
  persist(
    (set) => ({
      listWidth: 260,
      listCollapsed: false,
      bottomOpen: false,
      bottomHeight: 260,
      toggleList: () => set((s) => ({ listCollapsed: !s.listCollapsed })),
      toggleBottom: () => set((s) => ({ bottomOpen: !s.bottomOpen })),
      setListWidth: (px) => set({ listWidth: Math.min(480, Math.max(180, px)) }),
      setBottomHeight: (px) => set({ bottomHeight: Math.min(600, Math.max(120, px)) }),
    }),
    { name: "devbox.layout" },
  ),
);

export function autoCollapse(width: number, height: number) {
  return { list: width < 1100, bottom: height < 760 };
}
```

```tsx
// app/src/ui/Toast.tsx
import { create } from "zustand";
import { Button } from "./Button";
import s from "./Toast.module.css";

export interface ToastInput {
  tone: "info" | "warn" | "danger" | "ok";
  title: string;
  body?: string;
  undo?: () => void;
}
interface Toast extends ToastInput {
  id: number;
}
interface Toasts {
  items: Toast[];
  push(t: ToastInput): number;
  dismiss(id: number): void;
}

let next = 1;
export const useToasts = create<Toasts>((set, get) => ({
  items: [],
  push: (t) => {
    const id = next++;
    set({ items: [...get().items.slice(-4), { ...t, id }] });
    setTimeout(() => get().dismiss(id), t.undo ? 10_000 : 5_000);
    return id;
  },
  dismiss: (id) => set({ items: get().items.filter((x) => x.id !== id) }),
}));

export function ToastHost() {
  const { items, dismiss } = useToasts();
  return (
    <div className={s.host} role="region" aria-label="알림">
      {items.map((t) => (
        <div key={t.id} className={s.toast} data-tone={t.tone} role="status">
          <div className={s.text}>
            <strong>{t.title}</strong>
            {t.body && <span>{t.body}</span>}
          </div>
          {t.undo && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                t.undo?.();
                dismiss(t.id);
              }}
            >
              되돌리기
            </Button>
          )}
        </div>
      ))}
    </div>
  );
}
```

```css
/* app/src/ui/Toast.module.css */
.host { position: fixed; right: var(--space-4); bottom: calc(var(--statusbar) + var(--space-3)); display: flex; flex-direction: column; gap: var(--space-2); z-index: 50; }
.toast { display: flex; align-items: center; gap: var(--space-3); min-width: 280px; max-width: 420px; padding: var(--space-2) var(--space-3); background: var(--surface); border: 1px solid var(--border); border-left: 3px solid var(--info); border-radius: var(--radius-md); box-shadow: var(--shadow-2); }
.toast[data-tone="warn"] { border-left-color: var(--warn); }
.toast[data-tone="danger"] { border-left-color: var(--danger); }
.toast[data-tone="ok"] { border-left-color: var(--ok); }
.text { display: flex; flex-direction: column; gap: 2px; flex: 1; font-size: var(--text-sm); }
.text span { color: var(--fg-muted); }
@media (prefers-reduced-motion: no-preference) { .toast { animation: in 120ms ease-out; } }
@keyframes in { from { transform: translateY(6px); opacity: 0; } }
```

- [ ] **Step 4: 구현 — 나머지 컴포넌트**

```tsx
// app/src/ui/SplitPane.tsx
import { type ReactNode, useRef } from "react";
import s from "./SplitPane.module.css";

export function SplitPane({ dir, ratio, onRatio, min = 0.15, children }: { dir: "row" | "column"; ratio: number; onRatio: (r: number) => void; min?: number; children: [ReactNode, ReactNode] }) {
  const box = useRef<HTMLDivElement>(null);
  const clamp = (r: number) => Math.min(1 - min, Math.max(min, r));
  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return;
    const move = (ev: PointerEvent) => onRatio(clamp(dir === "row" ? (ev.clientX - rect.left) / rect.width : (ev.clientY - rect.top) / rect.height));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  const step = (d: number) => onRatio(clamp(ratio + d));
  return (
    <div ref={box} className={s.split} data-dir={dir} style={{ ["--ratio" as string]: `${ratio * 100}%` }}>
      <div className={s.a}>{children[0]}</div>
      {/* biome-ignore lint/a11y/useSemanticElements: separator is the ARIA pattern for resizable splits */}
      <div
        role="separator"
        aria-orientation={dir === "row" ? "vertical" : "horizontal"}
        aria-valuenow={Math.round(ratio * 100)}
        tabIndex={0}
        className={s.handle}
        onPointerDown={onPointerDown}
        onKeyDown={(e) => {
          if (e.key === (dir === "row" ? "ArrowLeft" : "ArrowUp")) step(-0.05);
          if (e.key === (dir === "row" ? "ArrowRight" : "ArrowDown")) step(0.05);
        }}
      />
      <div className={s.b}>{children[1]}</div>
    </div>
  );
}
```

```css
/* app/src/ui/SplitPane.module.css */
.split { display: grid; height: 100%; width: 100%; min-height: 0; min-width: 0; }
.split[data-dir="row"] { grid-template-columns: var(--ratio) 4px 1fr; }
.split[data-dir="column"] { grid-template-rows: var(--ratio) 4px 1fr; }
.a, .b { min-width: 0; min-height: 0; overflow: hidden; }
.handle { background: var(--border); cursor: col-resize; }
.split[data-dir="column"] > .handle { cursor: row-resize; }
.handle:hover, .handle:focus-visible { background: var(--accent); outline: none; }
```

```tsx
// app/src/ui/Tabs.tsx
import { X } from "lucide-react";
import type { ReactNode } from "react";
import s from "./Tabs.module.css";

export interface TabItem {
  id: string;
  label: string;
  badge?: ReactNode;
  closable?: boolean;
}

export function Tabs({ items, active, onSelect, onClose, actions }: { items: TabItem[]; active?: string; onSelect: (id: string) => void; onClose?: (id: string) => void; actions?: ReactNode }) {
  return (
    <div className={s.bar}>
      <div role="tablist" className={s.list}>
        {items.map((t) => (
          <div key={t.id} className={s.tab} data-active={t.id === active || undefined}>
            <button type="button" role="tab" aria-selected={t.id === active} onClick={() => onSelect(t.id)} onAuxClick={(e) => e.button === 1 && t.closable && onClose?.(t.id)}>
              {t.label}
              {t.badge}
            </button>
            {t.closable && onClose && (
              <button type="button" className={s.close} aria-label={`${t.label} 닫기`} onClick={() => onClose(t.id)}>
                <X size={12} />
              </button>
            )}
          </div>
        ))}
      </div>
      {actions && <div className={s.actions}>{actions}</div>}
    </div>
  );
}
```

```tsx
// app/src/ui/Menu.tsx
import * as M from "@radix-ui/react-dropdown-menu";
import type { ReactNode } from "react";
import { Kbd } from "./Kbd";
import s from "./Menu.module.css";

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  shortcut?: string[];
}

export function Menu({ trigger, items }: { trigger: ReactNode; items: MenuItem[] }) {
  return (
    <M.Root>
      <M.Trigger asChild>{trigger}</M.Trigger>
      <M.Portal>
        <M.Content className={s.content} sideOffset={4} align="end">
          {items.map((i) => (
            <M.Item key={i.label} className={s.item} data-danger={i.danger || undefined} disabled={i.disabled} onSelect={i.onSelect}>
              <span>{i.label}</span>
              {i.shortcut && <Kbd keys={i.shortcut} />}
            </M.Item>
          ))}
        </M.Content>
      </M.Portal>
    </M.Root>
  );
}
```

```tsx
// app/src/ui/TextArea.tsx
import { forwardRef, type TextareaHTMLAttributes, useLayoutEffect, useRef } from "react";
import s from "./TextArea.module.css";

export const TextArea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement> & { maxRows?: number }>(function TextArea({ maxRows = 12, className, ...rest }, ref) {
  const inner = useRef<HTMLTextAreaElement | null>(null);
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    el.style.height = "auto";
    const line = Number.parseFloat(getComputedStyle(el).lineHeight) || 20;
    el.style.height = `${Math.min(el.scrollHeight, line * maxRows + 12)}px`;
  });
  return (
    <textarea
      {...rest}
      ref={(el) => {
        inner.current = el;
        if (typeof ref === "function") ref(el);
        else if (ref) ref.current = el;
      }}
      className={`${s.area} ${className ?? ""}`}
    />
  );
});
```

`Badge`는 `<span className={s.badge} data-tone={tone}>`, `Select`는 Radix Select에 `aria-label={label}`을 붙인 얇은 감싸개다. 둘 다 Tabs와 같은 방식으로 토큰 색만 쓴다.

- [ ] **Step 5: 구현 — 섹션 배치·하단 패널·라우터**

```tsx
// app/src/shell/SectionLayout.tsx
import type { ReactNode } from "react";
import { useLayout } from "./layout";
import s from "./SectionLayout.module.css";

export function SectionLayout({ list, children }: { list: ReactNode; children: ReactNode }) {
  const { listWidth, listCollapsed, setListWidth } = useLayout();
  const onPointerDown = (e: React.PointerEvent) => {
    const startX = e.clientX;
    const start = listWidth;
    const move = (ev: PointerEvent) => setListWidth(start + ev.clientX - startX);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return (
    <div className={s.layout} style={{ gridTemplateColumns: listCollapsed ? "0 0 1fr" : `${listWidth}px 4px 1fr` }}>
      <aside className={s.list} aria-hidden={listCollapsed}>{list}</aside>
      {/* biome-ignore lint/a11y/useSemanticElements: resizable list edge */}
      <div role="separator" aria-orientation="vertical" className={s.handle} onPointerDown={onPointerDown} />
      <section className={s.body}>{children}</section>
    </div>
  );
}
```

```css
/* app/src/shell/SectionLayout.module.css */
.layout { display: grid; height: 100%; min-height: 0; }
.list { min-width: 0; overflow: auto; background: var(--bg-0); border-right: 1px solid var(--border); }
.list[aria-hidden="true"] { visibility: hidden; }
.handle { cursor: col-resize; }
.handle:hover { background: var(--accent); }
.body { min-width: 0; min-height: 0; overflow: hidden; }
```

```tsx
// app/src/shell/BottomPanel.tsx
import { type ReactNode, useState } from "react";
import { create } from "zustand";
import { Tabs } from "../ui";
import { useLayout } from "./layout";

interface BottomTab {
  id: string;
  label: string;
  render: () => ReactNode;
}
export const useBottomTabs = create<{ tabs: BottomTab[]; register(t: BottomTab): () => void }>((set, get) => ({
  tabs: [],
  register: (t) => {
    set({ tabs: [...get().tabs.filter((x) => x.id !== t.id), t] });
    return () => set({ tabs: get().tabs.filter((x) => x.id !== t.id) });
  },
}));

export function BottomPanel() {
  const { bottomOpen, bottomHeight } = useLayout();
  const tabs = useBottomTabs((s) => s.tabs);
  const [active, setActive] = useState<string>();
  if (!bottomOpen || tabs.length === 0) return null;
  const current = tabs.find((t) => t.id === active) ?? tabs[0];
  return (
    <div style={{ height: bottomHeight, display: "flex", flexDirection: "column", borderTop: "1px solid var(--border)", background: "var(--bg-1)" }}>
      <Tabs items={tabs.map((t) => ({ id: t.id, label: t.label }))} active={current.id} onSelect={setActive} />
      <div style={{ flex: 1, minHeight: 0 }}>{current.render()}</div>
    </div>
  );
}
```

`Frame.tsx`의 `main`을 `<ConnectionBanner/>` + `<div className={s.content}><Outlet/></div>` + `<BottomPanel/>` + `<ToastHost/>`로 바꾼다. `ResizeObserver`로 창 크기를 보고 `autoCollapse` 결과가 참이면 그 순간의 목록·하단 패널을 접는다(사용자가 다시 열면 그 상태를 따른다).

```tsx
// app/src/shell/router.tsx (교체)
import { createRootRoute, createRoute, createRouter, Outlet, redirect, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { EmptyState } from "../ui";
import { SECTIONS } from "./sections";

export interface RouterParts {
  frame: () => ReactNode;
  pages: Partial<Record<string, { component: () => ReactNode; validateSearch?: (s: Record<string, unknown>) => Record<string, unknown> }>>;
  settings: () => ReactNode;
  popouts: { path: string; component: () => ReactNode }[];
}

function Root({ frame }: { frame: () => ReactNode }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  return path.startsWith("/popout/") ? <Outlet /> : frame();
}

export function buildRouter(parts: RouterParts) {
  const rootRoute = createRootRoute({ component: () => <Root frame={parts.frame} /> });
  const index = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    beforeLoad: () => {
      throw redirect({ to: localStorage.getItem("devbox.lastSection") ?? "/agents" });
    },
  });
  const sectionRoutes = SECTIONS.map((s) => {
    const page = parts.pages[s.path];
    return createRoute({
      getParentRoute: () => rootRoute,
      path: s.path,
      validateSearch: page?.validateSearch,
      onEnter: () => localStorage.setItem("devbox.lastSection", s.path),
      component: page?.component ?? (() => <EmptyState title={s.label} body="이 화면은 다음 하위 프로젝트에서 채워집니다." />),
    });
  });
  const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: parts.settings });
  const popoutRoutes = parts.popouts.map((p) => createRoute({ getParentRoute: () => rootRoute, path: p.path, component: p.component }));
  return createRouter({ routeTree: rootRoute.addChildren([index, ...sectionRoutes, settingsRoute, ...popoutRoutes]) });
}
```

`App.tsx`의 `buildRouter` 호출을 `buildRouter({ frame, pages: {}, settings, popouts: [] })`로 바꾸고, 바인딩에 다음을 추가한다.
- `{ id: "layout.list", code: "KeyB", ctrl: true, shift: true, scope: "app", label: "목록 접기", run: () => useLayout.getState().toggleList() }`
- `{ id: "layout.bottom", code: "Backquote", ctrl: true, scope: "app", label: "하단 패널", run: () => useLayout.getState().toggleBottom() }`

바인딩을 하나 더 둔다.
- `{ id: "ui.zoomIn", code: "Equal", ctrl: true, scope: "app", … }`·`ui.zoomOut`(Minus)·`ui.zoomReset`(Digit0): 터미널 밖에서 UI 배율(`document.documentElement.style.zoom`, 90–150%, `localStorage["devbox.uiScale"]`)을 바꾼다. 터미널 포커스에서는 TerminalView가 글꼴 크기로 처리한다(Task 15).
- `{ id: "toast.undo", code: "KeyZ", ctrl: true, scope: "app", … }`: 입력란·편집기·터미널 밖에서 가장 최근의 되돌리기 토스트를 실행한다(10초 안, 01-design §8.5). 입력란 안에서는 `allowedWhileFocused`와 별개로 `document.activeElement`가 입력 요소면 건너뛴다.
- 마우스 4·5번 버튼(`mouseup`의 `button === 3 | 4`)은 뒤로·앞으로다(01-design §8.1).

Ctrl+\`는 Ctrl+문자 규칙상 터미널 포커스에서는 앱이 가져가지 않는다. 터미널 안에서는 TerminalView가 같은 키를 직접 처리한다(Task 15).

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/ui src/shell && pnpm --filter app typecheck
git add app/src && git commit -m "feat(app): add tabs, split panes, menus, toasts, section layout and the bottom panel"
```

---

### Task 14: 순서 보장 전송과 터미널 연결 컨트롤러

**Files:**
- Create: `app/src/rpc/orderedSender.ts`, `app/src/features/terminal/link.ts`
- Modify: `app/src/rpc/transport-tauri.ts`
- Test: `app/src/rpc/orderedSender.test.ts`, `app/src/features/terminal/link.test.ts`

**Interfaces:**
- Consumes: 04 Task 2 `RpcClient.openStream`, S0b `RpcClient.onState`·`call`
- Produces:
  - `createOrderedSender(onFailure) -> { enqueue(bytes, send: () => Promise<void>), dispose() }`
    - v0.9.0 `packages/workspace-features/src/terminal/lib/orderedInput.ts`를 이식한다. 한 번에 하나만 보내고 FIFO다. 실패를 재시도하지 않는다.
    - 상한: 대기 256개, 대기 바이트 8 MiB
  - `tauriTransport`가 모든 `rpc_send`를 이 송신기로 보낸다. 실패하면 `"closed"` 상태를 알리고 송신기를 새로 만든다.
  - `TerminalSink { write(data: Uint8Array, done: () => void): void; reset(): void }`
  - `type LinkState = "connecting" | "live" | "lost" | "ended"`
  - `class TerminalLink`
    - 생성자: `(client, { id, readOnly, sink, onState(s), onInputDropped() })`
    - `open(cols, rows)`: `terminal.attach` 스트림을 연다. 받은 데이터를 `sink.write(data, ack)`로 넘긴다.
    - `input(text)`: `live`가 아니면 버리고, 끊긴 구간마다 `onInputDropped`를 한 번 부른다.
    - `resize(cols, rows)`: 100ms로 묶어 `terminal.resize`를 보낸다.
    - 클라이언트가 `ready`로 돌아오면 `sink.reset()` 뒤 다시 연다.
    - 세션이 사라지면(`terminal.not_found`) `ended`.
    - `dispose()`

- [ ] **Step 1: 실패하는 테스트 — 순서 송신(이식)**

```ts
// app/src/rpc/orderedSender.test.ts — v0.9.0 orderedInput.test.ts에서 이식
import { createOrderedSender } from "./orderedSender";

function deferred() {
  let resolve!: () => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<void>((y, n) => {
    resolve = y;
    reject = n;
  });
  return { promise, resolve, reject };
}
const bytes = (n: number) => new Uint8Array(n);

test("sends one at a time in order", async () => {
  const s = createOrderedSender(vi.fn());
  const first = deferred();
  const second = vi.fn().mockResolvedValue(undefined);
  s.enqueue(bytes(1), () => first.promise);
  s.enqueue(bytes(1), second);
  expect(second).not.toHaveBeenCalled();
  first.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(second).toHaveBeenCalledOnce();
});

test("drops queued writes after an ambiguous failure and never retries", async () => {
  const failure = vi.fn();
  const s = createOrderedSender(failure);
  const first = deferred();
  const send = vi.fn(() => first.promise);
  const enter = vi.fn().mockResolvedValue(undefined);
  s.enqueue(bytes(7), send);
  s.enqueue(bytes(1), enter);
  first.reject(new Error("unknown delivery"));
  await Promise.resolve();
  s.enqueue(bytes(1), enter);
  expect(send).toHaveBeenCalledTimes(1);
  expect(enter).not.toHaveBeenCalled();
  expect(failure).toHaveBeenCalledTimes(1);
});

test("bounds pending frames and bytes while a send stalls", () => {
  const failure = vi.fn();
  const s = createOrderedSender(failure);
  const stall = deferred();
  s.enqueue(bytes(1), () => stall.promise);
  for (let i = 0; i < 255; i++) s.enqueue(bytes(1), vi.fn());
  expect(failure).not.toHaveBeenCalled();
  s.enqueue(bytes(1), vi.fn());
  expect(failure).toHaveBeenCalledOnce();

  const big = createOrderedSender(failure);
  big.enqueue(bytes(1), () => deferred().promise);
  big.enqueue(bytes(8 * 1024 * 1024), vi.fn());
  expect(failure).toHaveBeenCalledTimes(2);
});

test("disposal discards waiting writes and ignores late completion", async () => {
  const failure = vi.fn();
  const s = createOrderedSender(failure);
  const first = deferred();
  const later = vi.fn().mockResolvedValue(undefined);
  s.enqueue(bytes(1), () => first.promise);
  s.enqueue(bytes(1), later);
  s.dispose();
  first.reject(new Error("closed"));
  await Promise.resolve();
  expect(later).not.toHaveBeenCalled();
  expect(failure).not.toHaveBeenCalled();
});
```

- [ ] **Step 2: 실패하는 테스트 — 터미널 연결**

```ts
// app/src/features/terminal/link.test.ts
import type { ClientState, RpcClient } from "../../rpc/client";
import { RpcError } from "../../rpc/client";
import { type LinkState, TerminalLink } from "./link";

type Handlers = { onData: (b: Uint8Array, ack: () => void) => void; onEnd: () => void };

function fakeClient() {
  const stateCbs = new Set<(s: ClientState) => void>();
  const opened: { handlers: Handlers; written: Uint8Array[] }[] = [];
  const calls: [string, unknown][] = [];
  let attachError: RpcError | undefined;
  const client = {
    onState: (cb: (s: ClientState) => void) => {
      stateCbs.add(cb);
      return () => stateCbs.delete(cb);
    },
    call: vi.fn((m: string, p: unknown) => {
      calls.push([m, p]);
      return Promise.resolve({ rev: "1" });
    }),
    openStream: vi.fn((_m: string, _p: unknown, handlers: Handlers) => {
      if (attachError) return Promise.reject(attachError);
      const entry = { handlers, written: [] as Uint8Array[] };
      opened.push(entry);
      return Promise.resolve({ result: { streamId: opened.length }, streamId: opened.length, write: (b: Uint8Array) => entry.written.push(b), end: () => {} });
    }),
  };
  return {
    client: client as unknown as RpcClient,
    opened,
    calls,
    setState: (s: ClientState) => stateCbs.forEach((cb) => cb(s)),
    failAttach: (e: RpcError) => {
      attachError = e;
    },
  };
}

function sink() {
  const writes: string[] = [];
  let resets = 0;
  const acks: (() => void)[] = [];
  return {
    sink: { write: (d: Uint8Array, done: () => void) => (writes.push(new TextDecoder().decode(d)), acks.push(done)), reset: () => void resets++ },
    writes,
    acks,
    resets: () => resets,
  };
}

test("writes output to the sink, sends input and debounces resize", async () => {
  vi.useFakeTimers();
  const f = fakeClient();
  const s = sink();
  const states: LinkState[] = [];
  const link = new TerminalLink(f.client, { id: "t1", readOnly: false, sink: s.sink, onState: (x) => states.push(x), onInputDropped: vi.fn() });
  await link.open(80, 24);
  expect(states).toEqual(["connecting", "live"]);
  const ack = vi.fn();
  f.opened[0].handlers.onData(new TextEncoder().encode("hi"), ack);
  expect(s.writes).toEqual(["hi"]);
  s.acks[0]();
  expect(ack).toHaveBeenCalledOnce();
  link.input("ls\r");
  expect(new TextDecoder().decode(f.opened[0].written[0])).toBe("ls\r");
  link.resize(100, 30);
  link.resize(120, 40);
  vi.advanceTimersByTime(120);
  expect(f.calls.filter(([m]) => m === "terminal.resize")).toEqual([["terminal.resize", { streamId: 1, cols: 120, rows: 40 }]]);
  vi.useRealTimers();
});

test("drops input while disconnected, reports once, and reattaches on ready", async () => {
  const f = fakeClient();
  const s = sink();
  const dropped = vi.fn();
  const states: LinkState[] = [];
  const link = new TerminalLink(f.client, { id: "t1", readOnly: false, sink: s.sink, onState: (x) => states.push(x), onInputDropped: dropped });
  await link.open(80, 24);
  f.setState("reconnecting");
  f.opened[0].handlers.onEnd();
  link.input("a");
  link.input("b");
  expect(dropped).toHaveBeenCalledOnce();
  expect(f.opened[0].written).toHaveLength(0);
  f.setState("ready");
  await vi.waitFor(() => expect(f.opened).toHaveLength(2));
  expect(s.resets()).toBe(1);
  expect(states.at(-1)).toBe("live");
});

test("read-only links never send input and a missing session ends the link", async () => {
  const f = fakeClient();
  const s = sink();
  const states: LinkState[] = [];
  const ro = new TerminalLink(f.client, { id: "t1", readOnly: true, sink: s.sink, onState: () => {}, onInputDropped: vi.fn() });
  await ro.open(80, 24);
  ro.input("x");
  expect(f.opened[0].written).toHaveLength(0);

  f.failAttach(new RpcError("terminal.not_found", { id: "t2" }));
  const gone = new TerminalLink(f.client, { id: "t2", readOnly: false, sink: s.sink, onState: (x) => states.push(x), onInputDropped: vi.fn() });
  await gone.open(80, 24);
  expect(states.at(-1)).toBe("ended");
});
```

`RpcError` 생성자 모양(`new RpcError(code, detail?)`)은 S0b Task 17을 따른다. 다르면 테스트를 그 모양에 맞춘다.

- [ ] **Step 3: 실패 확인** — `pnpm --filter app exec vitest run src/rpc/orderedSender.test.ts src/features/terminal/link.test.ts`

- [ ] **Step 4: 구현**

```ts
// app/src/rpc/orderedSender.ts
// v0.9.0 packages/workspace-features/src/terminal/lib/orderedInput.ts 이식.
// Tauri invoke는 서로 순서를 보장하지 않으므로, 앞 전송이 끝난 뒤에 다음을 보낸다.
const MAX_PENDING = 256;
const MAX_PENDING_BYTES = 8 * 1024 * 1024;

interface Write {
  bytes: number;
  send: () => Promise<void>;
}

export function createOrderedSender(onFailure: () => void) {
  const pending: Write[] = [];
  let active = false;
  let stopped = false;
  let bytes = 0;
  const dispose = () => {
    stopped = true;
    pending.length = 0;
    bytes = 0;
  };
  const fail = () => {
    if (stopped) return;
    dispose();
    onFailure();
  };
  const drain = () => {
    if (active || stopped) return;
    const next = pending.shift();
    if (!next) return;
    active = true;
    void (async () => {
      try {
        await next.send();
      } catch {
        fail();
        return;
      }
      if (stopped) return;
      bytes -= next.bytes;
      active = false;
      drain();
    })();
  };
  return {
    enqueue(data: Uint8Array, send: () => Promise<void>) {
      if (stopped) return;
      if (pending.length + Number(active) >= MAX_PENDING || bytes + data.length > MAX_PENDING_BYTES) {
        fail();
        return;
      }
      bytes += data.length;
      pending.push({ bytes: data.length, send });
      drain();
    },
    dispose,
  };
}
```

```ts
// app/src/rpc/transport-tauri.ts 의 send·connect 수정
// 필드: let sender = createOrderedSender(onSendFailure);
// const onSendFailure = () => { stateCbs.forEach((cb) => cb("closed")); sender = createOrderedSender(onSendFailure); };
send(frame) {
  sender.enqueue(frame, () => invoke("rpc_send", frame));
},
```

`connect()`가 다시 불릴 때도 `sender.dispose()` 뒤 새로 만든다. `"closed"`를 받은 `RpcClient`는 S0b 규칙대로 재연결한다(Tauri 쪽 `supervise`가 브리지를 유지하므로 `rpc_attach`만 다시 한다).

```ts
// app/src/features/terminal/link.ts
import type { RpcClient } from "../../rpc/client";
import { RpcError } from "../../rpc/client";

export interface TerminalSink {
  write(data: Uint8Array, done: () => void): void;
  reset(): void;
}
export type LinkState = "connecting" | "live" | "lost" | "ended";

interface Opts {
  id: string;
  readOnly: boolean;
  sink: TerminalSink;
  onState(s: LinkState): void;
  onInputDropped(): void;
}

const enc = new TextEncoder();

export class TerminalLink {
  private stream?: { streamId: number; write(b: Uint8Array): void; end(): void };
  private state: LinkState = "connecting";
  private reported = false;
  private size = { cols: 80, rows: 24 };
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private offState: () => void;
  private disposed = false;

  constructor(private client: RpcClient, private opts: Opts) {
    this.offState = client.onState((s) => {
      if (s === "ready" && this.state === "lost") {
        this.opts.sink.reset();
        void this.open(this.size.cols, this.size.rows);
      } else if (s !== "ready" && this.state === "live") {
        this.set("lost");
      }
    });
  }

  private set(s: LinkState) {
    if (this.state === s && s !== "connecting") return;
    this.state = s;
    if (s === "live") this.reported = false;
    this.opts.onState(s);
  }

  async open(cols: number, rows: number) {
    this.size = { cols, rows };
    this.set("connecting");
    try {
      this.stream = await this.client.openStream(
        "terminal.attach",
        { id: this.opts.id, cols, rows, readOnly: this.opts.readOnly },
        {
          onData: (bytes, ack) => this.opts.sink.write(bytes, ack),
          onEnd: () => {
            this.stream = undefined;
            if (!this.disposed && this.state !== "ended") this.set("lost");
          },
        },
      );
      if (this.disposed) {
        this.stream.end();
        return;
      }
      this.set("live");
    } catch (e) {
      this.set(e instanceof RpcError && e.code === "terminal.not_found" ? "ended" : "lost");
    }
  }

  input(text: string) {
    if (this.opts.readOnly) return;
    if (this.state !== "live" || !this.stream) {
      if (!this.reported) {
        this.reported = true;
        this.opts.onInputDropped();
      }
      return;
    }
    this.stream.write(enc.encode(text));
  }

  resize(cols: number, rows: number) {
    this.size = { cols, rows };
    clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => {
      if (this.stream && !this.opts.readOnly) void this.client.call("terminal.resize", { streamId: this.stream.streamId, cols, rows }).catch(() => {});
    }, 100);
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.resizeTimer);
    this.offState();
    this.stream?.end();
  }
}
```

- [ ] **Step 5: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/rpc src/features/terminal && pnpm --filter app typecheck
git add app/src && git commit -m "feat(app): keep Tauri sends ordered and link terminals to attach streams"
```

---

### Task 15: TerminalView

**Files:**
- Create: `app/src/features/terminal/TerminalView.tsx`, `TerminalView.module.css`, `app/src/features/terminal/theme.ts`, `app/src/features/terminal/keys.ts`, `app/src/features/terminal/osc.ts`
- Create: `app/src/assets/fonts/D2Coding.woff2`(OFL, `LICENSE-D2Coding.txt` 함께), `app/src/styles/fonts.css`에 `@font-face`
- Modify: `app/package.json`(xterm 패키지)
- Test: `app/src/features/terminal/keys.test.ts`, `app/src/features/terminal/osc.test.ts`

**Interfaces:**
- Consumes: Task 14 `TerminalLink`
- Produces:
  - `<TerminalView id readOnly? scale? onCwd?(path) onBell?() onTitle?(t) autoFocus? />`
    - `data-keyscope="terminal"`
    - 연결 덮개: `connecting`이면 "연결 중", `lost`면 "다시 연결 중 · 입력 전송 안 됨", `ended`면 "세션이 끝났습니다" + [닫기]
    - 입력이 버려지면 토스트 "연결이 끊겨 입력을 보내지 못했습니다"
  - `terminalTheme(dark: boolean)`: CSS 토큰에서 xterm 색 생성
  - `terminalKey(e, hasSelection) -> "copy"|"paste"|"zoomIn"|"zoomOut"|"zoomReset"|"search"|"bottom"|null`
    - Ctrl+Shift+C·V
    - Ctrl+=·-·0
    - Ctrl+Shift+F
    - Ctrl+\`
    - 선택이 있을 때 Ctrl+C는 복사(선택이 없으면 셸로 SIGINT)
  - `parseOsc7(data) -> string | null`(`file://host/path` → 디코딩한 경로)

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/features/terminal/keys.test.ts
import { terminalKey } from "./keys";
const k = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

test("terminal-local keys", () => {
  expect(terminalKey(k({ code: "KeyC", ctrlKey: true, shiftKey: true }), false)).toBe("copy");
  expect(terminalKey(k({ code: "KeyC", ctrlKey: true }), true)).toBe("copy");
  expect(terminalKey(k({ code: "KeyC", ctrlKey: true }), false)).toBeNull();
  expect(terminalKey(k({ code: "KeyV", ctrlKey: true, shiftKey: true }), false)).toBe("paste");
  expect(terminalKey(k({ code: "Equal", ctrlKey: true }), false)).toBe("zoomIn");
  expect(terminalKey(k({ code: "Minus", ctrlKey: true }), false)).toBe("zoomOut");
  expect(terminalKey(k({ code: "Digit0", ctrlKey: true }), false)).toBe("zoomReset");
  expect(terminalKey(k({ code: "KeyF", ctrlKey: true, shiftKey: true }), false)).toBe("search");
  expect(terminalKey(k({ code: "Backquote", ctrlKey: true }), false)).toBe("bottom");
  expect(terminalKey(k({ code: "KeyL", ctrlKey: true }), false)).toBeNull();
  expect(terminalKey(k({ code: "KeyC", ctrlKey: true, shiftKey: true, isComposing: true }), true)).toBeNull();
});
```

```ts
// app/src/features/terminal/osc.test.ts
import { parseOsc7 } from "./osc";

test("OSC 7 file URLs become decoded paths", () => {
  expect(parseOsc7("file://host/home/u/projects/my%20app")).toBe("/home/u/projects/my app");
  expect(parseOsc7("file:///tmp")).toBe("/tmp");
  expect(parseOsc7("http://x/y")).toBeNull();
  expect(parseOsc7("garbage")).toBeNull();
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/features/terminal`

- [ ] **Step 3: 구현 — 순수 부분**

```ts
// app/src/features/terminal/keys.ts
export type TerminalKey = "copy" | "paste" | "zoomIn" | "zoomOut" | "zoomReset" | "search" | "bottom";

export function terminalKey(e: KeyboardEvent, hasSelection: boolean): TerminalKey | null {
  if (e.isComposing || e.keyCode === 229 || !e.ctrlKey || e.altKey) return null;
  if (e.shiftKey) {
    if (e.code === "KeyC") return "copy";
    if (e.code === "KeyV") return "paste";
    if (e.code === "KeyF") return "search";
    return null;
  }
  if (e.code === "KeyC" && hasSelection) return "copy";
  if (e.code === "Equal") return "zoomIn";
  if (e.code === "Minus") return "zoomOut";
  if (e.code === "Digit0") return "zoomReset";
  if (e.code === "Backquote") return "bottom";
  return null;
}
```

Ctrl+0은 Ctrl+숫자라 앱 단축키(섹션 이동 Ctrl+1–6)와 겹치지 않는다. Ctrl+1–6은 `allowedWhileFocused`가 앱으로 보내므로 터미널에서도 섹션 이동이 된다.

```ts
// app/src/features/terminal/osc.ts
export function parseOsc7(data: string): string | null {
  try {
    const u = new URL(data);
    return u.protocol === "file:" ? decodeURIComponent(u.pathname) : null;
  } catch {
    return null;
  }
}
```

```ts
// app/src/features/terminal/theme.ts
import type { ITheme } from "@xterm/xterm";

const v = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

export function terminalTheme(): ITheme {
  return {
    background: v("--term-bg"),
    foreground: v("--term-fg"),
    cursor: v("--accent"),
    selectionBackground: v("--term-selection"),
    black: v("--term-black"), red: v("--term-red"), green: v("--term-green"), yellow: v("--term-yellow"),
    blue: v("--term-blue"), magenta: v("--term-magenta"), cyan: v("--term-cyan"), white: v("--term-white"),
    brightBlack: v("--term-bright-black"), brightRed: v("--term-bright-red"), brightGreen: v("--term-bright-green"), brightYellow: v("--term-bright-yellow"),
    brightBlue: v("--term-bright-blue"), brightMagenta: v("--term-bright-magenta"), brightCyan: v("--term-bright-cyan"), brightWhite: v("--term-bright-white"),
  };
}
```

`tokens.css`의 다크·라이트 두 벌에 `--term-*` 18개를 추가한다(다크는 One Dark 계열, 라이트는 One Light 계열 값). 편집기·diff 색도 같은 파일의 토큰에서 만든다(01-design §8.3 "테마 모듈은 하나").

- [ ] **Step 4: 구현 — 컴포넌트**

```tsx
// app/src/features/terminal/TerminalView.tsx
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import { useRpc } from "../../rpc/RpcProvider";
import { useLayout } from "../../shell/layout";
import { Button, useToasts } from "../../ui";
import { openExternal } from "../../shell/external";
import { terminalKey } from "./keys";
import { type LinkState, TerminalLink } from "./link";
import { parseOsc7 } from "./osc";
import { terminalTheme } from "./theme";
import s from "./TerminalView.module.css";

const FONT = '"Cascadia Mono", "D2Coding", monospace';

export function TerminalView({ id, readOnly = false, scale, onCwd, onBell, onTitle, onClose, autoFocus }: { id: string; readOnly?: boolean; scale?: number; onCwd?: (p: string) => void; onBell?: () => void; onTitle?: (t: string) => void; onClose?: () => void; autoFocus?: boolean }) {
  const client = useRpc();
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<LinkState>("connecting");
  const [searching, setSearching] = useState(false);
  const searchRef = useRef<SearchAddon | undefined>(undefined);
  const termRef = useRef<Terminal | undefined>(undefined);
  // 콜백이 렌더마다 바뀌어도 터미널을 다시 만들지 않도록 ref로 들고 있는다.
  const cb = useRef({ onCwd, onBell, onTitle });
  cb.current = { onCwd, onBell, onTitle };

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const fontSize = Number(localStorage.getItem("devbox.terminalFont") ?? 13);
    const term = new Terminal({ fontFamily: FONT, fontSize, allowProposedApi: true, scrollback: 10_000, theme: terminalTheme(), disableStdin: readOnly, cursorBlink: !readOnly });
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon((_e, uri) => openExternal(uri)));
    term.open(el);
    // E2E는 글자를 DOM에서 읽어야 하므로 ?renderer=dom 이면 WebGL을 쓰지 않는다.
    if (new URLSearchParams(location.search).get("renderer") !== "dom") {
      try {
        term.loadAddon(new WebglAddon());
      } catch {
        // WebGL을 못 쓰면 기본 렌더러로 둔다.
      }
    }
    termRef.current = term;
    searchRef.current = search;
    const link = new TerminalLink(client, {
      id,
      readOnly,
      sink: { write: (d, done) => term.write(d, done), reset: () => term.reset() },
      onState: setState,
      onInputDropped: () => useToasts.getState().push({ tone: "warn", title: "연결이 끊겨 입력을 보내지 못했습니다", body: "다시 연결되면 화면을 확인하고 다시 입력해 주세요." }),
    });
    term.onData((d) => link.input(d));
    term.onBinary((d) => link.input(d));
    term.onResize(({ cols, rows }) => link.resize(cols, rows));
    term.onBell(() => cb.current.onBell?.());
    term.onTitleChange((t) => cb.current.onTitle?.(t));
    term.parser.registerOscHandler(7, (data) => {
      const p = parseOsc7(data);
      if (p) cb.current.onCwd?.(p);
      return true;
    });
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const k = terminalKey(e, term.hasSelection());
      if (!k) return true;
      e.preventDefault();
      if (k === "copy") void navigator.clipboard.writeText(term.getSelection());
      if (k === "paste") void navigator.clipboard.readText().then((t) => term.paste(t));
      if (k === "zoomIn" || k === "zoomOut" || k === "zoomReset") {
        const size = k === "zoomReset" ? 13 : Math.min(28, Math.max(9, (term.options.fontSize ?? 13) + (k === "zoomIn" ? 1 : -1)));
        term.options.fontSize = size;
        localStorage.setItem("devbox.terminalFont", String(size));
        fit.fit();
      }
      if (k === "search") setSearching(true);
      if (k === "bottom") useLayout.getState().toggleBottom();
      return false;
    });
    fit.fit();
    void link.open(term.cols, term.rows);
    if (autoFocus) term.focus();
    const ro = new ResizeObserver(() => {
      if (scale === undefined) fit.fit();
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      link.dispose();
      term.dispose();
    };
    // biome-ignore lint/correctness/useExhaustiveDependencies: autoFocus only matters when the terminal is created
  }, [client, id, readOnly, scale]);

  return (
    <div className={s.wrap} data-keyscope="terminal" style={scale ? { transform: `scale(${scale})`, transformOrigin: "0 0", width: `${100 / scale}%`, height: `${100 / scale}%` } : undefined}>
      <div ref={host} className={s.term} />
      {searching && <TerminalSearch addon={searchRef.current} onClose={() => (setSearching(false), termRef.current?.focus())} />}
      {state !== "live" && (
        <div className={s.overlay} role="status">
          {state === "connecting" && "연결 중"}
          {state === "lost" && "다시 연결 중 · 입력 전송 안 됨"}
          {state === "ended" && (
            <>
              세션이 끝났습니다
              {onClose && <Button size="sm" onClick={onClose}>닫기</Button>}
            </>
          )}
        </div>
      )}
    </div>
  );
}
```

- `TerminalSearch`는 같은 파일 아래의 작은 컴포넌트다. 입력란(`aria-label="터미널 검색"`, 열리면 포커스)과 [이전][다음][닫기]가 있고, Enter는 `findNext`, Shift+Enter는 `findPrevious`, Esc는 닫기다. `addon.findNext(text, { incremental: true })`를 쓴다.
- `openExternal(uri)`는 `app/src/shell/external.ts`에 둔다. Tauri면 `@tauri-apps/plugin-opener`의 `openUrl`, 브라우저면 `window.open(uri, "_blank", "noopener")`다. `http:`·`https:`만 허용한다.
- `TerminalView`가 xterm을 만든 뒤 `useEffect` 정리 함수가 `term.dispose()`를 부르므로, 같은 `id`로 다시 그려도 PTY 연결은 하나만 남는다(이전 링크는 `dispose`에서 스트림을 끝냄).
- 격자 보기(Task 19)는 `scale`을 넘긴다. 이때 fit을 하지 않고 tmux 창 크기 그대로 그린 뒤 CSS로 줄인다(`attach -f ignore-size`).

```css
/* app/src/features/terminal/TerminalView.module.css */
.wrap { position: relative; height: 100%; width: 100%; background: var(--term-bg); }
.term { position: absolute; inset: 4px 0 0 6px; }
.overlay { position: absolute; inset: 0; display: flex; gap: var(--space-3); align-items: center; justify-content: center; background: color-mix(in srgb, var(--bg-0) 70%, transparent); color: var(--fg); font-size: var(--text-sm); }
```

- [ ] **Step 5: 통과·커밋**

```bash
pnpm --filter app add @xterm/xterm@^6 @xterm/addon-fit @xterm/addon-webgl @xterm/addon-unicode11 @xterm/addon-search @xterm/addon-web-links @tauri-apps/plugin-opener
pnpm --filter app exec vitest run src/features/terminal && pnpm --filter app typecheck
git add app && git commit -m "feat(app): render tmux sessions with xterm, search, links and a reconnect overlay"
```

---

### Task 16: 터미널 섹션·하단 패널 터미널·분리 창·E2E

**Files:**
- Create: `app/src/features/terminal/TerminalPage.tsx`, `SessionList.tsx`, `PaneTree.tsx`, `store.ts`, `popout.ts`, `BottomTerminal.tsx`
- Modify: `app/src/App.tsx`(pages·popouts 등록, 하단 탭 등록, 명령 등록)
- Modify: `app/src-tauri/capabilities/default.json`(`core:webview:allow-create-webview-window`, `core:window:allow-set-focus`)
- Create: `app/e2e/terminal.spec.ts`
- Modify: `.github/workflows/ci.yml`(e2e job에 `sudo apt-get install -y tmux`)
- Test: `app/src/features/terminal/store.test.ts`

**Interfaces:**
- Consumes: Task 13 `SectionLayout`·`Tabs`·`SplitPane`·`Menu`·`useBottomTabs`, Task 15 `TerminalView`, 04 Task 3 `terminal.*`
- Produces:
  - `useTerminalLayout`(Zustand, `localStorage["devbox.terminalLayout"]`)
    - 상태: `byProject: Record<projectKey, { tabs: Tab[]; active?: string }>`
    - `Tab { id; root: Pane }`, `Pane = { kind: "leaf"; session } | { kind: "split"; dir; ratio; a; b }`
  - 순수 함수: `openTab(l, session)`, `splitActive(l, target, session, dir)`, `closePane(l, session)`, `prune(l, alive: Set<string>)`, `sessionsIn(pane)`
  - 터미널 화면:
    - 목록: 현재 프로젝트 세션(살아 있음 점·제목·명령). 행 메뉴는 [이름 바꾸기][새 창으로][세션 끝내기]
    - 위쪽 [새 세션▾]: 터미널 프로필마다 한 항목(`terminal.profiles`, 고르면 `command`로 만듦) · 기존 zellij 세션에 붙기
  - 단축키(터미널 섹션):
    - Ctrl+Shift+T: 새 세션 탭
    - Ctrl+Shift+E: 오른쪽 분할
    - Ctrl+Shift+D: 아래 분할
    - Ctrl+Shift+W: 칸 닫기(세션은 유지)
  - 하단 패널 `터미널` 탭: 현재 프로젝트의 "하단" 세션 하나(없으면 만들고 `localStorage`에 기억)
  - `openPopout(kind: "terminal"|"agent"|"agents-grid", id?)`: Tauri면 `WebviewWindow`, 브라우저면 `window.open`. 같은 대상 창이 있으면 앞으로 가져온다.
  - 분리 창 경로: `/popout/terminal/$id`

- [ ] **Step 1: 실패하는 테스트(배치 함수)**

```ts
// app/src/features/terminal/store.test.ts
import { closePane, emptyLayout, openTab, prune, sessionsIn, splitActive } from "./store";

test("open, split, close and prune panes", () => {
  let l = openTab(emptyLayout(), "s1");
  expect(l.tabs).toHaveLength(1);
  l = splitActive(l, "s1", "s2", "row");
  expect(sessionsIn(l.tabs[0].root)).toEqual(["s1", "s2"]);
  l = splitActive(l, "s2", "s3", "column");
  expect(sessionsIn(l.tabs[0].root)).toEqual(["s1", "s2", "s3"]);
  l = closePane(l, "s2");
  expect(sessionsIn(l.tabs[0].root)).toEqual(["s1", "s3"]);
  l = openTab(l, "s4");
  expect(l.active).toBe(l.tabs[1].id);
  l = prune(l, new Set(["s1", "s4"]));
  expect(l.tabs.map((t) => sessionsIn(t.root))).toEqual([["s1"], ["s4"]]);
  l = closePane(l, "s4");
  expect(l.tabs).toHaveLength(1);
  expect(l.active).toBe(l.tabs[0].id);
});

test("opening a session already shown focuses its tab instead of duplicating", () => {
  let l = openTab(emptyLayout(), "s1");
  l = openTab(l, "s2");
  l = openTab(l, "s1");
  expect(l.tabs).toHaveLength(2);
  expect(l.active).toBe(l.tabs[0].id);
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/features/terminal/store.test.ts`

- [ ] **Step 3: 구현 — 배치 함수와 저장**

```ts
// app/src/features/terminal/store.ts
import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Pane = { kind: "leaf"; session: string } | { kind: "split"; dir: "row" | "column"; ratio: number; a: Pane; b: Pane };
export interface Tab {
  id: string;
  root: Pane;
}
export interface Layout {
  tabs: Tab[];
  active?: string;
}

export const emptyLayout = (): Layout => ({ tabs: [] });

export function sessionsIn(p: Pane): string[] {
  return p.kind === "leaf" ? [p.session] : [...sessionsIn(p.a), ...sessionsIn(p.b)];
}

export function openTab(l: Layout, session: string): Layout {
  const existing = l.tabs.find((t) => sessionsIn(t.root).includes(session));
  if (existing) return { ...l, active: existing.id };
  const tab = { id: crypto.randomUUID(), root: { kind: "leaf", session } as Pane };
  return { tabs: [...l.tabs, tab], active: tab.id };
}

function mapPane(p: Pane, f: (leaf: Extract<Pane, { kind: "leaf" }>) => Pane | null): Pane | null {
  if (p.kind === "leaf") return f(p);
  const a = mapPane(p.a, f);
  const b = mapPane(p.b, f);
  if (a && b) return { ...p, a, b };
  return a ?? b;
}

export function splitActive(l: Layout, target: string, session: string, dir: "row" | "column"): Layout {
  return {
    ...l,
    tabs: l.tabs.map((t) => ({ ...t, root: mapPane(t.root, (leaf) => (leaf.session === target ? { kind: "split", dir, ratio: 0.5, a: leaf, b: { kind: "leaf", session } } : leaf)) ?? t.root })),
  };
}

export function closePane(l: Layout, session: string): Layout {
  const tabs = l.tabs.flatMap((t) => {
    const root = mapPane(t.root, (leaf) => (leaf.session === session ? null : leaf));
    return root ? [{ ...t, root }] : [];
  });
  const active = tabs.some((t) => t.id === l.active) ? l.active : tabs.at(-1)?.id;
  return { tabs, active };
}

export function prune(l: Layout, alive: Set<string>): Layout {
  return l.tabs.flatMap((t) => sessionsIn(t.root)).filter((s) => !alive.has(s)).reduce(closePane, l);
}

interface Store {
  byProject: Record<string, Layout>;
  update(project: string, f: (l: Layout) => Layout): void;
}

export const useTerminalLayout = create<Store>()(
  persist(
    (set, get) => ({
      byProject: {},
      update: (project, f) => set({ byProject: { ...get().byProject, [project]: f(get().byProject[project] ?? emptyLayout()) } }),
    }),
    { name: "devbox.terminalLayout" },
  ),
);
```

- [ ] **Step 4: 구현 — 화면**

```tsx
// app/src/features/terminal/TerminalPage.tsx (요점)
export function TerminalPage() {
  const project = useCurrentProject((s) => s.id) ?? "none";
  const sessions = useRpcQuery("terminal.list", { projectId: project === "none" ? null : project });
  useTopicInvalidation("terminal.changed", [["terminal.list", { projectId: project === "none" ? null : project }]]);
  const layout = useTerminalLayout((s) => s.byProject[project] ?? emptyLayout());
  const update = useTerminalLayout((s) => s.update);
  const create = useRpcMutation("terminal.create");
  const [focused, setFocused] = useState<string>();
  const cwdOf = useRef(new Map<string, string>());

  // 세션 목록이 바뀌면 사라진 칸을 정리한다.
  useEffect(() => {
    if (sessions.data) update(project, (l) => prune(l, new Set(sessions.data.items.filter((x) => x.alive).map((x) => x.id))));
  }, [sessions.data, project, update]);

  const newSession = async (place: "tab" | "row" | "column") => {
    const cwd = focused ? cwdOf.current.get(focused) : undefined;
    const s = await create.mutateAsync({ projectId: project === "none" ? null : project, cwd: cwd ?? null, title: null, command: null, attachZellij: null });
    update(project, (l) => (place === "tab" || !focused ? openTab(l, s.id) : splitActive(l, focused, s.id, place)));
    setFocused(s.id);
  };

  useKeymap(useMemo(() => [
    { id: "term.new", code: "KeyT", ctrl: true, shift: true, scope: "section", label: "새 터미널", run: () => void newSession("tab") },
    { id: "term.splitRight", code: "KeyE", ctrl: true, shift: true, scope: "section", label: "오른쪽 분할", run: () => void newSession("row") },
    { id: "term.splitDown", code: "KeyD", ctrl: true, shift: true, scope: "section", label: "아래 분할", run: () => void newSession("column") },
    { id: "term.closePane", code: "KeyW", ctrl: true, shift: true, scope: "section", label: "칸 닫기", run: () => focused && update(project, (l) => closePane(l, focused)) },
  ], [focused, project]));

  const active = layout.tabs.find((t) => t.id === layout.active);
  return (
    <SectionLayout list={<SessionList project={project} sessions={sessions} onOpen={(id) => update(project, (l) => openTab(l, id))} onNew={newSession} />}>
      {active ? (
        <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
          <Tabs items={layout.tabs.map((t) => ({ id: t.id, label: titleOf(t, sessions.data?.items), closable: true }))} active={layout.active}
            onSelect={(id) => update(project, (l) => ({ ...l, active: id }))}
            onClose={(id) => update(project, (l) => sessionsIn(l.tabs.find((t) => t.id === id)!.root).reduce(closePane, l))} />
          <div style={{ flex: 1, minHeight: 0 }}>
            <PaneTree pane={active.root} focused={focused} onFocus={setFocused} onCwd={(s, p) => cwdOf.current.set(s, p)}
              onRatio={(path, r) => update(project, (l) => setRatio(l, active.id, path, r))} onEnded={(s) => update(project, (l) => closePane(l, s))} />
          </div>
        </div>
      ) : (
        <EmptyState title="열린 터미널이 없습니다" body="Ctrl+Shift+T로 새 세션을 엽니다. 앱을 닫아도 세션은 계속 실행됩니다." action={<Button variant="primary" onClick={() => void newSession("tab")}>새 터미널</Button>} />
      )}
    </SectionLayout>
  );
}
```

- `PaneTree`는 `Pane`을 재귀로 그린다. leaf는 `<TerminalView id onCwd onClose={onEnded} />`를 `onFocusCapture`로 감싸 `onFocus(session)`을 부르고, 포커스 칸에 테두리를 표시한다. split은 `SplitPane`이다.
- 분할 비율과 탭 제목은 `store.ts`의 순수 함수로 둔다.

```ts
// app/src/features/terminal/store.ts 에 추가
export function setRatio(l: Layout, tabId: string, path: ("a" | "b")[], ratio: number): Layout {
  const walk = (p: Pane, rest: ("a" | "b")[]): Pane => {
    if (p.kind === "leaf") return p;
    if (rest.length === 0) return { ...p, ratio };
    const [head, ...tail] = rest;
    return head === "a" ? { ...p, a: walk(p.a, tail) } : { ...p, b: walk(p.b, tail) };
  };
  return { ...l, tabs: l.tabs.map((t) => (t.id === tabId ? { ...t, root: walk(t.root, path) } : t)) };
}

export function titleOf(t: Tab, sessions: { id: string; title: string }[] = []): string {
  const ids = sessionsIn(t.root);
  const first = sessions.find((s) => s.id === ids[0])?.title ?? "터미널";
  return ids.length > 1 ? `${first} +${ids.length - 1}` : first;
}
```

```ts
// app/src/features/terminal/store.test.ts 에 추가
test("ratio updates follow the split path and titles count panes", () => {
  let l = splitActive(openTab(emptyLayout(), "s1"), "s1", "s2", "row");
  l = setRatio(l, l.tabs[0].id, [], 0.3);
  expect(l.tabs[0].root).toMatchObject({ kind: "split", ratio: 0.3 });
  expect(titleOf(l.tabs[0], [{ id: "s1", title: "app" }])).toBe("app +1");
});
```
- `SessionList`:
  - 행: `StatusDot`(살아 있으면 ok, 아니면 neutral) + 제목 + 명령 + 메뉴(`Menu`).
  - [이름 바꾸기]는 행 안 입력란, Enter 저장.
  - [세션 끝내기]는 `confirmPolicy("terminal.close") === "always"`라서 `ConfirmDialog`("터미널 세션 끝내기 · <제목>", 버튼 "세션 끝내기")를 거친다.
  - [새 세션▾] 메뉴에서 "기존 zellij 세션에 붙기"를 고르면 `terminal.zellij_list`를 부르고, 목록 대화상자에서 고른 이름으로 `terminal.create{ attachZellij }`를 부른다. 목록이 비면 "zellij 세션이 없습니다".
- 프로젝트를 고르지 않았으면(`none`) HOME에서 시작하고 목록 제목은 "프로젝트 없음"이다.

```ts
// app/src/features/terminal/popout.ts
export async function openPopout(kind: "terminal" | "agent" | "agents-grid", id?: string) {
  const path = `/popout/${kind}${id ? `/${encodeURIComponent(id)}` : ""}`;
  const label = `popout-${kind}-${id ?? "main"}`.replace(/[^a-zA-Z0-9-]/g, "-");
  if ("__TAURI_INTERNALS__" in window) {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const existing = await WebviewWindow.getByLabel(label);
    if (existing) return void existing.setFocus();
    new WebviewWindow(label, { url: path, title: "devbox", width: 1000, height: 700, minWidth: 480, minHeight: 320 });
    return;
  }
  window.open(`${path}${location.search}`, label, "width=1000,height=700");
}
```

분리 창은 자기 창 이름으로 `rpc_attach`를 해서 별도 채널을 얻는다(S0b Task 23). 창이 닫히면 `Destroyed`에서 채널이 정리된다. 세션은 tmux에 남는다.

```tsx
// app/src/features/terminal/BottomTerminal.tsx
export function BottomTerminal() {
  const project = useCurrentProject((s) => s.id);
  const key = `devbox.bottomTerminal.${project ?? "none"}`;
  const [id, setId] = useState(() => localStorage.getItem(key));
  const create = useRpcMutation("terminal.create");
  const list = useRpcQuery("terminal.list", { projectId: project ?? null });
  const alive = !!id && !!list.data?.items.some((s) => s.id === id && s.alive);
  useEffect(() => {
    if (!list.data || alive || create.isPending) return;
    void create.mutateAsync({ projectId: project ?? null, cwd: null, title: "하단", command: null, attachZellij: null }).then((s) => {
      localStorage.setItem(key, s.id);
      setId(s.id);
    });
  }, [list.data, alive, key, project, create]);
  return alive && id ? <TerminalView id={id} /> : <EmptyState title="터미널을 준비하고 있습니다" />;
}
```

`App.tsx`에서 하는 일:
- `pages["/terminal"] = { component: TerminalPage }`
- `popouts: [{ path: "/popout/terminal/$id", component: PopoutTerminal }]`(`useParams`로 id → 전체 화면 `TerminalView`)
- `useBottomTabs.getState().register({ id: "terminal", label: "터미널", render: () => <BottomTerminal/> })`
- 명령 등록: "새 터미널"·"하단 패널 열고 닫기"·"터미널을 새 창으로"

- [ ] **Step 5: E2E**

```ts
// app/e2e/terminal.spec.ts
import { expect, test } from "@playwright/test";

const e2e = () => JSON.parse(process.env.DEVBOX_E2E ?? "{}");

test("a terminal session survives a page reload with its scrollback", async ({ page }) => {
  await page.goto(`/terminal?gateway=${encodeURIComponent(e2e().gatewayUrl)}`);
  await page.getByRole("button", { name: "새 터미널" }).click();
  const term = page.locator("[data-keyscope=terminal]").first();
  await term.click();
  await page.keyboard.type("for i in $(seq 1 60); do echo row-$i; done\n");
  await expect(term).toContainText("row-60");
  await page.reload();
  const again = page.locator("[data-keyscope=terminal]").first();
  await expect(again).toContainText("row-60");
  await again.click();
  await page.keyboard.press("Control+Shift+F");
  await page.getByRole("textbox", { name: "터미널 검색" }).fill("row-1");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("textbox", { name: "터미널 검색" })).toBeVisible();
});

test("split right opens a second live pane in the same tab", async ({ page }) => {
  await page.goto(`/terminal?gateway=${encodeURIComponent(e2e().gatewayUrl)}`);
  await page.getByRole("button", { name: "새 터미널" }).click();
  await page.locator("[data-keyscope=terminal]").first().click();
  await page.keyboard.press("Control+Shift+E");
  await expect(page.locator("[data-keyscope=terminal]")).toHaveCount(2);
  await page.locator("[data-keyscope=terminal]").nth(1).click();
  await page.keyboard.type("echo second-pane\n");
  await expect(page.locator("[data-keyscope=terminal]").nth(1)).toContainText("second-pane");
});
```

xterm은 WebGL 렌더러를 쓰면 DOM에 글자가 없다. E2E에서는 `?renderer=dom` 쿼리가 있으면 WebGL을 불러오지 않게 하고(`TerminalView`의 `loadAddon(new WebglAddon())` 앞에서 검사), 위 테스트의 `goto`에 `&renderer=dom`을 붙인다. 접근성 트리(`screenReaderMode`) 대신 DOM 렌더러의 `.xterm-rows`를 `toContainText`로 읽는다.

- [ ] **Step 6: 통과·커밋·묶음 K 끝**

```bash
pnpm --filter app exec vitest run src/features/terminal && pnpm --filter app exec playwright test e2e/terminal.spec.ts
git add app .github && git commit -m "feat(app): add the terminal section with tabs, splits, bottom panel and pop-out windows"
# 묶음 K 끝: PROGRESS.md의 묶음 K 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 17: 에이전트 목록·새 작업·이동 단축키·배지

**Files:**
- Create: `app/src/features/agents/{AgentsPage,AgentList,AgentRow,NewTaskDialog,model,hooks}.tsx|ts`
- Modify: `app/src/shell/ActivityBar.tsx`(배지), `app/src/shell/StatusBar.tsx`(에이전트 요약), `app/src/App.tsx`, `app/src/rpc/messages.ts`(agents·terminal·git 오류 문구)
- Test: `app/src/features/agents/model.test.ts`, `app/src/features/agents/NewTaskDialog.test.tsx`

**Interfaces:**
- Consumes: 04 Task 8–11의 `agents.list`·`agents.create`·`agents.profiles`·`agents.git_stats`·`agents.resources`·`projects.prompts`, 주제 `agents.changed`
- Produces:
  - `type AgentTask = Methods["agents.get"]["result"]`
  - `groupByUrgency(tasks, nowMs) -> { key, label, items }[]`
    - 순서: 입력 대기(waiting·idle) → 실패 → 검토 대기 → 실행 중 → 준비 중 → 닫힘(24시간)
    - 입력 대기는 오래 기다린 것부터, 나머지는 최근 것부터
  - `nextWaiting(groups, currentId) -> id | undefined`(순환)
  - `attentionCount(tasks)`(waiting+idle+failed), `hookMissing(tasks, profiles, nowMs) -> boolean`
    - Claude 프로필의 `running` 작업이 만든 지 90초가 지나도 `sessionId`가 없으면 참
  - `fillTemplate(body, vars: { title, branch, project }) -> string`
  - `useAgentsData()`: `agents.list` + `agents.git_stats`(10초) + `agents.resources`(3초, 화면이 보일 때만)를 한 번에 묶은 훅
  - 경로: `/agents?id=<작업>&view=list|grid`
  - 단축키: Ctrl+Shift+N(새 작업, 어디서나), Ctrl+Shift+J(다음 입력 대기, 어디서나)
  - 활동 막대 배지: 에이전트 섹션에 `attentionCount`. 실패가 있으면 위험 색
  - 상태 표시줄: "에이전트 n(입력 대기 m)" 클릭하면 `/agents`

- [ ] **Step 1: 실패하는 테스트 — 순수 함수**

```ts
// app/src/features/agents/model.test.ts
import { attentionCount, fillTemplate, groupByUrgency, hookMissing, nextWaiting } from "./model";

const t = (id: string, state: string, updatedMs: number, extra: Record<string, unknown> = {}) =>
  ({ id, state, updatedMs, createdMs: updatedMs, profile: "claude", sessionId: "s", closedMs: state === "closed" ? updatedMs : null, ...extra }) as never;

test("groups by urgency and orders waiting oldest first", () => {
  const now = 100 * 3600_000;
  const groups = groupByUrgency([t("r", "running", 5), t("w2", "waiting", 9), t("i", "idle", 3), t("f", "failed", 4), t("c", "closed", now - 1000), t("old", "closed", now - 25 * 3600_000), t("p", "preparing", 1), t("v", "review", 2)], now);
  expect(groups.map((g) => [g.key, g.items.map((x) => x.id)])).toEqual([
    ["attention", ["i", "w2"]],
    ["failed", ["f"]],
    ["review", ["v"]],
    ["running", ["r"]],
    ["preparing", ["p"]],
    ["closed", ["c"]],
  ]);
});

test("next waiting cycles through the attention group", () => {
  const groups = groupByUrgency([t("a", "waiting", 1), t("b", "idle", 2), t("c", "running", 3)], 10);
  expect(nextWaiting(groups, undefined)).toBe("a");
  expect(nextWaiting(groups, "a")).toBe("b");
  expect(nextWaiting(groups, "b")).toBe("a");
  expect(nextWaiting(groupByUrgency([t("c", "running", 3)], 10), "c")).toBeUndefined();
});

test("counts and hook detection", () => {
  expect(attentionCount([t("a", "waiting", 1), t("b", "idle", 1), t("c", "failed", 1), t("d", "running", 1)])).toBe(3);
  const profiles = [{ id: "claude", hooks: "claude" }, { id: "codex", hooks: "codex" }] as never;
  expect(hookMissing([t("a", "running", 0, { sessionId: null, createdMs: 0 })], profiles, 91_000)).toBe(true);
  expect(hookMissing([t("a", "running", 0, { sessionId: null, createdMs: 0 })], profiles, 60_000)).toBe(false);
  expect(hookMissing([t("a", "running", 0, { sessionId: null, createdMs: 0, profile: "codex" })], profiles, 91_000)).toBe(false);
});

test("templates substitute known variables and keep unknown ones", () => {
  expect(fillTemplate("{{title}} on {{branch}} in {{project}} {{other}}", { title: "로그인 수정", branch: "agent/login", project: "app" })).toBe("로그인 수정 on agent/login in app {{other}}");
});
```

- [ ] **Step 2: 실패하는 테스트 — 새 작업 대화상자**

```tsx
// app/src/features/agents/NewTaskDialog.test.tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { NewTaskDialog } from "./NewTaskDialog";

function client(responses: Record<string, unknown>) {
  return {
    start: () => {},
    onState: () => () => {},
    onReset: () => () => {},
    subscribe: () => () => {},
    call: vi.fn((m: string) => Promise.resolve(responses[m])),
  } as never as import("../../rpc/client").RpcClient & { call: ReturnType<typeof vi.fn> };
}

test("creates a task with the chosen profile, template text and worktree", async () => {
  useCurrentProject.getState().set("p1");
  const c = client({
    "projects.list": { rev: "1", items: [{ id: "p1", name: "app", path: "/h/projects/app", favorite: false, addedMs: 1 }] },
    "agents.profiles": { items: [{ id: "claude", label: "Claude Code", command: "claude", hooks: "claude", waitingPatterns: [] }, { id: "codex", label: "Codex", command: "codex", hooks: "codex", waitingPatterns: [] }] },
    "projects.prompts": { items: [{ name: "bugfix", body: "버그: {{title}}\n재현 후 고치고 테스트를 추가해." }] },
    "agents.create": { id: "a1", state: "preparing" },
  });
  const onCreated = vi.fn();
  render(<RpcProvider client={c}><NewTaskDialog open onOpenChange={() => {}} onCreated={onCreated} /></RpcProvider>);
  await userEvent.type(await screen.findByLabelText("제목"), "로그인 실패");
  await userEvent.click(screen.getByRole("combobox", { name: "도구" }));
  await userEvent.click(await screen.findByRole("option", { name: "Codex" }));
  await userEvent.click(screen.getByRole("button", { name: "템플릿: bugfix" }));
  expect(screen.getByLabelText("지시문")).toHaveValue("버그: 로그인 실패\n재현 후 고치고 테스트를 추가해.");
  await userEvent.keyboard("{Control>}{Enter}{/Control}");
  await waitFor(() =>
    expect(c.call).toHaveBeenCalledWith("agents.create", { projectId: "p1", title: "로그인 실패", profile: "codex", baseBranch: null, worktree: true, prompt: "버그: 로그인 실패\n재현 후 고치고 테스트를 추가해." }, expect.anything()),
  );
  expect(onCreated).toHaveBeenCalledWith("a1");
});
```

- [ ] **Step 3: 실패 확인** — `pnpm --filter app exec vitest run src/features/agents`

- [ ] **Step 4: 구현 — model.ts**

```ts
// app/src/features/agents/model.ts
import type { Methods } from "../../rpc/gen/rpc";

export type AgentTask = Methods["agents.get"]["result"];
export type ToolProfile = Methods["agents.profiles"]["result"]["items"][number];

const GROUPS = [
  { key: "attention", label: "입력 대기", states: ["waiting", "idle"], oldestFirst: true },
  { key: "failed", label: "실패", states: ["failed"], oldestFirst: false },
  { key: "review", label: "검토 대기", states: ["review"], oldestFirst: false },
  { key: "running", label: "실행 중", states: ["running"], oldestFirst: false },
  { key: "preparing", label: "준비 중", states: ["preparing"], oldestFirst: false },
  { key: "closed", label: "닫힘(24시간)", states: ["closed"], oldestFirst: false },
] as const;

export function groupByUrgency(tasks: AgentTask[], nowMs: number) {
  return GROUPS.map((g) => ({
    key: g.key,
    label: g.label,
    items: tasks
      .filter((t) => (g.states as readonly string[]).includes(t.state))
      .filter((t) => t.state !== "closed" || nowMs - Number(t.closedMs ?? t.updatedMs) < 24 * 3600_000)
      .sort((a, b) => (g.oldestFirst ? 1 : -1) * (Number(a.updatedMs) - Number(b.updatedMs))),
  })).filter((g) => g.items.length > 0);
}

export function nextWaiting(groups: ReturnType<typeof groupByUrgency>, current?: string): string | undefined {
  const items = groups.find((g) => g.key === "attention")?.items ?? [];
  if (items.length === 0) return undefined;
  const i = items.findIndex((t) => t.id === current);
  return items[(i + 1) % items.length].id;
}

export function attentionCount(tasks: AgentTask[]) {
  return tasks.filter((t) => t.state === "waiting" || t.state === "idle" || t.state === "failed").length;
}

export function hookMissing(tasks: AgentTask[], profiles: ToolProfile[], nowMs: number) {
  const claude = new Set(profiles.filter((p) => p.hooks === "claude").map((p) => p.id));
  return tasks.some((t) => t.state === "running" && claude.has(t.profile) && !t.sessionId && nowMs - Number(t.createdMs) > 90_000);
}

export function fillTemplate(body: string, vars: { title: string; branch: string; project: string }) {
  return body.replace(/\{\{(title|branch|project)\}\}/g, (_, k: keyof typeof vars) => vars[k]);
}

export const STATE_LABEL: Record<AgentTask["state"], string> = {
  preparing: "준비 중",
  running: "실행 중",
  waiting: "입력 대기",
  idle: "다음 지시 대기",
  failed: "실패",
  review: "검토 대기",
  closed: "닫힘",
};
```

`i64` 필드(`createdMs` 등)는 ts-rs가 `bigint`로 내보낼 수 있다. S0b 생성기 설정(`#[ts(type = "number")]` 또는 전역 `bigint → number`)을 따르고, 위 코드는 `Number(...)`로 감싸 두 경우 모두 동작하게 한다.

- [ ] **Step 5: 구현 — 화면**

```tsx
// app/src/features/agents/AgentsPage.tsx (요점)
export const agentsSearch = (s: Record<string, unknown>) => ({ id: typeof s.id === "string" ? s.id : undefined, view: s.view === "grid" ? "grid" : "list" }) as const;

export function AgentsPage() {
  const { id, view } = useSearch({ from: "/agents" });
  const navigate = useNavigate({ from: "/agents" });
  const data = useAgentsData();
  const [creating, setCreating] = useState(false);
  const groups = useMemo(() => groupByUrgency(data.tasks, Date.now()), [data.tasks]);
  return (
    <SectionLayout
      list={
        <AgentList groups={groups} stats={data.stats} resources={data.resources} selected={id}
          onSelect={(x) => void navigate({ search: (s) => ({ ...s, id: x }) })}
          header={<>
            <ViewSwitch value={view} onChange={(v) => void navigate({ search: (s) => ({ ...s, view: v }) })} />
            <Button size="sm" variant="primary" onClick={() => setCreating(true)}>새 작업 <Kbd keys={["Ctrl", "Shift", "N"]} /></Button>
          </>}
          banner={data.hookMissing && <Banner tone="warn" actions={<Button size="sm" onClick={() => void navigate({ to: "/settings" })}>연결 확인</Button>}>상태 알림이 연결되지 않았습니다</Banner>}
        />
      }
    >
      {view === "grid" ? <AgentGrid tasks={groups.flatMap((g) => g.items).filter((t) => t.state !== "closed")} onOpen={(x) => void navigate({ search: { id: x, view: "list" } })} />
        : id ? <AgentDetail id={id} /> : <EmptyState title="에이전트 작업을 고르세요" body="입력을 기다리는 작업이 위에 모입니다." />}
      <NewTaskDialog open={creating} onOpenChange={setCreating} onCreated={(x) => void navigate({ search: { id: x, view: "list" } })} />
    </SectionLayout>
  );
}
```

- `ViewSwitch`는 같은 파일의 작은 컴포넌트다. [목록][격자] 두 `Button`에 `aria-pressed`를 단다.
- `AgentList`는 그룹마다 `<section role="region" aria-label={group.label}>`로 감싼다(E2E가 그룹 이름으로 찾음). 선택한 행은 `aria-current="true"`다.
- [연결 확인]은 Task 21에서 `openSettings("agents")`(설정의 에이전트 탭을 바로 엶)로 바꾼다.

`AgentRow`의 표시 순서:
- 1줄: 상태 아이콘(모양 + 색) · 제목 · 경과 시간(`Intl.RelativeTimeFormat("ko")`) · 프로젝트 이름
- 2줄(흐린 글자): `attention.message` 또는 상태 문구
- 3줄(작은 글자): `변경 n · 뒤처짐 m · CPU x% · 메모리 y MB · 토큰 z`. 값이 없는 칸은 숨긴다.
- `idle`과 `waiting`은 같은 그룹이지만 아이콘이 다르다(💬 질문 대 ⏸ 턴 끝 같은 lucide 아이콘: `MessageCircleQuestion`·`PauseCircle`).
- 세션이 없고 닫히지 않은 작업은 행 끝에 [재개] 버튼을 둔다(`failed` + `reason == "session_lost"`).
- `failed`의 이유는 행의 한 줄 메시지 자리에 보인다: `session_lost` "WSL이 멈춰 세션이 끊겼습니다", `killed:*` "외부에서 종료됨(메모리 부족이나 다른 도구가 끝냈을 수 있습니다)", `exit:<n>` "도구가 오류로 끝났습니다(종료 코드 n)", `setup:<n>` "준비 명령이 실패했습니다". `killed:*`도 [재개]를 둔다.

`NewTaskDialog`:
- 필드: 제목(필수) · 프로젝트(`Select`, 기본 현재 프로젝트) · 도구(`Select`, `agents.profiles`) · 기준 브랜치(입력란, 비우면 현재 브랜치) · [작업 폴더 만들기] 스위치(기본 켬, 끄면 "현재 체크아웃에서 실행") · 지시문(`TextArea`)
- 템플릿 버튼: `projects.prompts` 결과마다 `템플릿: <이름>`. 누르면 `fillTemplate(body, { title, branch: 비어 있으면 "agent/<slug 예상>", project: 이름 })`을 지시문에 넣는다.
- Ctrl+Enter(조합 중이 아닐 때) 또는 [시작]으로 `agents.create`를 부르고, 성공하면 닫고 `onCreated(id)`를 부른다.
- 오류는 `messageFor`로 입력란 아래에 보인다.
- 마지막으로 쓴 도구 프로필을 `localStorage["devbox.lastProfile"]`에 기억한다.

`useAgentsData`:

```ts
// app/src/features/agents/hooks.ts
export function useAgentsData() {
  const list = useRpcQuery("agents.list", { includeClosedHours: 24 });
  useTopicInvalidation("agents.changed", [["agents.list", { includeClosedHours: 24 }], ["agents.git_stats", {}]]);
  const visible = usePageVisible();
  const stats = useRpcQuery("agents.git_stats", {}, { refetchInterval: 10_000 });
  const resources = useRpcQuery("agents.resources", {}, { refetchInterval: visible ? 3_000 : false });
  const profiles = useRpcQuery("agents.profiles", {});
  const tasks = list.data?.items ?? [];
  return {
    tasks,
    stats: new Map((stats.data?.items ?? []).map((x) => [x.id, x])),
    resources: new Map((resources.data?.items ?? []).map((x) => [x.id, x])),
    hookMissing: hookMissing(tasks, profiles.data?.items ?? [], Date.now()),
  };
}
```

`useRpcQuery`의 `opts`에 `refetchInterval`을 추가한다(S0b `query.ts`에서 `{ enabled?, refetchInterval? }`를 그대로 TanStack에 넘김). `usePageVisible`은 `document.visibilityState`를 구독하는 작은 훅이다(`app/src/shell/visibility.ts`).

전역 바인딩(App.tsx):
- `{ id: "agents.new", code: "KeyN", ctrl: true, shift: true, scope: "app", label: "새 에이전트 작업", run: () => openNewTask() }`
- `{ id: "agents.nextWaiting", code: "KeyJ", ctrl: true, shift: true, scope: "app", label: "다음 입력 대기", run: () => goNextWaiting() }`

`openNewTask`는 `useAgentsUi` 스토어(`creating` 플래그)를 켜고 `/agents`로 이동한다. `goNextWaiting`은 쿼리 캐시의 `agents.list`로 `nextWaiting`을 계산해 `/agents?id=`로 이동한다.

`messages.ts`에 04에서 만든 오류 코드 문구를 모두 넣는다. `tsc`가 빠진 키를 알려 준다. 예:
- `"agents.base_dirty": (d) => ({ title: "기준 체크아웃에 커밋하지 않은 변경이 있어 병합하지 않았습니다.", action: \`${files(d)} 를 커밋하거나 치운 뒤 다시 시도해 주세요.\` })`
- `"agents.conflict": (d) => ({ title: "충돌이 나서 병합을 멈추고 원래대로 돌렸습니다.", action: \`충돌 파일: ${files(d)}. [기준 반영]으로 에이전트에게 해결을 맡기세요.\` })`
- `"agents.session_missing": { title: "에이전트 세션이 없습니다.", action: "[재개]로 세션을 다시 시작해 주세요." }`
- `"agents.gh_missing": { title: "gh 명령을 찾지 못했습니다.", action: "WSL에 GitHub CLI를 설치하고 gh auth login을 한 뒤 다시 시도해 주세요." }`
- `"terminal.tmux_failed": (d) => ({ title: "tmux 명령이 실패했습니다.", action: reason(d) })`
- `"git.not_repo": { title: "Git 저장소가 아닙니다." }`

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/features/agents && pnpm --filter app typecheck
git add app/src && git commit -m "feat(app): list agent tasks by urgency and create new tasks with templates"
```

---

### Task 18: 에이전트 상세 — 머리줄·동작·탭·지시 작성

**Files:**
- Create: `app/src/features/agents/detail/{AgentDetail,AgentHeader,AgentActions,ChangesTab,CommitsTab,RunTab,Compose,seen,quote}.tsx|ts`
- Create: `app/src/ui/DiffView.tsx`, `app/src/ui/DiffView.module.css`, `app/src/ui/diff.ts`
- Test: `app/src/ui/diff.test.ts`, `app/src/features/agents/detail/Compose.test.tsx`, `app/src/features/agents/detail/AgentActions.test.tsx`

**Interfaces:**
- Consumes: 04 Task 9의 `agents.send`·`resume`·`review`·`rebase`·`rebase_abort`·`merge`·`pr`·`discard_check`·`discard`·`test`·`changes`·`patch`·`commits`, Task 10의 `agents.usage`, Task 15 `TerminalView`
- Produces:
  - `parseUnifiedDiff(text) -> DiffFile[]`, `DiffFile { oldPath, newPath, hunks: { header, lines: { kind: "ctx"|"add"|"del"|"meta", old?, new?, text }[] }[], binary }`
  - `<DiffView file onSelectLines?(range: { side: "old"|"new", from, to, text }) maxLines=5000 />`
    - 줄 번호를 누르면 선택, Shift+누르면 범위
  - `useSeen`(Zustand persist): `mark(agentId, path, sig)`, `isSeen(agentId, path, sig)`. `sig = status:added:removed`. 줄 수가 바뀌면 다시 안 봄 상태가 된다.
  - `quoteForPrompt(path, range) -> string`
  - `useDrafts`(Zustand persist): 작업별 작성 중 지시
  - `<Compose agentId disabled? />`: Enter 줄바꿈, Ctrl+Enter 보내기, 조합 중 Ctrl+Enter는 `compositionend` 뒤 한 번 실행
  - `AgentActions({ task, sessionAlive })`: 상태별 버튼
    - 늘 보이는 버튼: [이어서 지시](작성란 포커스) · [기준 반영] · [병합▾](merge·squash·rebase) · [PR]
    - 세션이 없고 닫히지 않은 작업은 [이어서 지시] 자리에 [재개]를 둔다.
    - "더 보기"(`Menu`): [테스트 실행] · [검토 시작](running·waiting·idle일 때) · [새 창으로] · [폐기]
    - worktree 없는 작업은 [기준 반영]·[병합▾]·[PR]을 숨기고, 폐기 대신 [세션 끝내기]다.
    - 닫힌 작업은 버튼이 없다.

- [ ] **Step 1: 실패하는 테스트 — diff 해석**

```ts
// app/src/ui/diff.test.ts
import { parseUnifiedDiff } from "./diff";

const patch = `diff --git a/a.txt b/a.txt
index 5626abf..f719efd 100644
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,3 @@
 one
-two
+TWO
+three
\\ No newline at end of file
`;

test("parses hunks with old and new line numbers", () => {
  const [f] = parseUnifiedDiff(patch);
  expect(f.newPath).toBe("a.txt");
  const lines = f.hunks[0].lines;
  expect(lines.map((l) => [l.kind, l.old ?? null, l.new ?? null, l.text])).toEqual([
    ["ctx", 1, 1, "one"],
    ["del", 2, null, "two"],
    ["add", null, 2, "TWO"],
    ["add", null, 3, "three"],
    ["meta", null, null, "\\ No newline at end of file"],
  ]);
});

test("new files from /dev/null and binary files", () => {
  const [n] = parseUnifiedDiff("diff --git a/b b/b\nnew file mode 100644\n--- /dev/null\n+++ b/b\n@@ -0,0 +1 @@\n+x\n");
  expect(n.oldPath).toBeNull();
  expect(n.hunks[0].lines[0]).toMatchObject({ kind: "add", new: 1, text: "x" });
  const [b] = parseUnifiedDiff("diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n");
  expect(b.binary).toBe(true);
});
```

- [ ] **Step 2: 실패하는 테스트 — 지시 작성(IME)과 폐기 확인**

```tsx
// app/src/features/agents/detail/Compose.test.tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { Compose } from "./Compose";
// client()는 NewTaskDialog.test.tsx와 같은 가짜 클라이언트(app/src/test/fakeClient.ts로 옮겨 함께 쓴다)
import { fakeClient } from "../../../test/fakeClient";

test("Ctrl+Enter during IME composition sends once after composition ends", async () => {
  const c = fakeClient({ "agents.send": { id: "a1", state: "running" } });
  render(<RpcProvider client={c}><Compose agentId="a1" /></RpcProvider>);
  const box = screen.getByLabelText("에이전트에게 보낼 지시");
  fireEvent.change(box, { target: { value: "테스트를 추가해" } });
  fireEvent.compositionStart(box);
  fireEvent.keyDown(box, { key: "Enter", code: "Enter", ctrlKey: true, isComposing: true });
  expect(c.call).not.toHaveBeenCalled();
  fireEvent.compositionEnd(box);
  await waitFor(() => expect(c.call).toHaveBeenCalledTimes(1));
  expect(c.call).toHaveBeenCalledWith("agents.send", { id: "a1", text: "테스트를 추가해" }, expect.anything());
  await waitFor(() => expect(box).toHaveValue(""));
});

test("plain Enter inserts a newline and does not send", () => {
  const c = fakeClient({});
  render(<RpcProvider client={c}><Compose agentId="a1" /></RpcProvider>);
  const box = screen.getByLabelText("에이전트에게 보낼 지시");
  fireEvent.keyDown(box, { key: "Enter", code: "Enter" });
  expect(c.call).not.toHaveBeenCalled();
});
```

```tsx
// app/src/features/agents/detail/AgentActions.test.tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcError } from "../../../rpc/client";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { fakeClient } from "../../../test/fakeClient";
import { AgentActions } from "./AgentActions";

const task = { id: "a1", state: "idle", title: "로그인", branch: "agent/login", worktreePath: "/w", terminalId: "t1", reason: null } as never;

test("discard with unmerged commits names the count and needs confirmation", async () => {
  const c = fakeClient({ "agents.discard_check": { unmergedCommits: 3, dirtyFiles: 1, headOid: "abc" }, "agents.discard": { id: "a1", state: "closed" } });
  render(<RpcProvider client={c}><AgentActions task={task} sessionAlive /></RpcProvider>);
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "폐기" }));
  expect(await screen.findByText(/병합하지 않은 커밋 3개/)).toBeInTheDocument();
  expect(screen.getByText(/커밋하지 않은 파일 1개/)).toBeInTheDocument();
  expect(c.call).not.toHaveBeenCalledWith("agents.discard", expect.anything(), expect.anything());
  await userEvent.click(screen.getByRole("button", { name: "작업 폴더와 브랜치 삭제" }));
  // 확인 대화상자가 보여 준 상태를 그대로 보낸다(IR-11).
  await waitFor(() =>
    expect(c.call).toHaveBeenCalledWith("agents.discard", { id: "a1", expect: { headOid: "abc", unmergedCommits: 3 } }, expect.anything()),
  );
});

test("a merge refused because the task moved re-asks with the new state", async () => {
  const c = fakeClient({
    "agents.changes": { base: "main", ahead: 1, behind: 0, headOid: "h1", baseOid: "b1", files: [] },
    "agents.merge": new RpcError("agents.state_changed", { head: "h2", base: "b1", unmerged: 2 }),
  });
  render(<RpcProvider client={c}><AgentActions task={task} sessionAlive /></RpcProvider>);
  await userEvent.click(screen.getByRole("button", { name: "병합" }));
  await userEvent.click(await screen.findByRole("button", { name: /에 병합/ }));
  await waitFor(() =>
    expect(c.call).toHaveBeenCalledWith("agents.merge", expect.objectContaining({ expect: { headOid: "h1", baseOid: "b1" } }), expect.anything()),
  );
  expect(await screen.findByText(/확인한 뒤 작업이 바뀌었습니다/)).toBeInTheDocument();
});

test("a lost session offers resume instead of instructing", () => {
  const lost = { ...(task as object), state: "failed", reason: "session_lost" } as never;
  render(<RpcProvider client={fakeClient({})}><AgentActions task={lost} sessionAlive={false} /></RpcProvider>);
  expect(screen.getByRole("button", { name: "재개" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "이어서 지시" })).toBeNull();
});
```

`app/src/test/fakeClient.ts`로 S0b `ProjectSwitcher.test.tsx`의 `fakeClient`를 옮겨 공용으로 쓴다(그 테스트도 이 파일을 쓰게 바꾼다). 옮길 때 값이 `RpcError`이면 reject하도록 한 줄을 더한다: `call: vi.fn((method: string) => calls[method] instanceof RpcError ? Promise.reject(calls[method]) : Promise.resolve(calls[method]))`.

- [ ] **Step 3: 실패 확인** — `pnpm --filter app exec vitest run src/ui/diff.test.ts src/features/agents/detail`

- [ ] **Step 4: 구현 — diff.ts**

```ts
// app/src/ui/diff.ts
export interface DiffLine {
  kind: "ctx" | "add" | "del" | "meta";
  old?: number;
  new?: number;
  text: string;
}
export interface DiffFile {
  oldPath: string | null;
  newPath: string | null;
  binary: boolean;
  hunks: { header: string; lines: DiffLine[] }[];
}

const strip = (p: string) => (p === "/dev/null" ? null : p.replace(/^[ab]\//, ""));

export function parseUnifiedDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let f: DiffFile | undefined;
  let o = 0;
  let n = 0;
  for (const raw of text.split("\n")) {
    if (raw.startsWith("diff --git ")) {
      const m = /^diff --git a\/(.*) b\/(.*)$/.exec(raw);
      f = { oldPath: m?.[1] ?? null, newPath: m?.[2] ?? null, binary: false, hunks: [] };
      files.push(f);
      continue;
    }
    if (!f) continue;
    if (raw.startsWith("Binary files ")) f.binary = true;
    else if (raw.startsWith("--- ")) f.oldPath = strip(raw.slice(4));
    else if (raw.startsWith("+++ ")) f.newPath = strip(raw.slice(4));
    else if (raw.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw);
      o = Number(m?.[1] ?? 0);
      n = Number(m?.[2] ?? 0);
      f.hunks.push({ header: raw, lines: [] });
    } else if (f.hunks.length > 0) {
      const h = f.hunks[f.hunks.length - 1];
      if (raw.startsWith("+")) h.lines.push({ kind: "add", new: n++, text: raw.slice(1) });
      else if (raw.startsWith("-")) h.lines.push({ kind: "del", old: o++, text: raw.slice(1) });
      else if (raw.startsWith(" ")) h.lines.push({ kind: "ctx", old: o++, new: n++, text: raw.slice(1) });
      else if (raw.startsWith("\\")) h.lines.push({ kind: "meta", text: raw });
    }
  }
  return files;
}
```

새 파일 diff(`@@ -0,0 +1 @@`)는 `n`이 1부터 시작하므로 위 규칙 그대로 맞는다.

- [ ] **Step 5: 구현 — Compose·동작·탭**

```tsx
// app/src/features/agents/detail/Compose.tsx
import { useRef } from "react";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation } from "../../../rpc/query";
import { Button, Kbd, TextArea, useToasts } from "../../../ui";

export const useDrafts = create<{ text: Record<string, string>; set(id: string, t: string): void }>()(
  persist((set, get) => ({ text: {}, set: (id, t) => set({ text: { ...get().text, [id]: t } }) }), { name: "devbox.agentDrafts" }),
);

export function Compose({ agentId, disabled }: { agentId: string; disabled?: boolean }) {
  const text = useDrafts((s) => s.text[agentId] ?? "");
  const setText = useDrafts((s) => s.set);
  const send = useRpcMutation("agents.send");
  const composing = useRef(false);
  const sendAfterComposition = useRef(false);
  const submit = () => {
    const value = useDrafts.getState().text[agentId]?.trim();
    if (!value || send.isPending) return;
    send.mutate(
      { id: agentId, text: value },
      {
        onSuccess: () => setText(agentId, ""),
        onError: (e) => {
          const m = messageFor(e.code, e.detail);
          useToasts.getState().push({ tone: "danger", title: m.title, body: m.action });
        },
      },
    );
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      style={{ display: "flex", gap: "var(--space-2)", alignItems: "flex-end", padding: "var(--space-2)", borderTop: "1px solid var(--border)" }}
    >
      <TextArea
        aria-label="에이전트에게 보낼 지시"
        placeholder="다음 지시를 쓰세요. Enter 줄바꿈, Ctrl+Enter 보내기"
        value={text}
        disabled={disabled}
        maxRows={10}
        style={{ flex: 1 }}
        onChange={(e) => setText(agentId, e.target.value)}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={() => {
          composing.current = false;
          if (sendAfterComposition.current) {
            sendAfterComposition.current = false;
            queueMicrotask(submit);
          }
        }}
        onKeyDown={(e) => {
          if (e.key !== "Enter" || !e.ctrlKey) return;
          e.preventDefault();
          if (composing.current || e.nativeEvent.isComposing) sendAfterComposition.current = true;
          else submit();
        }}
      />
      <Button type="submit" variant="primary" disabled={disabled || !text.trim() || send.isPending}>
        보내기 <Kbd keys={["Ctrl", "Enter"]} />
      </Button>
    </form>
  );
}
```

```ts
// app/src/features/agents/detail/quote.ts
export function quoteForPrompt(path: string, r: { side: "old" | "new"; from: number; to: number; text: string }) {
  const where = r.from === r.to ? `L${r.from}` : `L${r.from}-${r.to}`;
  const which = r.side === "old" ? " (변경 전)" : "";
  return `\`${path}\` ${where}${which}:\n\`\`\`\n${r.text}\n\`\`\`\n`;
}
```

`AgentActions` 동작 규칙:
- 병합▾:
  - 세 항목을 둔다. merge는 "병합 커밋", squash는 "커밋 하나로 합쳐 병합", rebase는 "기준 위로 옮긴 뒤 병합".
  - squash는 메시지 입력 대화상자(기본 `<제목> (agent)`)를 거친다.
  - 확인 정책 `always`로 "`<base>`에 병합 · `<브랜치>`" 확인 후 실행한다.
  - 오류 `agents.conflict`면 토스트 대신 상세 위 배너로 충돌 파일과 [기준 반영으로 에이전트에게 맡기기]를 보인다.
- 기준 반영:
  - 결과 `conflict`이면 배너를 띄운다. "충돌 파일 n개 · 에이전트에게 해결을 맡길까요?" + [해결 지시 보내기][되돌리기(rebase_abort)].
  - [해결 지시 보내기]는 "기준 브랜치 위로 rebase하다 충돌이 났습니다. 충돌 파일: …. 충돌을 해결하고 `git rebase --continue`까지 마쳐 주세요."를 `agents.send`로 보낸다.
- PR: 제목·본문 대화상자(기본 제목 = 작업 제목, 본문 = 지시문 + 커밋 목록). 성공하면 `prUrl`을 열 수 있는 토스트를 띄운다.
- 폐기: 먼저 `agents.discard_check`를 부르고 확인 문구를 만든다. "병합하지 않은 커밋 n개와 커밋하지 않은 파일 m개가 사라집니다." 둘 다 0이면 "작업 폴더와 브랜치를 지웁니다."다. 버튼은 "작업 폴더와 브랜치 삭제"다. worktree 없는 작업은 "세션 끝내기"다.
- **확인한 상태 보내기(IR-11):** 병합·PR은 확인 대화상자를 열 때의 `agents.changes`의 `{ headOid, baseOid }`를, 폐기는 `agents.discard_check`의 `{ headOid, unmergedCommits }`를 `expect`로 보낸다. 오류 `agents.state_changed`면 실행하지 않은 것이다. 대화상자를 닫지 않고 "확인한 뒤 작업이 바뀌었습니다. 바뀐 내용을 확인하고 다시 실행하세요."를 보이며, 변경·커밋 조회를 무효화하고 새 값으로 확인 문구를 다시 만든다. 오류 문구는 `messages.ts`의 `agents.state_changed` 항목에 둔다.
- 테스트 실행: `agents.test` → 반환된 터미널 ID를 실행 탭에 띄운다. `[agent] test`가 없으면 버튼을 비활성화하고 툴팁 "`.devbox/devbox.toml`의 `[agent] test`를 설정하세요"를 단다. 이를 위해 `projects.config`를 조회한다.
- 재개: `agents.resume` → 상태가 `running`이 되면 터미널 탭으로 간다.
- 검토 시작: `agents.review`.

탭:
- **터미널:** `<TerminalView id={task.terminalId} />`. 상단 오른쪽에 [새 창으로](`openPopout("agent", id)`).
- **변경:**
  - 왼쪽은 파일 목록이다(`agents.changes`; 상태 글자 A/M/D/R, +/− 수, 봤음 체크). 오른쪽은 `DiffView`다(`agents.patch`).
  - 위에 "기준 `<base>` 대비 · 앞섬 n · 뒤처짐 m"을 보인다.
  - 줄을 고르면 [지시에 인용]이 뜨고, `quoteForPrompt`를 작성란 초안 끝에 붙인다.
  - [봤음]을 누르면 다음 안 본 파일로 이동한다.
- **커밋:** `agents.commits` 목록(짧은 SHA·제목·시각). 누르면 그 커밋의 변경을 보인다. 이 화면은 S4에서 소스 섹션 공용 화면으로 바뀐다. S1에서는 목록만 보인다.
- **실행:** 마지막 `agents.test` 터미널. 없으면 EmptyState와 [테스트 실행].

머리줄: 제목 · 프로젝트 · 브랜치(`PathLabel` 대신 텍스트) · 도구 · 상태(`StatusDot` + `STATE_LABEL`) · 경과 · CPU · 메모리 · 토큰(`agents.usage`, 닫힌 작업은 고정값) · 뒤처짐. 닫힌 작업은 머리줄 아래에 "닫힘 · 병합됨/PR/폐기 · 시각"과 PR 링크를 보인다.

`agents.changed` 이벤트가 오면 `agents.get{id}`·`agents.changes{id}`·`agents.commits{id}`를 무효화한다(`useTopicInvalidation`).

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/ui src/features/agents && pnpm --filter app typecheck
git add app/src && git commit -m "feat(app): review, instruct, rebase, merge and discard agent tasks from the detail view"
```

---

### Task 19: 격자 보기·알림 센터·에이전트 E2E

**Files:**
- Create: `app/src/features/agents/AgentGrid.tsx`, `app/src/features/notifications/{store,NotificationCenter,AttentionBridge}.tsx|ts`
- Modify: `app/src/shell/TitleBar.tsx`(🔔), `app/src/App.tsx`(`AttentionBridge`, 분리 창 경로 `/popout/agent/$id`·`/popout/agents-grid`)
- Modify: `app/e2e/support.ts`·`global-setup.ts`(가짜 도구 프로필·git 저장소·`DEVBOX_NO_SCOPE`. 임시 HOME에 `.gitconfig`(`user.name devbox-test`, `user.email test@devbox.invalid`, `init.defaultBranch main`)를 써서 데몬의 병합 커밋이 신원 없이 실패하지 않게 한다. 04 Task 8의 `TestDaemon`과 같은 내용)
- Create: `app/e2e/agents.spec.ts`
- Test: `app/src/features/notifications/store.test.ts`

**Interfaces:**
- Consumes: 주제 `agents.attention { id, state, message }`, Task 15 `TerminalView(readOnly, scale)`
- Produces:
  - `useNotifications`(Zustand, 최근 100개, `localStorage` 유지)
    - 상태: `items: { key, id, kind: "agent"|"error", title, body, atMs, target?: { to, search } , read }[]`, `unread`
    - `push(n)`: 같은 `key`(예: `agent:<id>`)가 있으면 그 항목을 바꿔 맨 위로 올리고 읽지 않음으로 만든다.
    - `markAllRead()`, `clear()`
  - `AttentionBridge`(메인 창에서만 렌더):
    - `agents.attention`을 구독해 알림 센터에 넣는다.
    - Tauri면 `notify_attention`·`set_badge` 명령을 부른다(Task 22). 브라우저면 `document.title` 앞에 `(n) `을 붙인다.
  - `AgentGrid({ tasks, onOpen })`: 칸마다 읽기 전용 `TerminalView`(`scale` = 칸 너비 / 1000px)와 제목·상태
    - 칸은 `<button aria-label="<제목> 터미널 미리보기">`로 감싼 덮개를 가진다(터미널 위 클릭을 가로챔). 누르면 상세
    - 칸 수: 1–4개는 2열, 5–9개는 3열, 10개 이상은 4열 + 세로 스크롤
    - [새 창으로]는 `openPopout("agents-grid")`
  - 분리 창: `/popout/agent/$id`(상세 전체), `/popout/agents-grid`

- [ ] **Step 1: 실패하는 테스트(알림 묶기)**

```ts
// app/src/features/notifications/store.test.ts
import { useNotifications } from "./store";

beforeEach(() => useNotifications.getState().clear());

test("the same agent replaces its notification and stays unread", () => {
  const n = useNotifications.getState();
  n.push({ key: "agent:a1", kind: "agent", title: "로그인 · 입력 대기", body: "needs Bash", target: { to: "/agents", search: { id: "a1" } } });
  n.push({ key: "agent:a2", kind: "agent", title: "검색 · 턴 끝", body: "" });
  n.push({ key: "agent:a1", kind: "agent", title: "로그인 · 다음 지시 대기", body: "done" });
  const s = useNotifications.getState();
  expect(s.items.map((x) => x.key)).toEqual(["agent:a1", "agent:a2"]);
  expect(s.items[0].title).toBe("로그인 · 다음 지시 대기");
  expect(s.unread).toBe(2);
  s.markAllRead();
  expect(useNotifications.getState().unread).toBe(0);
});

test("keeps at most 100 notifications", () => {
  for (let i = 0; i < 120; i++) useNotifications.getState().push({ key: `e${i}`, kind: "error", title: `오류 ${i}`, body: "" });
  expect(useNotifications.getState().items).toHaveLength(100);
  expect(useNotifications.getState().items[0].key).toBe("e119");
});
```

- [ ] **Step 2: 구현**

```ts
// app/src/features/notifications/store.ts
import { create } from "zustand";
import { persist } from "zustand/middleware";

export interface Notice {
  key: string;
  kind: "agent" | "error";
  title: string;
  body: string;
  atMs: number;
  read: boolean;
  target?: { to: string; search?: Record<string, string> };
}

interface Store {
  items: Notice[];
  unread: number;
  push(n: Omit<Notice, "atMs" | "read">): void;
  markAllRead(): void;
  clear(): void;
}

const count = (items: Notice[]) => items.filter((x) => !x.read).length;

export const useNotifications = create<Store>()(
  persist(
    (set, get) => ({
      items: [],
      unread: 0,
      push: (n) => {
        const items = [{ ...n, atMs: Date.now(), read: false }, ...get().items.filter((x) => x.key !== n.key)].slice(0, 100);
        set({ items, unread: count(items) });
      },
      markAllRead: () => {
        const items = get().items.map((x) => ({ ...x, read: true }));
        set({ items, unread: 0 });
      },
      clear: () => set({ items: [], unread: 0 }),
    }),
    { name: "devbox.notifications" },
  ),
);
```

```tsx
// app/src/features/notifications/AttentionBridge.tsx
import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useRpc } from "../../rpc/RpcProvider";
import { attentionCount, STATE_LABEL, type AgentTask } from "../agents/model";
import { useNotifications } from "./store";
import { nativeAttention, nativeBadge } from "./native";

export function AttentionBridge() {
  const client = useRpc();
  const qc = useQueryClient();
  useEffect(
    () =>
      client.subscribe(
        "agents.attention",
        (_rev, p) => {
          const list = qc.getQueryData<{ items: AgentTask[] }>(["agents.list", { includeClosedHours: 24 }]);
          const task = list?.items.find((t) => t.id === p.id);
          const title = `${task?.title ?? "에이전트 작업"} · ${STATE_LABEL[p.state]}`;
          useNotifications.getState().push({ key: `agent:${p.id}`, kind: "agent", title, body: p.message, target: { to: "/agents", search: { id: p.id } } });
          void nativeAttention({ id: p.id, title, body: p.message });
        },
        () => {},
      ),
    [client, qc],
  );
  // 배지는 목록 캐시에서 계산한다(입력 대기 작업 수).
  useEffect(
    () =>
      qc.getQueryCache().subscribe(() => {
        const list = qc.getQueryData<{ items: AgentTask[] }>(["agents.list", { includeClosedHours: 24 }]);
        if (list) void nativeBadge(attentionCount(list.items), list.items.filter((t) => t.state !== "closed").length);
      }),
    [qc],
  );
  return null;
}
```

`native.ts`:
- Tauri 안이면 `invoke("notify_attention", { a: { id, title, body, visible } })`·`invoke("set_badge", { attention, open })`를 부른다(Task 22가 명령을 만든다).
- 그 전까지와 브라우저에서는 `document.title = attention ? \`(${attention}) devbox\` : "devbox"`만 한다.
- 같은 값이면 다시 부르지 않는다(마지막 값 기억).

`NotificationCenter`는 제목 표시줄 🔔 버튼(`aria-label="알림 n개"`, 읽지 않은 수)의 Popover다.
- 내용: 읽지 않은 수 배지, 항목(제목·본문·상대 시각), 누르면 `target`으로 이동, [모두 읽음][비우기].
- 오류 토스트도 `kind: "error"`로 보관한다(`useToasts.push`가 danger면 알림 센터에도 넣도록 Task 13의 `push`를 고친다).

`AttentionBridge`는 `App.tsx`에서 `!location.pathname.startsWith("/popout/")`일 때만 렌더한다.

- [ ] **Step 3: E2E 준비**

`app/e2e/support.ts`의 `makeEnv()`에 다음을 추가한다.
- `projects/gamma`를 실제 git 저장소로 만든다(`git init -b main`, 파일 하나 커밋, `user.name/email`은 저장소 로컬 설정).
- `~/.config/devbox/config.toml`에 가짜 도구 프로필을 쓴다.

```toml
[[agents.profiles]]
id = "fake"
label = "Fake"
command = "bash <home>/fake-tool.sh"
hooks = "none"
```

- `<home>/fake-tool.sh`는 04 Task 9의 `COMMITTING_TOOL` 앞에 권한 요청 단계를 더한 스크립트다(`SessionStart` → `Notification(permission_prompt, "needs Bash")` → `read` → 파일 커밋 → `Stop` → `sleep 600`).
- `startDaemon`의 env에 `DEVBOX_NO_SCOPE: "1"`을 더한다.

- [ ] **Step 4: E2E**

```ts
// app/e2e/agents.spec.ts
import { expect, test } from "@playwright/test";

const e2e = () => JSON.parse(process.env.DEVBOX_E2E ?? "{}");

async function addGamma(page: import("@playwright/test").Page) {
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가" }).click();
  await page.getByRole("button", { name: /gamma/ }).click();
}

test("new task → waiting within 2s of the hook → instruct → idle → squash merge closes it", async ({ page }) => {
  await page.goto(`/agents?gateway=${encodeURIComponent(e2e().gatewayUrl)}&renderer=dom`);
  await addGamma(page);
  await page.keyboard.press("Control+Shift+N");
  await page.getByLabel("제목").fill("Fix login");
  await page.getByRole("combobox", { name: "도구" }).click();
  await page.getByRole("option", { name: "Fake" }).click();
  await page.getByLabel("지시문").fill("fix it");
  const started = Date.now();
  await page.getByRole("button", { name: "시작" }).click();
  const group = page.getByRole("region", { name: "입력 대기" });
  await expect(group.getByText("Fix login")).toBeVisible({ timeout: 10_000 });
  await expect(group.getByText("needs Bash")).toBeVisible();
  expect(Date.now() - started).toBeLessThan(10_000);
  await expect(page.getByRole("button", { name: "알림 1개" })).toBeVisible();

  await group.getByText("Fix login").click();
  await page.getByLabel("에이전트에게 보낼 지시").fill("yes");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByText("다음 지시 대기")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("tab", { name: /변경/ }).click();
  await expect(page.getByText("agent.txt")).toBeVisible();
  await page.getByRole("button", { name: "병합" }).click();
  await page.getByRole("menuitem", { name: "커밋 하나로 합쳐 병합" }).click();
  await page.getByRole("button", { name: /에 병합/ }).click();
  await expect(page.getByRole("region", { name: "닫힘(24시간)" }).getByText("Fix login")).toBeVisible({ timeout: 10_000 });
});

test("Ctrl+Shift+J jumps to the next task waiting for input", async ({ page }) => {
  await page.goto(`/terminal?gateway=${encodeURIComponent(e2e().gatewayUrl)}&renderer=dom`);
  await page.keyboard.press("Control+Shift+J");
  await expect(page).toHaveURL(/\/agents\?id=/);
});

test("grid view shows live read-only tiles", async ({ page }) => {
  await page.goto(`/agents?view=grid&gateway=${encodeURIComponent(e2e().gatewayUrl)}&renderer=dom`);
  const tiles = page.getByRole("button", { name: /터미널 미리보기/ });
  await expect(tiles.first()).toBeVisible({ timeout: 10_000 });
  await tiles.first().click();
  await expect(page).toHaveURL(/view=list/);
});
```

- 두 번째·세 번째 시험은 첫 시험의 작업(이미 닫힘)에 기대지 않는다. `describe` 안의 `beforeAll`이 gateway WebSocket으로 직접 작업을 만든다. 이 보조 함수를 `support.ts`에 둔다.

```ts
// app/e2e/support.ts 에 추가 — 화면 없이 RPC 한 번(프레임은 화면 코드의 frame.ts를 그대로 씀)
import { decodeFrame, jsonFrame } from "../src/rpc/frame";

export async function rpc<T = unknown>(method: string, params: unknown): Promise<T> {
  const { gatewayUrl, instance } = JSON.parse(process.env.DEVBOX_E2E ?? "{}");
  const ws = new WebSocket(gatewayUrl);
  ws.binaryType = "arraybuffer";
  await new Promise((ok, fail) => {
    ws.onopen = ok;
    ws.onerror = fail;
  });
  const waiting: ((m: { type: string; result?: T; error?: { code: string } }) => void)[] = [];
  ws.onmessage = (e) => waiting.shift()?.(JSON.parse(new TextDecoder().decode(decodeFrame(new Uint8Array(e.data as ArrayBuffer)).body)));
  const next = () => new Promise<{ type: string; result?: T; error?: { code: string } }>((r) => waiting.push(r));
  // hello 모양은 S0b client.ts가 보내는 것과 같다.
  ws.send(jsonFrame({ type: "hello", version: "1.0.0-dev", protocol: 1, client: "test", instance }));
  if ((await next()).type !== "welcome") throw new Error("no welcome");
  ws.send(jsonFrame({ type: "request", id: 1, method, params }));
  const res = await next();
  ws.close();
  if (res.error) throw new Error(res.error.code);
  return res.result as T;
}

export async function gammaProjectId(home: string): Promise<string> {
  const list = await rpc<{ items: { id: string; path: string }[] }>("projects.list", {});
  const found = list.items.find((p) => p.path.endsWith("/projects/gamma"));
  if (found) return found.id;
  return (await rpc<{ id: string }>("projects.add", { path: `${home}/projects/gamma` })).id;
}
```

두 번째·세 번째 시험은 이렇게 감싼다.

```ts
test.describe("with a task waiting for input", () => {
  test.beforeAll(async () => {
    const projectId = await gammaProjectId(e2e().home);
    const t = await rpc<{ id: string }>("agents.create", { projectId, title: "Waiting one", profile: "fake", baseBranch: null, worktree: true, prompt: "x" });
    await expect.poll(async () => (await rpc<{ state: string }>("agents.get", { id: t.id })).state, { timeout: 10_000 }).toBe("waiting");
  });
  // 위의 "Ctrl+Shift+J …"·"grid view …" 시험 두 개
});
```
- SC3의 2초 기준은 hook 이벤트 수신부터 목록 반영까지다. 위 시험은 전체 생성 흐름(worktree·tmux·bash 시작 포함)을 10초로 잡고, hook → 목록 구간은 데몬 통합 시험(04 Task 8)에서 잰다(Task 24에서 수치 기록).

- [ ] **Step 5: 통과·커밋·묶음 L 끝**

```bash
pnpm --filter app exec vitest run src/features && pnpm --filter app exec playwright test e2e/agents.spec.ts
git add app && git commit -m "feat(app): add the agent grid, notification center and agent end-to-end journeys"
# 묶음 L 끝: PROGRESS.md의 묶음 L 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 20: 프로젝트 전환기 카드·시작 구성·clone

**Files:**
- Modify: `app/src/features/projects/ProjectSwitcher.tsx`, `AddProjectDialog.tsx`
- Create: `app/src/features/projects/ProjectCard.tsx`, `app/e2e/launch.spec.ts`
- Test: `app/src/features/projects/ProjectSwitcher.test.tsx`(추가 시험)

**Interfaces:**
- Consumes: 04 Task 11 `projects.summary`·`projects.launch`·`projects.clone`·`projects.config`
- Produces:
  - 전환기 행 = `ProjectCard`: 이름·경로·브랜치·변경 여부·앞섬/뒤처짐·터미널 수·열린 에이전트(입력 대기 수)
    - 카드 오른쪽 [시작 구성 실행]은 `[launch]`가 있을 때만 보인다.
    - 설정 파일 오류가 있으면 카드에 경고 아이콘 + 툴팁으로 오류 문구를 보인다.
  - [시작 구성 실행]: `projects.launch` 뒤 그 프로젝트로 전환하고 터미널 섹션으로 이동한다. 결과 토스트는 "터미널 n개 · 에이전트 m개를 시작했습니다"다.
  - 카드의 별 버튼은 즐겨찾기(`projects.set_favorite`)다. 즐겨찾기가 위로 온다.
  - 카드 메뉴의 외부 열기: [VS Code로 열기](`code --remote wsl+<배포판> <경로>`) · [탐색기로 열기](`\\wsl.localhost\<배포판>\<경로>`) · [Windows Terminal로 열기](`wt.exe -p <배포판> -d <경로>`). Tauri 명령 `open_external({ kind, path })`가 Windows 쪽에서 실행하고, 브라우저 개발 화면에서는 메뉴를 숨긴다.
  - 프로젝트 추가 대화상자에 [Git 주소로 가져오기] 탭: URL + 대상 폴더(기본 `~/projects/<이름>`) → `projects.clone`(진행 중 표시, 실패 시 git 메시지)

- [ ] **Step 1: 실패하는 테스트(카드 정보와 시작 구성)**

```tsx
// app/src/features/projects/ProjectSwitcher.test.tsx 에 추가
test("cards show git and agent summary and run the launch profile", async () => {
  const client = fakeClient({
    "projects.list": { rev: "3", items: [{ id: "a", name: "devbox", path: "/home/u/projects/devbox", favorite: false, addedMs: 1 }] },
    "projects.summary": { items: [{ id: "a", branch: "main", dirty: true, ahead: 2, behind: 0, terminals: 3, agents: { open: 2, attention: 1 } }] },
    "projects.config": { config: { agent: {}, launch: { services: [], terminals: [{ name: "shell" }], agents: [] } }, error: null },
    "projects.launch": { terminals: 1, agents: 0 },
  });
  render(<RpcProvider client={client}><ProjectSwitcher open onOpenChange={() => {}} /></RpcProvider>);
  expect(await screen.findByText("main")).toBeInTheDocument();
  expect(screen.getByText("변경 있음")).toBeInTheDocument();
  expect(screen.getByText("앞섬 2")).toBeInTheDocument();
  expect(screen.getByText("에이전트 2 · 입력 대기 1")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "devbox 시작 구성 실행" }));
  await waitFor(() => expect(client.call).toHaveBeenCalledWith("projects.launch", { id: "a" }, expect.anything()));
  expect(useCurrentProject.getState().id).toBe("a");
});
```

`projects.launch`의 결과 타입은 04 Task 11의 `LaunchResult { terminals, agents }`다.

- [ ] **Step 2: 구현** — 위 Interfaces대로 `ProjectCard`를 만들고 `ProjectSwitcher`의 행을 바꾼다.
  - `projects.summary`는 전환기가 열릴 때만 조회한다(`enabled: open`).
  - `projects.config`는 카드가 보일 때 프로젝트마다 조회한다(`enabled: open`, 결과는 30초 캐시).
  - `AddProjectDialog`에 `Tabs`(폴더에서 · Git 주소로)를 두고, 두 번째 탭은 `projects.clone`을 부른다.

- [ ] **Step 3: E2E(시작 구성)**

```ts
// app/e2e/launch.spec.ts
import { expect, test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

test("launch profile opens its terminals in one action", async ({ page }) => {
  const { gatewayUrl, home } = JSON.parse(process.env.DEVBOX_E2E ?? "{}");
  mkdirSync(join(home, "projects", "gamma", ".devbox"), { recursive: true });
  writeFileSync(join(home, "projects", "gamma", ".devbox", "devbox.toml"), '[launch]\nterminals = [{ name = "web" }, { name = "logs" }]\n');
  await page.goto(`/agents?gateway=${encodeURIComponent(gatewayUrl)}&renderer=dom`);
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "gamma 시작 구성 실행" }).click();
  await expect(page).toHaveURL(/\/terminal/);
  await expect(page.getByText("web")).toBeVisible();
  await expect(page.getByText("logs")).toBeVisible();
});
```

`gamma`가 아직 등록되지 않았으면 이 시험의 `beforeAll`에서 `rpc("projects.add", { path })`로 등록한다(Task 19의 `support.ts` 보조 함수).

- [ ] **Step 4: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/features/projects && pnpm --filter app exec playwright test e2e/launch.spec.ts
git add app && git commit -m "feat(app): show project cards with git and agent summary and run launch profiles"
```

---

### Task 21: 설정 — 에이전트·셸 환경 다시 읽기

**Files:**
- Modify: `crates/cli/src/doctor.rs`(도구 점검 추가), `crates/protocol/src/system.rs`(`system.refresh_env` 선언), `crates/cli/src/daemon/routes.rs`(등록)
- Create: `crates/cli/src/daemon/system.rs`(`system.refresh_env` 처리: `setup`의 환경 잡기를 다시 실행한 뒤 데몬 재시작)
- Create: `app/src/features/settings/{AgentsSettings,GeneralSettings}.tsx`
- Modify: `app/src/features/settings/SettingsPage.tsx`(탭: 일반·에이전트·앱 진단·정보)
- Test: `crates/cli/src/doctor.rs`(단위), `app/src/features/settings/AgentsSettings.test.tsx`

**Interfaces:**
- Produces:
  - doctor 점검 추가(데몬 환경 기준 `which`):
    - `tool.claude`·`tool.codex`: 없으면 경고 "PATH에서 찾지 못했습니다 · 설치했다면 [셸 환경 다시 읽기]"
    - `tool.gh`: 없으면 경고 "PR 만들기를 쓰려면 설치"
    - `tool.tmux`: 3.2 미만이면 오류(S0b `tmux_at_least` 재사용)
    - `hook.binary`: `~/.local/share/devbox/bin/devbox`가 실행 가능한지
  - `system.refresh_env{}`(mutation)
    1. S0b `setup::capture_env`로 셸 환경을 다시 읽어 `~/.config/devbox/<i>/env`에 쓴다.
    2. `{ restarting: true }`를 돌려준 뒤 1초 뒤 데몬을 끝낸다. systemd가 다시 띄운다.
    3. tmux·실행은 별도 unit이라 유지된다(SC4).
    4. 테스트 인스턴스와 `--foreground`에서는 다시 쓰기만 하고 끝내지 않는다(`restarting: false`).
  - 설정 › 에이전트 탭:
    - 도구 프로필 목록(`agents.profiles`, 사용자 정의는 `config.toml`에서 편집한다는 안내와 [설정 파일 위치 복사])
    - 점검 결과(`system.doctor`에서 `tool.*`·`hook.*`만)
    - [셸 환경 다시 읽기]
    - 정리 후보(`agents.cleanup_candidates`, 현재 프로젝트) + [선택 항목 정리](확인 `always`)
  - 설정 › 일반 탭: 테마(시스템·다크·라이트), UI 배율(90–150%), 터미널 글꼴 크기, 시작 섹션
  - `openSettings(tab: "general"|"agents"|"shortcuts"|"diagnostics"|"about")`(`app/src/features/settings/open.ts`): `useSettingsUi`(Zustand)에 탭을 두고 `/settings`로 이동한다. Task 17의 [연결 확인] 배너 버튼을 `openSettings("agents")`로 바꾼다.

- [ ] **Step 1: 실패하는 테스트(Rust, doctor)**

```rust
// crates/cli/src/doctor.rs 의 tests에 추가
#[test]
fn missing_agent_tools_are_warnings_not_errors() {
    let probe = FakeProbe { which: vec!["tmux".into()], tmux_version: Some("tmux 3.4".into()), ..Default::default() };
    let checks = checks(&probe);
    let get = |id: &str| checks.iter().find(|c| c.id == id).unwrap();
    assert_eq!(get("tool.claude").status, Status::Warn);
    assert_eq!(get("tool.gh").status, Status::Warn);
    assert_eq!(get("tool.tmux").status, Status::Ok);
}
```

S0b `Probe` 트레잇에 `fn which(&self, name: &str) -> bool`이 없으면 추가한다. 실제 구현은 `which::which(name).is_ok()`이고, `FakeProbe`는 `which` 목록으로 답한다.

- [ ] **Step 2: 실패하는 테스트(화면)**

```tsx
// app/src/features/settings/AgentsSettings.test.tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { fakeClient } from "../../test/fakeClient";
import { AgentsSettings } from "./AgentsSettings";

test("shows tool checks and refreshes the shell environment", async () => {
  const c = fakeClient({
    "agents.profiles": { items: [{ id: "claude", label: "Claude Code", command: "claude", hooks: "claude", waitingPatterns: [] }] },
    "system.doctor": { checks: [{ id: "tool.claude", status: "warn", title: "claude 명령", detail: "PATH에서 찾지 못했습니다" }, { id: "units", status: "ok", title: "unit", detail: "" }] },
    "agents.cleanup_candidates": { worktrees: [], branches: [] },
    "system.refresh_env": { restarting: true },
  });
  render(<RpcProvider client={c}><AgentsSettings /></RpcProvider>);
  expect(await screen.findByText("PATH에서 찾지 못했습니다")).toBeInTheDocument();
  expect(screen.queryByText("unit")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "셸 환경 다시 읽기" }));
  await waitFor(() => expect(c.call).toHaveBeenCalledWith("system.refresh_env", {}, expect.anything()));
  expect(await screen.findByText(/데몬을 다시 시작합니다/)).toBeInTheDocument();
});
```

S0b의 `Check` 필드 이름(`id`·`status`·`title`·`detail`)이 다르면 그 이름에 맞춘다.

- [ ] **Step 3: 구현·통과·커밋·묶음 M 끝**

```bash
cargo test -p devbox-cli doctor && cargo run -q -p xtask -- gen-ts
pnpm --filter app exec vitest run src/features/settings && pnpm --filter app typecheck
git add crates xtask app && git commit -m "feat(settings): check agent tools and reload the shell environment"
# 묶음 M 끝: PROGRESS.md의 묶음 M 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

---

### Task 22: Windows 알림·딥 링크·작업 표시줄 배지 (`crates/win`)

**Files:**
- Create: `crates/win/Cargo.toml`, `crates/win/src/lib.rs`, `crates/win/src/toast.rs`, `crates/win/src/taskbar.rs`
- Modify: `Cargo.toml`(workspace members에 `crates/win`)
- Modify: `app/src-tauri/Cargo.toml`(`devbox-win`, `tauri-plugin-deep-link`, single-instance `deep-link` 기능), `app/src-tauri/tauri.conf.json`(`plugins.deep-link.desktop.schemes = ["devbox"]`), `app/src-tauri/src/lib.rs`, `app/src-tauri/src/notify.rs`(신규), `app/src-tauri/capabilities/default.json`
- Create: `app/src/features/notifications/native.ts`(Tauri에서만 토스트·배지 명령을 부르고 브라우저에서는 아무것도 하지 않음)
- Modify: `app/src/App.tsx`(`devbox://navigate` 수신)
- Modify: `.github/workflows/windows.yml`(paths에 `crates/win/**`)
- Test: `crates/win/src/toast.rs`(XML 생성 단위 — Linux에서도 실행), Windows CI의 `cargo test -p devbox-win`

**Interfaces:**
- Produces:
  - `devbox_win::toast::xml(ToastSpec { title, body, launch_uri, actions: [(label, uri)] }) -> String`(이스케이프 포함, 플랫폼 무관)
  - `devbox_win::toast::tag_for(agent_id) -> String`(64자 이내)
  - `#[cfg(windows)] devbox_win::toast::show(aumid, spec, tag, group) -> windows::core::Result<()>`: 같은 tag는 교체된다.
  - `#[cfg(windows)] devbox_win::taskbar::set_overlay(hwnd, count)`: 0이면 지운다. 1–9는 숫자, 10 이상은 "9+" 아이콘(실행 중 GDI로 그림).
  - `#[cfg(windows)] devbox_win::taskbar::flash(hwnd)`: `FlashWindowEx(FLASHW_TRAY | FLASHW_TIMERNOFG)`. 창이 앞에 있으면 하지 않는다.
  - Tauri 명령:
    - `notify_attention({ id, title, body })`
      - 메인 창이 앞에 있고 같은 작업 화면을 보고 있으면 토스트를 띄우지 않는다. 화면 코드가 `visible` 플래그를 함께 넘긴다.
      - 그 밖에는 토스트(`devbox://agents/<id>`, 버튼 [열기])를 띄우고 깜빡인다.
    - `set_badge({ attention, open })`: 작업 표시줄 배지는 `attention`, 트레이 요약(Task 23)은 둘 다 쓴다.
  - 딥 링크: `devbox://agents/<id>` → 기존 인스턴스가 `devbox://navigate` 이벤트(`{ to: "/agents", search: { id } }`)를 메인 창에 보내고 창을 앞으로 가져온다.
  - 개발 실행(설치 안 된 AUMID)에서는 토스트가 실패할 수 있다. 실패하면 깜빡임만 하고, 한 번만 로그에 남긴다.

- [ ] **Step 1: 실패하는 테스트(플랫폼 무관)**

```rust
// crates/win/src/toast.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_xml_escapes_text_and_uses_protocol_activation() {
        let x = xml(&ToastSpec {
            title: "로그인 <수정> & 테스트".into(),
            body: "needs \"Bash\"".into(),
            launch_uri: "devbox://agents/a1".into(),
            actions: vec![("열기".into(), "devbox://agents/a1".into())],
        });
        assert!(x.contains(r#"<toast activationType="protocol" launch="devbox://agents/a1""#));
        assert!(x.contains("로그인 &lt;수정&gt; &amp; 테스트"));
        assert!(x.contains("needs &quot;Bash&quot;"));
        assert!(x.contains(r#"<action content="열기" activationType="protocol" arguments="devbox://agents/a1"/>"#));
    }

    #[test]
    fn tags_are_short_and_stable() {
        assert_eq!(tag_for("a1b2"), "agent-a1b2");
        assert!(tag_for(&"x".repeat(100)).len() <= 64);
    }
}
```

```rust
// app/src-tauri/src/notify.rs 의 순수 함수 테스트
#[cfg(test)]
mod tests {
    #[test]
    fn deep_links_map_to_routes() {
        assert_eq!(super::route_for("devbox://agents/a1"), Some(("/agents".to_string(), Some("a1".to_string()))));
        assert_eq!(super::route_for("devbox://agents/"), None);
        assert_eq!(super::route_for("https://evil/agents/a1"), None);
    }
}
```

- [ ] **Step 2: 구현 — 플랫폼 무관 부분**

```rust
// crates/win/src/toast.rs (테스트 위)
pub struct ToastSpec {
    pub title: String,
    pub body: String,
    pub launch_uri: String,
    pub actions: Vec<(String, String)>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub fn xml(s: &ToastSpec) -> String {
    let actions: String = s.actions.iter().map(|(l, u)| format!(r#"<action content="{}" activationType="protocol" arguments="{}"/>"#, esc(l), esc(u))).collect();
    format!(
        r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{}</actions></toast>"#,
        esc(&s.launch_uri),
        esc(&s.title),
        esc(&s.body),
        actions
    )
}

pub fn tag_for(agent_id: &str) -> String {
    let mut t = format!("agent-{agent_id}");
    t.truncate(64);
    t
}

#[cfg(windows)]
pub fn show(aumid: &str, spec: &ToastSpec, tag: &str, group: &str) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml(spec)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(tag))?;
    toast.SetGroup(&HSTRING::from(group))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Show(&toast)
}
```

```toml
# crates/win/Cargo.toml
[package]
name = "devbox-win"
version.workspace = true
edition.workspace = true
publish = false

[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = ["Data_Xml_Dom", "UI_Notifications", "Win32_Foundation", "Win32_UI_WindowsAndMessaging", "Win32_UI_Shell", "Win32_Graphics_Gdi", "Win32_System_Com"] }
```

`taskbar.rs`:
- `ITaskbarList3::SetOverlayIcon(hwnd, icon, description)`을 쓴다. 아이콘은 16×16 원 위에 숫자를 GDI로 그려 `CreateIconIndirect`로 만든다. `description`은 "입력 대기 n개"다.
- `flash`는 `FlashWindowEx`다. 창이 앞에 있는지(`GetForegroundWindow() == hwnd`)를 먼저 본다.
- 모두 `#[cfg(windows)]`이고, Linux에서는 빈 모듈이다.

AUMID는 설치본의 바로가기에 Tauri NSIS가 넣는 identifier(`io.github.jihoon22lee.devbox`)를 쓴다. S0a·S0b 실사용에서 실제 값을 확인하고 다르면 상수를 고친다.

- [ ] **Step 3: 구현 — Tauri 연결**

```rust
// app/src-tauri/src/notify.rs
use tauri::{AppHandle, Emitter, Manager};

pub fn route_for(uri: &str) -> Option<(String, Option<String>)> {
    let rest = uri.strip_prefix("devbox://")?;
    let mut parts = rest.trim_end_matches('/').splitn(2, '/');
    match (parts.next()?, parts.next()) {
        ("agents", Some(id)) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) => Some(("/agents".into(), Some(id.into()))),
        _ => None,
    }
}

pub fn handle_deep_link(app: &AppHandle, uri: &str) {
    if let Some((to, id)) = route_for(uri) {
        crate::window::focus_main(app);
        let _ = app.emit_to("main", "devbox://navigate", serde_json::json!({ "to": to, "search": { "id": id } }));
    }
}

#[derive(serde::Deserialize)]
pub struct Attention {
    id: String,
    title: String,
    body: String,
    visible: bool,
}

#[tauri::command]
pub fn notify_attention(app: AppHandle, a: Attention) {
    let Some(w) = app.get_webview_window("main") else { return };
    if a.visible && w.is_focused().unwrap_or(false) {
        return;
    }
    #[cfg(windows)]
    {
        let uri = format!("devbox://agents/{}", a.id);
        let spec = devbox_win::toast::ToastSpec { title: a.title, body: a.body, launch_uri: uri.clone(), actions: vec![("열기".into(), uri)] };
        if devbox_win::toast::show(&app.config().identifier, &spec, &devbox_win::toast::tag_for(&a.id), "agents").is_err() {
            log_once("toast unavailable (dev build without registered AUMID)");
        }
        if let Ok(hwnd) = w.hwnd() {
            devbox_win::taskbar::flash(hwnd.0 as _);
        }
    }
    #[cfg(not(windows))]
    let _ = (a.title, a.body);
}

#[tauri::command]
pub fn set_badge(app: AppHandle, attention: u32, open: u32) {
    crate::tray::set_summary(&app, open, attention); // Task 23에서 추가. 그 전에는 이 줄 없이 둔다.
    #[cfg(windows)]
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            devbox_win::taskbar::set_overlay(hwnd.0 as _, attention);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, attention, open);
}

#[cfg(windows)]
fn log_once(msg: &str) {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| eprintln!("devbox: {msg}"));
}
```

`lib.rs` 변경:
- `tauri_plugin_deep_link::init()`을 추가한다.
- single-instance 콜백에서 `argv`의 `devbox://` 인자를 `notify::handle_deep_link`로 넘긴다. 처음 실행된 프로세스의 인자도 `setup`에서 같은 함수로 처리한다.
- 개발 실행에서는 `app.deep_link().register_all()`로 스킴을 등록한다(설치본은 NSIS가 등록).
- invoke 목록에 `notify::notify_attention`, `notify::set_badge`를 추가한다.

화면 쪽 변경:
- `native.ts`의 `nativeAttention`은 `visible: document.visibilityState === "visible" && 현재 경로가 /agents?id=<id>`를 함께 넘긴다.
- `App.tsx`는 `listen("devbox://navigate", e => router.navigate(e.payload))`를 메인 창에서만 등록한다.

- [ ] **Step 4: 통과·커밋**

```bash
cargo test -p devbox-win
git add crates/win Cargo.toml app && git commit -m "feat(app): show Windows toasts with deep links, taskbar badges and window flashing"
```

Windows 쪽 컴파일과 시험은 PR의 Windows 워크플로(`crates/win/**` 경로)에서 돈다.

---

### Task 23: 트레이·전역 단축키·자동 시작·닫기 동작

**Files:**
- Create: `app/src-tauri/src/tray.rs`, `app/src-tauri/src/shortcuts.rs`
- Modify: `app/src-tauri/src/lib.rs`, `app/src-tauri/src/settings.rs`(앱 설정 파일에 `autostart_asked`·`tray_notice_shown`·`global_shortcuts`), `app/src-tauri/Cargo.toml`(`tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, tauri `tray-icon` 기능), `app/src-tauri/capabilities/default.json`
- Create: `app/src/features/settings/ShortcutsSettings.tsx`, `app/src/shell/FirstRun.tsx`
- Test: `app/src-tauri/src/shortcuts.rs`(단축키 문자열 해석 단위), `app/src/shell/FirstRun.test.tsx`

**Interfaces:**
- Produces:
  - 트레이 메뉴:
    - 항목: "devbox 열기" · "에이전트 n · 입력 대기 m"(누르면 에이전트 섹션) · "명령 팔레트" · 구분선 · "종료"
    - 왼쪽 클릭은 열기다. 요약 줄은 `set_badge`가 올 때 같이 갱신한다.
    - "종료"를 누를 때 열린 에이전트 작업(또는 S2부터 실행 중인 서비스)이 있으면 메인 창에 확인 대화상자를 띄운다: "에이전트 n개가 실행 중입니다. 앱을 끄면 WSL이 약 15초 뒤 멈추면서 함께 종료될 수 있습니다." [종료][취소](01-design §4.1).
  - 창 닫기(×):
    - 메인 창은 숨긴다(`prevent_close` + `hide`).
    - 처음 한 번은 트레이 알림 토스트 "트레이에서 계속 실행됩니다 · 완전히 끄려면 트레이 메뉴의 종료"를 띄운다.
    - 분리 창은 그냥 닫힌다.
  - 전역 단축키(`tauri-plugin-global-shortcut`):
    - 팔레트: 설치본은 Ctrl+Alt+Space, 개발 빌드(`cfg!(debug_assertions)`)는 Ctrl+Alt+Shift+Space다.
    - 누르면 메인 창을 앞으로 가져오고 `devbox://palette` 이벤트를 보낸다.
    - 등록 실패는 `shortcut_status()` 명령으로 화면에 알린다(설정 › 단축키에 빨간 표시와 "다른 프로그램이 쓰는 중").
    - 빠른 캡처(Ctrl+Alt+N)는 S3에서 추가한다.
    - 터미널 빠른 호출: 설정에서 단축키를 지정하면(기본 없음) 같은 방식으로 등록한다. 누르면 메인 창을 앞으로 가져오고 `devbox://bottom-terminal` 이벤트로 하단 패널을 열어 터미널에 포커스한다. 이미 앞에 있고 하단 패널이 열려 있으면 창을 숨긴다(드롭다운 터미널처럼 토글).
  - `parse_accelerator("Ctrl+Alt+Space") -> Result<Shortcut, String>`(설정 값 검증)
  - 자동 시작: `tauri-plugin-autostart`
    - 첫 실행 화면(`FirstRun`)에서 "Windows 시작 시 devbox 실행"(기본 켬)을 묻는다.
    - 답을 앱 설정에 기록한다(다시 묻지 않음). 설정 › 일반에서 바꿀 수 있다.
  - 첫 실행 화면은 게이트가 `ready`가 된 뒤 한 번만 보인다. 자동 시작과 `instanceIdleTimeout` 안내를 담는다. 트레이가 브리지를 붙잡고 있어 WSL이 유휴로 꺼지지 않는다는 설명이다(01-design §14 R1).

- [ ] **Step 1: 실패하는 테스트**

```rust
// app/src-tauri/src/shortcuts.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects_accelerators() {
        assert!(parse_accelerator("Ctrl+Alt+Space").is_ok());
        assert!(parse_accelerator("Ctrl+Alt+Shift+Space").is_ok());
        assert!(parse_accelerator("Space").is_err(), "a global shortcut needs a modifier");
        assert!(parse_accelerator("Ctrl+Alt+Nope").is_err());
    }

    #[test]
    fn dev_builds_use_shift_variants() {
        assert_eq!(default_palette(true), "Ctrl+Alt+Shift+Space");
        assert_eq!(default_palette(false), "Ctrl+Alt+Space");
    }
}
```

```tsx
// app/src/shell/FirstRun.test.tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FirstRun } from "./FirstRun";

test("asks once about autostart with it on by default", async () => {
  const done = vi.fn();
  render(<FirstRun onDone={done} />);
  const box = screen.getByRole("checkbox", { name: "Windows 시작 시 devbox 실행" });
  expect(box).toBeChecked();
  await userEvent.click(box);
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  expect(done).toHaveBeenCalledWith({ autostart: false });
});
```

- [ ] **Step 2: 구현 요점**

```rust
// app/src-tauri/src/shortcuts.rs (테스트 위)
use tauri_plugin_global_shortcut::Shortcut;

pub fn default_palette(debug: bool) -> &'static str {
    if debug { "Ctrl+Alt+Shift+Space" } else { "Ctrl+Alt+Space" }
}

pub fn parse_accelerator(s: &str) -> Result<Shortcut, String> {
    if !s.contains('+') {
        return Err("전역 단축키에는 Ctrl·Alt·Shift 중 하나가 필요합니다".into());
    }
    s.parse::<Shortcut>().map_err(|e| e.to_string())
}

/// 등록하고, 실패하면 이유를 상태에 남긴다.
pub fn register(app: &tauri::AppHandle, status: &std::sync::Mutex<Vec<(String, String)>>) {
    use tauri::Emitter;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let palette = crate::settings::load(app).palette_shortcut.unwrap_or_else(|| default_palette(cfg!(debug_assertions)).into());
    let result = parse_accelerator(&palette).and_then(|sc| {
        app.global_shortcut()
            .on_shortcut(sc, |app, _, e| {
                if e.state == ShortcutState::Pressed {
                    crate::window::focus_main(app);
                    let _ = app.emit_to("main", "devbox://palette", ());
                }
            })
            .map_err(|e| e.to_string())
    });
    if let Err(reason) = result {
        status.lock().expect("shortcut status").push((palette, reason));
    }
}
```

`tray.rs`:
- `TrayIconBuilder`에 메뉴(`MenuBuilder`)를 붙인다.
- `set_summary(app, open, attention)`가 요약 항목 문구를 바꾼다. `notify::set_badge`에서 부른다(Task 22 코드의 주석 줄을 이때 켠다).
- "종료"는 `app.exit(0)`이다. 브리지 자식은 Job Object로 함께 끝난다(S0b).

`lib.rs`:
- `on_window_event`에서 `CloseRequested`이고 창 이름이 `main`이면 `api.prevent_close()` + `hide()`, 그리고 첫 번째면 안내 토스트를 띄운다(Windows는 `devbox_win::toast::show`, 개발 실행은 무시).
- `.plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))`를 추가한다. `--minimized`로 시작하면 메인 창을 보이지 않고 트레이만 띄운다(`window::configure_main`에서 `show()` 생략).

화면:
- `FirstRun`은 `ConnectionGate`가 `ready`가 된 뒤 앱 설정(`get_app_settings` 명령)의 `firstRunDone`이 거짓이면 띄운다. 끝나면 `set_autostart(enabled)`·`finish_first_run` 명령을 부른다.
- `App.tsx`는 `listen("devbox://palette", () => setPalette(true))`를 등록한다.
- 설정 › 단축키는 앱 단축키 표(레지스트리에서 생성, 읽기 전용)와 전역 단축키(팔레트 입력란 + 상태)를 보인다. 변경하면 `set_palette_shortcut` → 재등록이고, 결과를 표시한다.

- [ ] **Step 3: 통과·커밋·묶음 N 끝**

```bash
pnpm --filter app exec vitest run src/shell/FirstRun.test.tsx && pnpm --filter app typecheck
git add app && git commit -m "feat(app): add tray, global palette shortcut, autostart and close-to-tray"
# 묶음 N 끝: PROGRESS.md의 묶음 N 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s1-agents-terminal
```

Windows 워크플로가 통과하면 머지한다. Rust Windows 코드의 실제 동작은 Task 24의 실사용 확인에서 본다.

---

### Task 24: S1 완료 — 성공 기준 측정·규모 통계·실사용 확인

**Files:**
- Create: `crates/cli/tests/survival.rs`(SC4), `crates/cli/tests/terminal_latency.rs`(SC10), `scripts/loc.sh`(SC8)
- Modify: `crates/cli/tests/agents.rs`(SC3 시간 측정 출력)
- Modify: `04-s1-agents-terminal.md`·`04b-s1-app.md`(상태 줄), `PROGRESS.md`(S1 완료·PR 행·현재 위치)

**Interfaces:**
- Produces:
  - `survival.rs`: 데몬을 재시작해도 터미널 세션·에이전트 작업이 유지되는지 확인한다(SC4의 L2 부분). 확인 항목: `terminal.list`의 `alive`, `agents.get`의 상태, attach 기록.
  - `scripts/loc.sh`: 제품 코드 줄 수를 Rust와 TS로 나눠 보이고, 테스트·생성 파일은 뺀다. S1 예산 6만 줄과 비교해 출력한다(게이트 아님).
  - SC3: hook → `agents.changed` 수신까지 걸린 시간을 통합 시험에서 재고 `eprintln!`으로 남긴다. 2초를 넘으면 실패한다.

- [ ] **Step 1: SC4 통합 시험**

```rust
// crates/cli/tests/survival.rs
mod support;
use std::time::Duration;
use support::TestDaemon;

#[tokio::test]
async fn sessions_and_agent_tasks_survive_a_daemon_restart() {
    let mut d = TestDaemon::start_with_env(&[("DEVBOX_NO_SCOPE", "1")]);
    d.write_config("[[agents.profiles]]\nid = \"sleepy\"\nlabel = \"S\"\ncommand = \"sleep 600\"\nhooks = \"none\"\n");
    let repo = d.git_repo("projects/keep");
    let c = d.client().await;
    let term: serde_json::Value = serde_json::from_str(&c.raw_call("terminal.create", serde_json::json!({ "cwd": d.home.path(), "command": "bash --norc" }), None).await.unwrap()).unwrap();
    let p = d.add_project(&c, &repo).await;
    let agent = d.create_agent(&c, &p, "sleepy", true).await;
    d.wait_for(&c, &agent, "running", Duration::from_secs(10)).await;

    d.restart();
    let c = d.client().await;
    let list: serde_json::Value = serde_json::from_str(&c.raw_call("terminal.list", serde_json::json!({}), None).await.unwrap()).unwrap();
    assert!(list["items"].as_array().unwrap().iter().any(|s| s["id"] == term["id"] && s["alive"] == true));
    let a: serde_json::Value = serde_json::from_str(&c.raw_call("agents.get", serde_json::json!({ "id": agent }), None).await.unwrap()).unwrap();
    assert_eq!(a["state"], "running", "a live tmux session must not be marked lost");
}
```

`TestDaemon::restart`는 데몬 프로세스만 다시 띄우고 tmux 서버(`-L devbox-<test 인스턴스>`)는 그대로 둔다. S0b 구현이 tmux까지 끄면 Drop에서만 끄도록 고친다.

- [ ] **Step 2: SC3 측정**

04 Task 8의 `agent_task_goes_from_preparing_to_waiting_to_idle_via_hooks`에 다음을 추가한다.
1. `agents.changed`를 구독한다.
2. 가짜 도구가 `Notification`을 보내기 직전 시각(밀리초)을 테스트 HOME 안의 `hook-sent.ts` 파일에 쓰게 한다.
3. 이벤트 수신 시각과의 차이를 `eprintln!("SC3 hook→event {ms}ms")`로 남긴다.
4. `assert!(ms < 2000)`.

Windows 알림까지의 구간은 실사용 확인 3번에서 본다.

- [ ] **Step 2b: SC10 터미널 반응 측정 (IR-6)**

데몬·PTY·tmux를 거친 입력 왕복과, 한 터미널이 출력을 쏟는 동안 다른 연결의 응답 지연을 잰다. 브리지 구간은 S0a 결과(`09-s0a-results.md`)의 64B p95를 더해 판정한다.

```rust
// crates/cli/tests/terminal_latency.rs
mod support;
use std::time::{Duration, Instant};
use support::TestDaemon;

fn pct(v: &mut Vec<f64>, p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

#[tokio::test]
async fn input_echo_p95_and_ping_during_flood_meet_sc10() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let s: serde_json::Value = serde_json::from_str(
        &c.raw_call("terminal.create", serde_json::json!({ "cwd": d.home.path(), "command": "sh -c 'stty raw -echo; cat'" }), None).await.unwrap(),
    ).unwrap();
    let a = c.open_stream("terminal.attach", serde_json::json!({ "id": s["id"], "cols": 120, "rows": 40, "readOnly": false })).await.unwrap();
    a.credit(1 << 22).await;
    let _ = a.read_available(Duration::from_millis(500)).await;

    let marks = ["가", "나", "다", "라"];
    let mut echo = Vec::new();
    for i in 0..200 {
        let m = marks[i % marks.len()].as_bytes();
        let t0 = Instant::now();
        a.write(m).await;
        a.read_until(m, Duration::from_secs(1)).await;
        echo.push(t0.elapsed().as_secs_f64() * 1000.0);
        a.credit(1 << 16).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (p50, p95) = (pct(&mut echo.clone(), 0.5), pct(&mut echo, 0.95));
    eprintln!("SC10 echo p50={p50:.2}ms p95={p95:.2}ms (브리지 제외)");

    // 다른 세션이 100MB를 출력하는 동안, 그 출력을 읽는 화면이 느려도(신용을 조금만 줌) 다른 연결의 ping은 막히지 않아야 한다.
    let f: serde_json::Value = serde_json::from_str(
        &c.raw_call("terminal.create", serde_json::json!({ "cwd": d.home.path(), "command": "sh -c 'yes x | head -c 100000000; sleep 60'" }), None).await.unwrap(),
    ).unwrap();
    let flood = c.open_stream("terminal.attach", serde_json::json!({ "id": f["id"], "cols": 120, "rows": 40, "readOnly": true })).await.unwrap();
    flood.credit(1 << 18).await;
    let other = d.client().await;
    let mut ping = Vec::new();
    for _ in 0..100 {
        let t0 = Instant::now();
        other.raw_call("system.ping", serde_json::json!({}), None).await.unwrap();
        ping.push(t0.elapsed().as_secs_f64() * 1000.0);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ping95 = pct(&mut ping, 0.95);
    eprintln!("SC10 ping during flood p95={ping95:.2}ms");

    assert!(p50 <= 10.0 && p95 <= 25.0, "echo p50={p50} p95={p95}");
    assert!(ping95 <= 100.0, "ping p95 during flood={ping95}");
}
```

판정이 실패하면 시험을 느슨하게 고치지 않는다. 원인(PTY 읽기 크기, 송신 큐 공정성, tmux 다시 그리기)을 찾아 04 Task 1·4를 고친다.

- [ ] **Step 3: 규모 통계**

```bash
# scripts/loc.sh
#!/usr/bin/env bash
# 제품 코드 줄 수(SC8 관찰용, 게이트 아님). 테스트·생성 파일·E2E는 뺀다.
set -euo pipefail
cd "$(dirname "$0")/.."
budget=${1:-60000}
rust=0
while IFS= read -r f; do
  # 파일 끝의 #[cfg(test)] 모듈부터는 세지 않는다.
  n=$(awk '/^#\[cfg\(test\)\]/{exit} {c++} END{print c+0}' "$f")
  rust=$((rust + n))
done < <(git ls-files 'crates/*.rs' 'app/src-tauri/src/*.rs' 'xtask/*.rs' | grep -v -e '/tests/' -e '/benches/')
ts=$(git ls-files 'app/src/*.ts' 'app/src/*.tsx' | grep -v -e '\.test\.' -e '/gen/' -e '^app/src/test/' | xargs -r cat | wc -l)
total=$((rust + ts))
echo "rust=$rust ts=$ts total=$total budget=$budget"
if [ "$total" -gt "$budget" ]; then echo "예산 초과: 다음 하위 프로젝트 전에 단순화를 검토하세요"; fi
```

`bash scripts/loc.sh 60000` 결과를 PR 본문에 적는다. S3 끝에는 `120000`, S6 끝에는 `200000`을 인자로 준다.

- [ ] **Step 4: 실사용 확인(사용자, 10개)**

WSL에서 `bash scripts/dogfood.sh v1/s1-agents-terminal`로 설치 파일을 받아 설치한 뒤 확인한다(브랜치를 push해 두면 스크립트가 그 커밋의 `windows.yml` 실행을 찾거나 새로 시작한다. Windows 쪽 빌드 도구가 있으면 `scripts/win-build.ps1`도 된다).

1. 새 에이전트 작업(Claude Code, 작업 폴더 켬)을 만들면 1번의 [시작]으로 작업 폴더·세션·도구 실행까지 이어진다.
2. Claude가 권한을 물으면 2초 안에 목록 "입력 대기"에 메시지가 보인다.
3. 같은 때 Windows 토스트가 뜨고, 누르면 그 작업 상세가 열린다. 작업 표시줄 아이콘에 숫자 배지가 붙는다.
4. 작성란에서 한글로 지시를 쓰고 Ctrl+Enter → 에이전트가 받아 진행한다(조합 중 글자 누락 없음).
5. 에이전트 두 개를 동시에 돌리고 격자 보기에서 둘 다 실시간으로 보인다.
6. 변경 탭에서 줄을 인용해 지시하고, squash 병합 → 기준 체크아웃에 반영되고 작업 폴더가 정리된다.
7. 앱 창을 닫았다(트레이) 다시 열어도 터미널·에이전트 세션이 그대로다. `systemctl --user restart devbox@prod`를 해도 그대로다.
8. 터미널 탭·분할을 만들고 하나를 새 창으로 띄운다. 새 창을 닫아도 세션은 목록에 남는다.
9. 트레이 상주 중 Ctrl+Alt+Space로 팔레트가 뜬다(개발 빌드면 Ctrl+Alt+Shift+Space).
10. 프로젝트 전환기 카드에서 [시작 구성 실행]으로 터미널들이 한 번에 열린다.

- [ ] **Step 5: 상태 갱신·PR**

```bash
git add crates scripts && git commit -m "test: measure S1 success criteria and add a code size report"
sed -i 's/^- 상태: 계획 · 미착수 · 시작 조건: S0b 완료.*/- 상태: 완료(S1 PR 머지)/' docs/superpowers/plans/2026-10-08-devbox-v1/04-s1-agents-terminal.md
sed -i 's/^- 상태: 계획 · 미착수 · 시작 조건: \[04-s1-agents-terminal\].*/- 상태: 완료(S1 PR 머지, 실사용 확인 10\/10)/' docs/superpowers/plans/2026-10-08-devbox-v1/04b-s1-app.md
git add docs && git commit -m "docs(plan): mark S1 complete"
git push origin v1/s1-agents-terminal && gh pr create --base main --head v1/s1-agents-terminal --title "feat: agents, terminal and projects (S1)" --body "S1 에이전트·터미널·프로젝트 전체(04·04b Task 1–24). SC3(hook→목록 n ms)·SC4(데몬 재시작 유지)·SC10(입력 왕복 p50/p95, 출력 폭주 중 ping) 수치, 규모 통계(rust=…, ts=…), 실사용 확인 10개 결과.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

CI·사용자 확인 뒤 `gh pr merge --rebase v1/s1-agents-terminal`, worktree·브랜치 정리. 사용자에게 S1 완료와 측정값을 보고하고 S2를 시작한다.
