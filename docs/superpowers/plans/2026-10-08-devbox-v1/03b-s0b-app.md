# S0b 걷는 뼈대 (2/2: 화면 골격·Windows 껍데기) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획 · 미착수 · 시작 조건: [03-s0b-skeleton](03-s0b-skeleton.md)의 PR A·B·C 머지

**Goal:** React 화면 골격(프레임·라우팅·RPC 클라이언트·상태 갱신 규칙·연결 표시·명령 팔레트·프로젝트 전환기)과 Windows Tauri 껍데기(WSL 확인·바이너리 배치·setup·브리지·창별 채널)를 만든다. 같은 화면이 브라우저(dev-gateway)와 Tauri 양쪽에서 실제 데몬에 붙어 `system.ping`·`projects.*`가 동작하게 한다.

**Architecture:**
- `app/`는 Vite + React 19 한 벌이다. `app/src/rpc`가 프레임 인코딩, 클라이언트, 전송 두 가지(WebSocket·Tauri Channel), TanStack Query 연결을 맡는다.
- `app/src-tauri`는 Windows 전용 Tauri 앱이다.
  - WSL 배포판 확인 → Linux 바이너리 배치(stdin) → `devbox setup` → `wsl.exe … devbox bridge` 자식 프로세스(Job Object)를 거쳐 데몬에 연결한다.
  - 창마다 `ipc::Channel`을 `crates/mux`에 등록해 프레임 헤더만 보고 라우팅한다.

**Tech Stack:** React 19.1, TypeScript 5.8, Vite 7, TanStack Router 1·Query 5, Zustand 5, Radix Primitives, lucide-react, Pretendard, Vitest 4 + Testing Library, Playwright, Tauri 2.12(+single-instance), winreg, windows 0.62, webview2-com.

**Spec:** [01-design.md](01-design.md) §5.4·§5.5·§5.6·§6.1·§6.5·§8.1·§8.3·§8.5·§8.6·§10

## Global Constraints

- [03-s0b-skeleton](03-s0b-skeleton.md)의 Global Constraints를 그대로 따른다.
- 화면 개발 서버 포트는 1450(strictPort), dev-gateway는 1451이다. E2E는 화면 1460·gateway 1461을 쓴다(IR-7).
- 화면 문구는 한국어로 쓴다. 용어는 `docs/ui-terms.md`를 따른다. 단축키 표기는 `Ctrl`이다.
- 생성 파일 `app/src/rpc/gen/rpc.ts`는 손으로 고치지 않는다.
- `app/src-tauri`는 Cargo workspace에서 `exclude`한다. Linux CI가 GTK 없이 workspace를 빌드할 수 있어야 하기 때문이다. `app/src-tauri`는 자체 `Cargo.lock`을 쓰고, `crates/mux`·`crates/protocol`은 path 의존으로 가져온다.
- Tauri identifier는 `io.github.jihoon22lee.devbox`이다. 배포 빌드의 인스턴스는 `prod`, debug 빌드는 `dev`이다.
- 단축키 매칭은 `event.code`로 한다. 터미널·편집기(`data-keyscope="terminal|editor"`) 안에서는 Ctrl+Shift·Ctrl+Alt·Ctrl+숫자·F키만 앱이 가져간다.

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 데몬이 재시작돼 연결이 끊긴 사이에 보낸 요청 | `connection_lost`로 끝나고, 재연결 뒤 구독을 다시 걸고 화면 데이터를 다시 읽음 | Task 17·18·21 |
| 늦게 도착한 조회 결과가 더 새 이벤트 뒤에 옴 | 캐시의 rev가 더 크면 덮어쓰지 않음 | Task 18 |
| 한글 입력 상태에서 단축키 | `event.code`로 같은 동작, 조합 중에는 무시 | Task 19 |
| WSL 오류가 UTF-16LE 문장으로 표준 출력에 나옴 | 프레임으로 해석하지 않고 오류 화면에 문장·코드를 표시 | Task 23 |
| 창을 새로 고침 | 옛 채널을 닫고 새 채널로 다시 연결(구독 중복 없음) | Task 22·23 |

---

## PR 묶음

| PR | 브랜치 | 과제 |
|---|---|---|
| D | `feat/app/skeleton` | Task 16–21 |
| E | `feat/app/windows-shell` | Task 22–24 |

---

### Task 16: 앱 골격, 디자인 토큰, 기본 컴포넌트

**Files:**
- Create: `app/package.json`, `app/tsconfig.json`, `app/vite.config.ts`, `app/index.html`, `app/src/main.tsx`, `app/src/test-setup.ts`
- Create: `app/src/ui/tokens.css`, `app/src/ui/global.css`, `app/src/ui/index.ts`
- Create: `app/src/ui/Button.tsx`·`Button.module.css`, `IconButton.tsx`, `Tooltip.tsx`, `Dialog.tsx`·`Dialog.module.css`, `ConfirmDialog.tsx`, `Banner.tsx`·`Banner.module.css`, `EmptyState.tsx`, `ErrorState.tsx`, `Kbd.tsx`, `StatusDot.tsx`·`StatusDot.module.css`
- Test: `app/src/ui/ui.test.tsx`

**Interfaces:**
- Produces:
  - `Button({ variant?: "primary"|"secondary"|"ghost"|"danger", size?: "sm"|"md", ...button props })`
  - `IconButton({ label, icon, onClick, shortcut? })`: 툴팁과 `aria-label` 포함
  - `Dialog({ open, onOpenChange, title, description?, children, footer? })`
  - `ConfirmDialog({ open, title, body, confirmLabel, tone?, onConfirm, onCancel })`: 기본 포커스는 취소
  - `Banner({ tone: "info"|"warn"|"danger", children, actions? })`
  - `EmptyState({ title, body?, action? })`
  - `ErrorState({ title, action?, diagnosticId? })`: 진단 ID가 있으면 [진단 복사]
  - `Kbd({ keys: string[] })`
  - `StatusDot({ tone: "ok"|"warn"|"danger"|"progress"|"idle", label })`: 색과 모양을 함께 쓴다

- [ ] **Step 0: worktree** — `git worktree add ../devbox-wt/feat-app-skeleton -b feat/app/skeleton origin/main`

- [ ] **Step 1: 패키지 설정**

```json
// app/package.json
{
  "name": "app",
  "private": true,
  "version": "1.0.0-dev",
  "type": "module",
  "scripts": {
    "dev": "vite --port 1450 --strictPort",
    "build": "tsc -b && vite build",
    "typecheck": "tsc -b --noEmit",
    "test": "vitest run",
    "e2e": "playwright test",
    "tauri": "tauri"
  },
  "dependencies": {
    "@radix-ui/react-dialog": "^1.1.15",
    "@radix-ui/react-tooltip": "^1.2.8",
    "@tanstack/react-query": "^5.90.0",
    "@tanstack/react-router": "^1.131.0",
    "@tauri-apps/api": "^2.12.0",
    "lucide-react": "^0.544.0",
    "pretendard": "^1.3.9",
    "react": "^19.1.1",
    "react-dom": "^19.1.1",
    "zustand": "^5.0.8"
  },
  "devDependencies": {
    "@playwright/test": "^1.55.0",
    "@tauri-apps/cli": "^2.12.0",
    "@testing-library/jest-dom": "^6.8.0",
    "@testing-library/react": "^16.3.0",
    "@testing-library/user-event": "^14.6.1",
    "@types/react": "^19.1.12",
    "@types/react-dom": "^19.1.9",
    "@vitejs/plugin-react": "^5.0.2",
    "jsdom": "^27.0.0",
    "typescript": "~5.8.3",
    "vite": "^7.1.5",
    "vitest": "^4.0.0"
  }
}
```

버전은 실행 시점의 최신 호환 버전으로 올려도 된다. `pnpm install` 뒤 잠금 파일을 커밋한다.

```json
// app/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2023", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "types": ["vite/client", "vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src", "e2e", "vite.config.ts", "playwright.config.ts"]
}
```

```ts
// app/vite.config.ts
/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1450, strictPort: true, fs: { allow: [".."] }, watch: { ignored: ["**/src-tauri/**"] } },
  test: { environment: "jsdom", globals: true, setupFiles: ["src/test-setup.ts"], include: ["src/**/*.test.{ts,tsx}"] },
});
```

```ts
// app/src/test-setup.ts
import "@testing-library/jest-dom/vitest";
```

```html
<!-- app/index.html -->
<!doctype html>
<html lang="ko" data-theme="dark">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>devbox</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

```tsx
// app/src/main.tsx (Task 19에서 App으로 바꾼다)
import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./ui/tokens.css";
import "./ui/global.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <p>devbox</p>
  </StrictMode>,
);
```

- [ ] **Step 2: 토큰과 전역 CSS**

```css
/* app/src/ui/tokens.css */
:root {
  --font-sans: "Pretendard Variable", Pretendard, "Segoe UI Variable", system-ui, sans-serif;
  --font-mono: "Cascadia Mono", "D2Coding", Consolas, monospace;
  --text-xs: 12px; --text-sm: 13px; --text-md: 14px; --text-lg: 16px; --text-xl: 20px; --text-2xl: 24px;
  --leading-body: 1.5; --leading-code: 1.4;
  --space-1: 4px; --space-2: 8px; --space-3: 12px; --space-4: 16px; --space-5: 20px; --space-6: 24px; --space-8: 32px;
  --radius-sm: 4px; --radius-md: 6px; --radius-lg: 8px;
  --row-sm: 24px; --row-md: 28px; --row-lg: 32px;
  --control-sm: 24px; --control-md: 28px; --control-lg: 32px;
  --titlebar: 36px; --statusbar: 24px; --activitybar: 48px; --listregion: 260px;
}
:root, [data-theme="dark"] {
  color-scheme: dark;
  --bg-0: #0d1015; --bg-1: #12161c; --bg-2: #181d25; --bg-3: #20262f;
  --surface: #171c23; --border: #283039; --border-strong: #38414d;
  --fg: #e7eaef; --fg-muted: #a1aab7; --fg-subtle: #6d7684;
  --accent: #5b8cff; --accent-fg: #0a0f1d; --selection: color-mix(in srgb, var(--accent) 22%, transparent);
  --ok: #3fb97a; --warn: #e0a43a; --danger: #f0616d; --info: #58b4e6; --progress: #9a7cf0;
  --focus-ring: 0 0 0 2px color-mix(in srgb, var(--accent) 65%, transparent);
  --shadow-1: 0 1px 2px rgb(0 0 0 / 0.45); --shadow-2: 0 10px 28px rgb(0 0 0 / 0.5);
}
[data-theme="light"] {
  color-scheme: light;
  --bg-0: #ffffff; --bg-1: #f6f7f9; --bg-2: #eef0f3; --bg-3: #e3e6eb;
  --surface: #ffffff; --border: #d8dce2; --border-strong: #b9c0ca;
  --fg: #1b1f24; --fg-muted: #4f5866; --fg-subtle: #7a8391;
  --accent: #2f6bf0; --accent-fg: #ffffff; --selection: color-mix(in srgb, var(--accent) 18%, transparent);
  --ok: #1f8a55; --warn: #a96c00; --danger: #c9303d; --info: #1f7fb3; --progress: #6c4fd6;
  --focus-ring: 0 0 0 2px color-mix(in srgb, var(--accent) 55%, transparent);
  --shadow-1: 0 1px 2px rgb(16 24 40 / 0.08); --shadow-2: 0 10px 28px rgb(16 24 40 / 0.16);
}
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation: none !important; transition: none !important; }
}
```

```css
/* app/src/ui/global.css */
* { box-sizing: border-box; }
html, body, #root { height: 100%; margin: 0; }
body {
  background: var(--bg-1); color: var(--fg);
  font: var(--text-sm) / var(--leading-body) var(--font-sans);
  -webkit-font-smoothing: antialiased; overflow: hidden;
}
:focus-visible { outline: none; box-shadow: var(--focus-ring); border-radius: var(--radius-sm); }
::selection { background: var(--selection); }
button, input, select, textarea { font: inherit; color: inherit; }
@media (forced-colors: active) { :focus-visible { outline: 2px solid CanvasText; } }
```

- [ ] **Step 3: 실패하는 컴포넌트 테스트**

```tsx
// app/src/ui/ui.test.tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button, ConfirmDialog, ErrorState, IconButton, StatusDot } from "./index";

test("button runs its handler", async () => {
  const onClick = vi.fn();
  render(<Button onClick={onClick}>저장</Button>);
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  expect(onClick).toHaveBeenCalledOnce();
});

test("icon button is labelled for screen readers", () => {
  render(<IconButton label="새 에이전트 작업" icon={<span>+</span>} onClick={() => {}} />);
  expect(screen.getByRole("button", { name: "새 에이전트 작업" })).toBeInTheDocument();
});

test("status dots differ by shape as well as colour", () => {
  const { container } = render(
    <>
      <StatusDot tone="ok" label="연결됨" />
      <StatusDot tone="danger" label="끊김" />
    </>,
  );
  const shapes = [...container.querySelectorAll("[data-shape]")].map((e) => e.getAttribute("data-shape"));
  expect(shapes).toEqual(["circle", "square"]);
  expect(screen.getByLabelText("끊김")).toBeInTheDocument();
});

test("confirm dialog focuses cancel by default and names the action", () => {
  render(<ConfirmDialog open title="프로젝트 제거" body="목록에서만 지웁니다" confirmLabel="프로젝트 제거" onConfirm={() => {}} onCancel={() => {}} />);
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  expect(screen.getByRole("button", { name: "프로젝트 제거" })).toBeInTheDocument();
});

test("error state offers diagnostic copy only with an id", async () => {
  const write = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText: write } });
  const { rerender } = render(<ErrorState title="저장하지 못했습니다" />);
  expect(screen.queryByRole("button", { name: "진단 복사" })).toBeNull();
  rerender(<ErrorState title="저장하지 못했습니다" diagnosticId="ab12cd34" />);
  await userEvent.click(screen.getByRole("button", { name: "진단 복사" }));
  expect(write).toHaveBeenCalledWith(expect.stringContaining("ab12cd34"));
});
```

- [ ] **Step 4: 실패 확인**

Run: `pnpm install && pnpm --filter app exec vitest run src/ui/ui.test.tsx`
Expected: `./index` 미정의로 실패

- [ ] **Step 5: 컴포넌트 구현**

```tsx
// app/src/ui/Button.tsx
import type { ButtonHTMLAttributes } from "react";
import s from "./Button.module.css";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
};

export function Button({ variant = "secondary", size = "md", className, type = "button", ...rest }: ButtonProps) {
  return <button type={type} className={[s.button, s[variant], s[size], className].filter(Boolean).join(" ")} {...rest} />;
}
```

```css
/* app/src/ui/Button.module.css */
.button { display: inline-flex; align-items: center; gap: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-2); padding: 0 var(--space-3); cursor: pointer; white-space: nowrap; }
.button:hover:not(:disabled) { background: var(--bg-3); }
.button:disabled { opacity: 0.5; cursor: default; }
.sm { height: var(--control-sm); font-size: var(--text-xs); }
.md { height: var(--control-md); }
.primary { background: var(--accent); border-color: var(--accent); color: var(--accent-fg); }
.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 88%, white); }
.ghost { background: transparent; border-color: transparent; }
.danger { background: transparent; border-color: var(--danger); color: var(--danger); }
.secondary {}
```

```tsx
// app/src/ui/Tooltip.tsx
import * as T from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";

export function Tooltip({ content, children }: { content: ReactNode; children: ReactNode }) {
  return (
    <T.Root delayDuration={400}>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content side="right" sideOffset={6} style={{ background: "var(--bg-3)", color: "var(--fg)", padding: "4px 8px", borderRadius: "var(--radius-sm)", fontSize: "var(--text-xs)", boxShadow: "var(--shadow-1)" }}>
          {content}
        </T.Content>
      </T.Portal>
    </T.Root>
  );
}
export const TooltipProvider = T.Provider;
```

```tsx
// app/src/ui/IconButton.tsx
import type { ReactNode } from "react";
import { Button } from "./Button";
import { Kbd } from "./Kbd";
import { Tooltip } from "./Tooltip";

export function IconButton({ label, icon, onClick, shortcut, pressed }: { label: string; icon: ReactNode; onClick: () => void; shortcut?: string[]; pressed?: boolean }) {
  return (
    <Tooltip content={<span>{label} {shortcut && <Kbd keys={shortcut} />}</span>}>
      <Button variant="ghost" aria-label={label} aria-pressed={pressed} onClick={onClick} style={{ width: "var(--control-lg)", height: "var(--control-lg)", padding: 0, justifyContent: "center" }}>
        {icon}
      </Button>
    </Tooltip>
  );
}
```

```tsx
// app/src/ui/Kbd.tsx
export function Kbd({ keys }: { keys: string[] }) {
  return (
    <kbd style={{ fontFamily: "var(--font-sans)", fontSize: "var(--text-xs)", color: "var(--fg-muted)", border: "1px solid var(--border)", borderRadius: "var(--radius-sm)", padding: "0 4px" }}>
      {keys.join("+")}
    </kbd>
  );
}
```

```tsx
// app/src/ui/StatusDot.tsx
import s from "./StatusDot.module.css";

const SHAPE = { ok: "circle", progress: "circle", idle: "ring", warn: "triangle", danger: "square" } as const;

export function StatusDot({ tone, label }: { tone: keyof typeof SHAPE; label: string }) {
  return <span role="img" aria-label={label} className={`${s.dot} ${s[tone]}`} data-shape={SHAPE[tone]} />;
}
```

```css
/* app/src/ui/StatusDot.module.css */
.dot { display: inline-block; width: 8px; height: 8px; flex: none; }
.ok { background: var(--ok); border-radius: 50%; }
.progress { background: var(--progress); border-radius: 50%; }
.idle { border: 2px solid var(--fg-subtle); border-radius: 50%; }
.warn { width: 0; height: 0; border-left: 5px solid transparent; border-right: 5px solid transparent; border-bottom: 9px solid var(--warn); }
.danger { background: var(--danger); border-radius: 1px; }
@media (forced-colors: active) { .dot { forced-color-adjust: none; } }
```

```tsx
// app/src/ui/Dialog.tsx
import * as D from "@radix-ui/react-dialog";
import type { ReactNode } from "react";
import s from "./Dialog.module.css";

export function Dialog(props: { open: boolean; onOpenChange: (o: boolean) => void; title: string; description?: string; children?: ReactNode; footer?: ReactNode; width?: number }) {
  return (
    <D.Root open={props.open} onOpenChange={props.onOpenChange}>
      <D.Portal>
        <D.Overlay className={s.overlay} />
        <D.Content className={s.content} style={{ width: props.width ?? 480 }} aria-describedby={props.description ? undefined : undefined}>
          <D.Title className={s.title}>{props.title}</D.Title>
          {props.description && <D.Description className={s.description}>{props.description}</D.Description>}
          <div className={s.body}>{props.children}</div>
          {props.footer && <div className={s.footer}>{props.footer}</div>}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}
```

```css
/* app/src/ui/Dialog.module.css */
.overlay { position: fixed; inset: 0; background: rgb(0 0 0 / 0.45); }
.content { position: fixed; top: 18%; left: 50%; transform: translateX(-50%); max-width: calc(100vw - 32px); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-lg); box-shadow: var(--shadow-2); padding: var(--space-4); }
.title { margin: 0 0 var(--space-2); font-size: var(--text-lg); font-weight: 600; }
.description { margin: 0 0 var(--space-3); color: var(--fg-muted); }
.body { display: flex; flex-direction: column; gap: var(--space-2); }
.footer { display: flex; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-4); }
```

```tsx
// app/src/ui/ConfirmDialog.tsx
import { useEffect, useRef } from "react";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

export function ConfirmDialog(props: { open: boolean; title: string; body: string; confirmLabel: string; tone?: "danger" | "primary"; onConfirm: () => void; onCancel: () => void }) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (props.open) queueMicrotask(() => cancel.current?.focus());
  }, [props.open]);
  return (
    <Dialog
      open={props.open}
      onOpenChange={(o) => !o && props.onCancel()}
      title={props.title}
      description={props.body}
      footer={
        <>
          <Button ref={cancel} onClick={props.onCancel}>취소</Button>
          <Button variant={props.tone === "primary" ? "primary" : "danger"} onClick={props.onConfirm}>{props.confirmLabel}</Button>
        </>
      }
    />
  );
}
```

`Button`이 `ref`를 받도록 React 19의 `ref` prop 전달을 그대로 쓴다(별도 forwardRef 불필요).

```tsx
// app/src/ui/Banner.tsx
import type { ReactNode } from "react";
import s from "./Banner.module.css";

export function Banner({ tone, children, actions }: { tone: "info" | "warn" | "danger"; children: ReactNode; actions?: ReactNode }) {
  return (
    <div role={tone === "danger" ? "alert" : "status"} className={`${s.banner} ${s[tone]}`}>
      <span className={s.text}>{children}</span>
      {actions && <span className={s.actions}>{actions}</span>}
    </div>
  );
}
```

```css
/* app/src/ui/Banner.module.css */
.banner { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-2) var(--space-4); border-bottom: 1px solid var(--border); }
.info { background: color-mix(in srgb, var(--info) 14%, var(--bg-1)); }
.warn { background: color-mix(in srgb, var(--warn) 16%, var(--bg-1)); }
.danger { background: color-mix(in srgb, var(--danger) 16%, var(--bg-1)); }
.text { flex: 1; }
.actions { display: flex; gap: var(--space-2); }
```

```tsx
// app/src/ui/EmptyState.tsx
import type { ReactNode } from "react";

export function EmptyState({ title, body, action }: { title: string; body?: string; action?: ReactNode }) {
  return (
    <div style={{ display: "grid", placeItems: "center", height: "100%", padding: "var(--space-8)" }}>
      <div style={{ textAlign: "center", maxWidth: 420 }}>
        <p style={{ fontSize: "var(--text-lg)", margin: 0 }}>{title}</p>
        {body && <p style={{ color: "var(--fg-muted)" }}>{body}</p>}
        {action}
      </div>
    </div>
  );
}
```

```tsx
// app/src/ui/ErrorState.tsx
import type { ReactNode } from "react";
import { Button } from "./Button";

export function ErrorState({ title, action, diagnosticId }: { title: string; action?: ReactNode; diagnosticId?: string }) {
  return (
    <div role="alert" style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <p style={{ margin: 0, fontSize: "var(--text-md)" }}>{title}</p>
      <div style={{ display: "flex", gap: "var(--space-2)" }}>
        {action}
        {diagnosticId && (
          <Button onClick={() => void navigator.clipboard.writeText(`devbox 진단 ID: ${diagnosticId}`)}>진단 복사</Button>
        )}
      </div>
    </div>
  );
}
```

```ts
// app/src/ui/index.ts
export { Banner } from "./Banner";
export { Button } from "./Button";
export { ConfirmDialog } from "./ConfirmDialog";
export { Dialog } from "./Dialog";
export { EmptyState } from "./EmptyState";
export { ErrorState } from "./ErrorState";
export { IconButton } from "./IconButton";
export { Kbd } from "./Kbd";
export { StatusDot } from "./StatusDot";
export { Tooltip, TooltipProvider } from "./Tooltip";
```

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/ui/ui.test.tsx
git add app pnpm-lock.yaml && git commit -m "feat(app): scaffold the app with design tokens and base components"
```

---

### Task 17: RPC 프레임·클라이언트·WebSocket 전송·오류 문구

**Files:**
- Create: `app/src/rpc/frame.ts`, `app/src/rpc/client.ts`, `app/src/rpc/transport-ws.ts`, `app/src/rpc/messages.ts`
- Test: `app/src/rpc/frame.test.ts`, `app/src/rpc/client.test.ts`, `app/src/rpc/messages.test.ts`

**Interfaces:**
- Consumes:
  - `app/src/rpc/gen/rpc.ts`의 `Methods`, `Topics`, `METHOD_INFO`, `RPC_ERROR_CODES`, `RpcErrorCode`
  - `crates/protocol/fixtures/envelopes.json`
- Produces:
  - `encodeFrame`, `decodeFrame`, `jsonFrame(msg)`, `FRAME`
  - `interface Transport { connect, close, send, onFrame, onState }`, `type TransportState`
  - `class RpcError { code, detail, diagnosticId }`
  - `class RpcClient`:
    - `start()`, `state`, `onState(cb)`, `onReset(cb)`
    - `call(method, params, { clientRequestId?, signal? })`
    - `subscribe(topic, onEvent(rev, payload), onResync)`
    - `daemonId`, `daemonVersion`
  - `wsTransport(url)`
  - `messageFor(code, detail?) -> { title, action? }`

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/rpc/frame.test.ts
import fixtures from "../../../crates/protocol/fixtures/envelopes.json";
import { decodeFrame, encodeFrame, FRAME, jsonFrame } from "./frame";

test("frames round trip with kind, channel and body", () => {
  const raw = encodeFrame({ kind: FRAME.stream, channel: 7, body: new Uint8Array([1, 2, 255]) });
  expect(new DataView(raw.buffer).getUint32(0, true)).toBe(5 + 3);
  expect(decodeFrame(raw)).toEqual({ kind: FRAME.stream, channel: 7, body: new Uint8Array([1, 2, 255]) });
});

test("json frames carry the shared fixture shapes unchanged", () => {
  const raw = jsonFrame(fixtures.client.hello);
  const f = decodeFrame(raw);
  expect(f.kind).toBe(FRAME.json);
  expect(JSON.parse(new TextDecoder().decode(f.body))).toEqual(fixtures.client.hello);
});

test("short or zero-length frames are rejected", () => {
  expect(() => decodeFrame(new Uint8Array([0, 0, 0, 0]))).toThrow();
  expect(() => decodeFrame(new Uint8Array([9, 0, 0, 0, 0]))).toThrow();
});
```

```ts
// app/src/rpc/client.test.ts
import fixtures from "../../../crates/protocol/fixtures/envelopes.json";
import { RpcClient, RpcError, type Transport, type TransportState } from "./client";
import { decodeFrame, jsonFrame } from "./frame";

class FakeTransport implements Transport {
  sent: unknown[] = [];
  private frameCb?: (f: Uint8Array) => void;
  private stateCb?: (s: TransportState) => void;
  connect() { this.stateCb?.("open"); }
  close() {}
  send(f: Uint8Array) { this.sent.push(JSON.parse(new TextDecoder().decode(decodeFrame(f).body))); }
  onFrame(cb: (f: Uint8Array) => void) { this.frameCb = cb; return () => {}; }
  onState(cb: (s: TransportState) => void) { this.stateCb = cb; return () => {}; }
  deliver(msg: unknown) { this.frameCb?.(jsonFrame(msg)); }
  drop() { this.stateCb?.("closed"); }
  reopen() { this.stateCb?.("open"); }
}

function setup() {
  const t = new FakeTransport();
  const c = new RpcClient(t, { version: "1.0.0-dev", instance: "test", client: "test" });
  c.start();
  t.deliver({ ...fixtures.server.welcome, daemonId: "d-1" });
  return { t, c };
}

test("hello is sent on open and the client becomes ready on welcome", () => {
  const { t, c } = setup();
  expect(t.sent[0]).toMatchObject({ type: "hello", protocol: 1, client: "test" });
  expect(c.state).toBe("ready");
  expect(c.daemonId).toBe("d-1");
});

test("calls resolve with results and reject with typed errors", async () => {
  const { t, c } = setup();
  const ok = c.call("system.ping", {});
  const req = t.sent.at(-1) as { id: number };
  t.deliver({ type: "response", id: req.id, result: { daemonId: "d-1", version: "x", uptimeMs: 1 } });
  await expect(ok).resolves.toMatchObject({ daemonId: "d-1" });

  const bad = c.call("projects.add", { path: "rel" });
  const req2 = t.sent.at(-1) as { id: number };
  t.deliver({ type: "response", id: req2.id, error: { code: "projects.path_invalid", detail: { path: "rel" } } });
  await expect(bad).rejects.toMatchObject({ code: "projects.path_invalid", detail: { path: "rel" } });
});

test("a dropped connection rejects pending calls and resubscribes after reconnect", async () => {
  const { t, c } = setup();
  const events: string[] = [];
  let resyncs = 0;
  c.subscribe("projects.changed", (rev) => events.push(rev), () => resyncs++);
  const sub1 = t.sent.at(-1) as { id: number };
  t.deliver({ type: "subscribed", id: sub1.id, subId: 11, rev: "5" });
  t.deliver({ type: "event", subId: 11, rev: "6", payload: { rev: "6" } });
  expect(events).toEqual(["6"]);

  const pending = c.call("projects.list", {});
  t.drop();
  await expect(pending).rejects.toBeInstanceOf(RpcError);
  await expect(pending).rejects.toMatchObject({ code: "connection_lost" });
  expect(c.state).toBe("reconnecting");

  t.reopen();
  t.deliver({ ...fixtures.server.welcome, daemonId: "d-1" });
  const resub = t.sent.at(-1) as { type: string; topic: string };
  expect(resub).toMatchObject({ type: "subscribe", topic: "projects.changed" });
  expect(resyncs).toBe(1);
});

test("a different daemon id after reconnect triggers a reset", () => {
  const { t, c } = setup();
  const reset = vi.fn();
  c.onReset(reset);
  t.drop();
  t.reopen();
  t.deliver({ ...fixtures.server.welcome, daemonId: "d-2" });
  expect(reset).toHaveBeenCalledOnce();
});

test("a version mismatch is reported as a state", () => {
  const t = new FakeTransport();
  const c = new RpcClient(t, { version: "1.0.0-dev", instance: "test", client: "test" });
  c.start();
  t.deliver({ ...fixtures.server.welcome, version: "0.0.1" });
  expect(c.state).toBe("versionMismatch");
});
```

```ts
// app/src/rpc/messages.test.ts
import { RPC_ERROR_CODES } from "./gen/rpc";
import { messageFor } from "./messages";

test("every generated error code has a Korean message", () => {
  for (const code of RPC_ERROR_CODES) {
    const m = messageFor(code, undefined);
    expect(m.title.length).toBeGreaterThan(0);
    expect(m.title).not.toMatch(/[A-Za-z_]{6,}\./);
  }
  expect(messageFor("connection_lost", undefined).title).toContain("결과");
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/rpc`

- [ ] **Step 3: 구현 — frame.ts**

```ts
// app/src/rpc/frame.ts
export const FRAME = { json: 0, stream: 1, control: 2 } as const;
export type FrameKind = (typeof FRAME)[keyof typeof FRAME];
export interface Frame {
  kind: FrameKind;
  channel: number;
  body: Uint8Array;
}

const encoder = new TextEncoder();

export function encodeFrame(f: Frame): Uint8Array {
  const out = new Uint8Array(9 + f.body.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, 5 + f.body.length, true);
  v.setUint8(4, f.kind);
  v.setUint32(5, f.channel, true);
  out.set(f.body, 9);
  return out;
}

export function decodeFrame(raw: Uint8Array): Frame {
  if (raw.length < 9) throw new Error("frame shorter than header");
  const v = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  const len = v.getUint32(0, true);
  if (len < 5 || len + 4 !== raw.length) throw new Error(`bad frame length ${len}`);
  const kind = v.getUint8(4);
  if (kind > 2) throw new Error(`unknown frame kind ${kind}`);
  return { kind: kind as FrameKind, channel: v.getUint32(5, true), body: raw.slice(9) };
}

export function jsonFrame(msg: unknown, kind: FrameKind = FRAME.json): Uint8Array {
  return encodeFrame({ kind, channel: 0, body: encoder.encode(JSON.stringify(msg)) });
}
```

- [ ] **Step 4: 구현 — client.ts**

```ts
// app/src/rpc/client.ts
import { decodeFrame, FRAME, jsonFrame } from "./frame";
import type { Methods, RpcErrorCode, Topics } from "./gen/rpc";

export type TransportState = "connecting" | "open" | "closed";
export interface Transport {
  connect(): void;
  close(): void;
  send(frame: Uint8Array): void;
  onFrame(cb: (frame: Uint8Array) => void): () => void;
  onState(cb: (s: TransportState) => void): () => void;
}
export type ClientState = "connecting" | "ready" | "reconnecting" | "versionMismatch";
export type ErrorCode = RpcErrorCode | "connection_lost";

export class RpcError extends Error {
  constructor(
    readonly code: ErrorCode,
    readonly detail?: unknown,
    readonly diagnosticId?: string,
  ) {
    super(code);
  }
}

type Pending = { resolve: (v: unknown) => void; reject: (e: RpcError) => void; subKey?: number };
type Sub = { topic: string; onEvent: (rev: string, payload: unknown) => void; onResync: () => void; subId?: number };
type ServerMsg =
  | { type: "welcome"; version: string; protocol: number; daemonId: string }
  | { type: "response"; id: number; result?: unknown; error?: { code: RpcErrorCode; detail?: unknown }; diagnosticId?: string }
  | { type: "subscribed"; id: number; subId: number; rev: string }
  | { type: "event"; subId: number; rev: string; payload: unknown }
  | { type: "resync"; subId: number };

const decoder = new TextDecoder();

export class RpcClient {
  state: ClientState = "connecting";
  daemonId?: string;
  daemonVersion?: string;
  private nextId = 1;
  private nextSubKey = 1;
  private pending = new Map<number, Pending>();
  private subs = new Map<number, Sub>();
  private bySubId = new Map<number, number>();
  private stateCbs = new Set<(s: ClientState) => void>();
  private resetCbs = new Set<() => void>();
  private everReady = false;

  constructor(
    private transport: Transport,
    private opts: { version: string; instance: string; client: "app" | "dev" | "test" },
  ) {}

  start(): void {
    this.transport.onState((s) => {
      if (s === "open") this.send({ type: "hello", version: this.opts.version, protocol: 1, client: this.opts.client, instance: this.opts.instance });
      if (s === "closed") this.onClosed();
    });
    this.transport.onFrame((raw) => this.onFrame(raw));
    this.transport.connect();
  }

  onState(cb: (s: ClientState) => void): () => void {
    this.stateCbs.add(cb);
    return () => this.stateCbs.delete(cb);
  }
  onReset(cb: () => void): () => void {
    this.resetCbs.add(cb);
    return () => this.resetCbs.delete(cb);
  }

  call<M extends keyof Methods>(method: M, params: Methods[M]["params"], opts: { clientRequestId?: string; signal?: AbortSignal } = {}): Promise<Methods[M]["result"]> {
    if (this.state !== "ready") return Promise.reject(new RpcError("connection_lost"));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      opts.signal?.addEventListener("abort", () => this.send({ type: "cancel", id }), { once: true });
      this.send({ type: "request", id, method, params, ...(opts.clientRequestId ? { clientRequestId: opts.clientRequestId } : {}) });
    });
  }

  subscribe<T extends keyof Topics>(topic: T, onEvent: (rev: string, payload: Topics[T]) => void, onResync: () => void): () => void {
    const key = this.nextSubKey++;
    this.subs.set(key, { topic, onEvent: onEvent as Sub["onEvent"], onResync });
    if (this.state === "ready") this.sendSubscribe(key);
    return () => {
      const s = this.subs.get(key);
      this.subs.delete(key);
      if (s?.subId !== undefined) {
        this.bySubId.delete(s.subId);
        this.send({ type: "unsubscribe", subId: s.subId });
      }
    };
  }

  private sendSubscribe(key: number) {
    const sub = this.subs.get(key);
    if (!sub) return;
    const id = this.nextId++;
    this.pending.set(id, { resolve: () => {}, reject: () => {}, subKey: key });
    this.send({ type: "subscribe", id, topic: sub.topic });
  }

  private send(msg: unknown) {
    this.transport.send(jsonFrame(msg));
  }

  private setState(s: ClientState) {
    this.state = s;
    for (const cb of this.stateCbs) cb(s);
  }

  private onClosed() {
    for (const p of this.pending.values()) p.reject(new RpcError("connection_lost"));
    this.pending.clear();
    this.bySubId.clear();
    for (const s of this.subs.values()) s.subId = undefined;
    if (this.state !== "versionMismatch") this.setState("reconnecting");
  }

  private onFrame(raw: Uint8Array) {
    const f = decodeFrame(raw);
    if (f.kind !== FRAME.json) return;
    const msg = JSON.parse(decoder.decode(f.body)) as ServerMsg;
    switch (msg.type) {
      case "welcome": {
        this.daemonVersion = msg.version;
        if (msg.version !== this.opts.version) {
          this.setState("versionMismatch");
          return;
        }
        const changed = this.daemonId !== undefined && this.daemonId !== msg.daemonId;
        this.daemonId = msg.daemonId;
        if (changed) for (const cb of this.resetCbs) cb();
        this.setState("ready");
        for (const key of this.subs.keys()) this.sendSubscribe(key);
        if (this.everReady) for (const s of this.subs.values()) s.onResync();
        this.everReady = true;
        return;
      }
      case "response": {
        const p = this.pending.get(msg.id);
        if (!p) return;
        this.pending.delete(msg.id);
        if (msg.error) p.reject(new RpcError(msg.error.code, msg.error.detail, msg.diagnosticId));
        else p.resolve(msg.result);
        return;
      }
      case "subscribed": {
        const p = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        const sub = p?.subKey !== undefined ? this.subs.get(p.subKey) : undefined;
        if (sub && p?.subKey !== undefined) {
          sub.subId = msg.subId;
          this.bySubId.set(msg.subId, p.subKey);
        }
        return;
      }
      case "event": {
        const key = this.bySubId.get(msg.subId);
        if (key !== undefined) this.subs.get(key)?.onEvent(msg.rev, msg.payload);
        return;
      }
      case "resync": {
        const key = this.bySubId.get(msg.subId);
        if (key !== undefined) this.subs.get(key)?.onResync();
        return;
      }
    }
  }
}
```

- [ ] **Step 5: 구현 — WebSocket 전송과 문구**

```ts
// app/src/rpc/transport-ws.ts
import type { Transport, TransportState } from "./client";

const DELAYS = [200, 500, 1000, 2000, 5000];

export function wsTransport(url: string): Transport {
  const frameCbs = new Set<(f: Uint8Array) => void>();
  const stateCbs = new Set<(s: TransportState) => void>();
  let ws: WebSocket | undefined;
  let attempt = 0;
  let closed = false;
  const emit = (s: TransportState) => stateCbs.forEach((cb) => cb(s));
  const open = () => {
    emit("connecting");
    ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => {
      attempt = 0;
      emit("open");
    };
    ws.onmessage = (e) => frameCbs.forEach((cb) => cb(new Uint8Array(e.data as ArrayBuffer)));
    ws.onclose = () => {
      emit("closed");
      if (!closed) setTimeout(open, DELAYS[Math.min(attempt++, DELAYS.length - 1)]);
    };
  };
  return {
    connect: open,
    close: () => {
      closed = true;
      ws?.close();
    },
    send: (frame) => {
      if (ws?.readyState === WebSocket.OPEN) ws.send(frame);
    },
    onFrame: (cb) => {
      frameCbs.add(cb);
      return () => frameCbs.delete(cb);
    },
    onState: (cb) => {
      stateCbs.add(cb);
      return () => stateCbs.delete(cb);
    },
  };
}
```

```ts
// app/src/rpc/messages.ts
import type { ErrorCode } from "./client";
import type { RpcErrorCode } from "./gen/rpc";

export interface Message {
  title: string;
  action?: string;
}

type Table = { [C in RpcErrorCode]: Message | ((detail: unknown) => Message) };

const path = (d: unknown) => (d && typeof d === "object" && "path" in d ? String((d as { path: unknown }).path) : "");

// 생성된 오류 코드가 늘면 이 표에 빠진 키가 컴파일 오류로 드러난다.
const TABLE: Table = {
  internal: { title: "예상하지 못한 문제가 생겼습니다.", action: "진단 ID를 복사해 두면 원인을 찾을 수 있습니다." },
  invalid_params: { title: "요청 형식이 맞지 않습니다.", action: "앱과 데몬 버전을 확인해 주세요." },
  method_not_found: { title: "데몬이 이 기능을 모릅니다.", action: "앱을 다시 시작해 데몬을 갱신해 주세요." },
  hello_required: { title: "연결 준비가 끝나지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  cancelled: { title: "취소했습니다." },
  daemon_restarting: { title: "데몬이 다시 시작하는 중이라 실행하지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  not_found: { title: "대상을 찾을 수 없습니다." },
  secrets_locked: { title: "비밀 값을 아직 열지 못했습니다.", action: "앱을 연 상태에서 다시 시도해 주세요." },
  "system.unavailable": { title: "데몬 정보를 읽지 못했습니다." },
  "projects.path_invalid": (d) => ({ title: `경로를 열 수 없습니다: ${path(d)}`, action: "WSL 안의 절대 경로(/home/...)를 입력해 주세요." }),
  "projects.not_directory": (d) => ({ title: `폴더가 아닙니다: ${path(d)}` }),
  "projects.already_exists": (d) => ({ title: `이미 추가한 프로젝트입니다: ${path(d)}` }),
  "projects.not_found": { title: "프로젝트를 찾을 수 없습니다.", action: "목록을 새로 고쳐 주세요." },
};

export function messageFor(code: ErrorCode, detail: unknown): Message {
  if (code === "connection_lost") return { title: "연결이 끊겨 결과를 확인하지 못했습니다.", action: "다시 연결되면 상태를 확인해 주세요." };
  const m = TABLE[code];
  return typeof m === "function" ? m(detail) : m;
}
```

새 도메인 오류가 생기면 `pnpm gen` 뒤 `tsc`가 이 표의 빠진 키를 알려 준다.

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/rpc && pnpm --filter app typecheck
git add app/src/rpc && git commit -m "feat(app): add the RPC client, WebSocket transport and error messages"
```

---

### Task 18: TanStack Query 연결과 rev 규칙, 연결 상태

**Files:**
- Create: `app/src/rpc/query.ts`, `app/src/rpc/connection.ts`, `app/src/rpc/RpcProvider.tsx`
- Test: `app/src/rpc/query.test.ts`

**Interfaces:**
- Consumes: Task 17의 `RpcClient`
- Produces:
  - `compareRev(a, b)`
  - `useRpc()`(클라이언트)
  - `useRpcQuery(method, params, opts?)`
  - `useRpcMutation(method)`: 생성형 mutation이면 `clientRequestId`를 자동으로 붙인다.
  - `useTopicInvalidation(topic, queryKeys)`
  - `useConnection()`: `{ state, daemonVersion, lastReadyMs }`
  - `RpcProvider({ client, children })`
  - 규칙: 이벤트 rev ≤ 캐시 rev이면 무시. `resync`·재연결이면 무조건 무효화. `daemonId`가 바뀌면 `queryClient.clear()`.

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/rpc/query.test.ts
import { QueryClient } from "@tanstack/react-query";
import { compareRev, invalidateIfNewer, setIfNewer } from "./query";

test("revs compare as big integers, not strings", () => {
  expect(compareRev("10", "9")).toBe(1);
  expect(compareRev("9007199254740993", "9007199254740992")).toBe(1);
  expect(compareRev("5", "5")).toBe(0);
});

test("events no newer than the cache are ignored", async () => {
  const qc = new QueryClient();
  qc.setQueryData(["projects.list", {}], { rev: "8", items: [] });
  const spy = vi.spyOn(qc, "invalidateQueries");
  await invalidateIfNewer(qc, [["projects.list", {}]], "8");
  expect(spy).not.toHaveBeenCalled();
  await invalidateIfNewer(qc, [["projects.list", {}]], "9");
  expect(spy).toHaveBeenCalledOnce();
});

test("a late response never overwrites a newer cache entry", () => {
  const qc = new QueryClient();
  qc.setQueryData(["projects.list", {}], { rev: "12", items: ["new"] });
  setIfNewer(qc, ["projects.list", {}], { rev: "11", items: ["old"] });
  expect(qc.getQueryData(["projects.list", {}])).toEqual({ rev: "12", items: ["new"] });
  setIfNewer(qc, ["projects.list", {}], { rev: "13", items: ["newest"] });
  expect(qc.getQueryData(["projects.list", {}])).toEqual({ rev: "13", items: ["newest"] });
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/rpc/query.test.ts`

- [ ] **Step 3: 구현**

```ts
// app/src/rpc/query.ts
import { type QueryClient, type QueryKey, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { RpcError } from "./client";
import { METHOD_INFO, type Methods, type Topics } from "./gen/rpc";
import { useRpc } from "./RpcProvider";

export function compareRev(a: string, b: string): number {
  const x = BigInt(a);
  const y = BigInt(b);
  return x < y ? -1 : x > y ? 1 : 0;
}

const revOf = (data: unknown): string | undefined =>
  data && typeof data === "object" && "rev" in data && typeof (data as { rev: unknown }).rev === "string" ? (data as { rev: string }).rev : undefined;

export async function invalidateIfNewer(qc: QueryClient, keys: QueryKey[], eventRev: string | null): Promise<void> {
  for (const key of keys) {
    const cached = revOf(qc.getQueryData(key));
    if (eventRev !== null && cached !== undefined && compareRev(cached, eventRev) >= 0) continue;
    await qc.invalidateQueries({ queryKey: key });
  }
}

export function setIfNewer(qc: QueryClient, key: QueryKey, data: unknown): void {
  const cached = revOf(qc.getQueryData(key));
  const incoming = revOf(data);
  if (cached !== undefined && incoming !== undefined && compareRev(incoming, cached) <= 0) return;
  qc.setQueryData(key, data);
}

type QueryMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "query" ? M : never }[keyof Methods];
type MutationMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "mutation" ? M : never }[keyof Methods];

export function useRpcQuery<M extends QueryMethod>(method: M, params: Methods[M]["params"], opts: { enabled?: boolean } = {}) {
  const client = useRpc();
  const qc = useQueryClient();
  return useQuery<Methods[M]["result"], RpcError>({
    queryKey: [method, params],
    enabled: opts.enabled ?? true,
    retry: (count, e) => e instanceof RpcError && e.code === "connection_lost" && count < 3,
    queryFn: async ({ signal }) => {
      const r = await client.call(method, params, { signal });
      setIfNewer(qc, [method, params], r);
      return (qc.getQueryData([method, params]) as Methods[M]["result"]) ?? r;
    },
  });
}

export function useRpcMutation<M extends MutationMethod>(method: M) {
  const client = useRpc();
  return useMutation<Methods[M]["result"], RpcError, Methods[M]["params"]>({
    mutationFn: (params) => client.call(method, params, { clientRequestId: crypto.randomUUID() }),
    retry: false,
  });
}

export function confirmPolicy(method: keyof Methods) {
  return METHOD_INFO[method].confirm;
}

export function useTopicInvalidation<T extends keyof Topics>(topic: T, keys: QueryKey[]) {
  const client = useRpc();
  const qc = useQueryClient();
  const keyText = JSON.stringify(keys);
  // biome-ignore lint/correctness/useExhaustiveDependencies: keys are compared by their JSON text
  useEffect(() => client.subscribe(topic, (rev) => void invalidateIfNewer(qc, keys, rev), () => void invalidateIfNewer(qc, keys, null)), [client, qc, topic, keyText]);
}
```

```ts
// app/src/rpc/connection.ts
import { create } from "zustand";
import type { ClientState } from "./client";

interface ConnectionStore {
  state: ClientState;
  daemonVersion?: string;
  lastReadyMs?: number;
  set(state: ClientState, daemonVersion?: string): void;
}

export const useConnection = create<ConnectionStore>((set) => ({
  state: "connecting",
  set: (state, daemonVersion) => set((s) => ({ state, daemonVersion: daemonVersion ?? s.daemonVersion, lastReadyMs: state === "ready" ? Date.now() : s.lastReadyMs })),
}));
```

```tsx
// app/src/rpc/RpcProvider.tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import type { RpcClient } from "./client";
import { useConnection } from "./connection";

const Ctx = createContext<RpcClient | null>(null);

export function useRpc(): RpcClient {
  const c = useContext(Ctx);
  if (!c) throw new Error("RpcProvider missing");
  return c;
}

export function RpcProvider({ client, children }: { client: RpcClient; children: ReactNode }) {
  const [qc] = useState(() => new QueryClient({ defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false } } }));
  const setConn = useConnection((s) => s.set);
  useEffect(() => {
    const offState = client.onState((s) => setConn(s, client.daemonVersion));
    const offReset = client.onReset(() => qc.clear());
    client.start();
    return () => {
      offState();
      offReset();
    };
  }, [client, qc, setConn]);
  return (
    <Ctx.Provider value={client}>
      <QueryClientProvider client={qc}>{children}</QueryClientProvider>
    </Ctx.Provider>
  );
}
```

- [ ] **Step 4: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/rpc && pnpm --filter app typecheck
git add app/src/rpc && git commit -m "feat(app): connect RPC to TanStack Query with rev-ordered updates"
```

---

### Task 19: 앱 프레임·라우팅·단축키 레지스트리·명령 팔레트

**Files:**
- Create: `app/src/App.tsx`, `app/src/shell/Frame.tsx`·`Frame.module.css`, `TitleBar.tsx`, `ActivityBar.tsx`, `StatusBar.tsx`, `ConnectionBanner.tsx`, `CommandPalette.tsx`, `keymap.ts`, `commands.ts`, `sections.tsx`, `router.tsx`
- Create: `app/src/features/settings/SettingsPage.tsx`(S0b 최소: 데몬 정보와 점검. S1 Task 21이 탭을 더함)
- Create: `app/src/shell/SectionBoundary.tsx`(섹션 본문 오류 경계, 01-design §8.6 오류 위치) · Test: `app/src/shell/SectionBoundary.test.tsx`(`section_render_error_keeps_the_frame_and_other_sections`: 한 섹션 컴포넌트가 그리기 중 throw → 그 자리에 ErrorState와 [다시 시도], 활동 막대·다른 섹션 이동은 동작)
- Modify: `app/src/main.tsx`
- Test: `app/src/shell/keymap.test.ts`, `app/src/shell/CommandPalette.test.tsx`

**Interfaces:**
- Consumes: Task 16·17·18
- Produces:
  - `type Scope = "global"|"app"|"section"|"terminal"|"editor"`
  - `Binding { id, code, ctrl?, shift?, alt?, scope, label, run }`
  - `matches(e, b)`, `allowedWhileFocused(b, focusScope)`, `useKeymap(bindings)`
  - `useCommands`(Zustand): `register(cmd) -> unregister`, `list()`
  - `CommandPalette`: Ctrl+Shift+P·F1, 터미널·편집기 밖에서는 Ctrl+K도
  - 섹션 경로: `/agents` `/terminal` `/runs` `/source` `/notes` `/tools` `/settings`. 시작 경로는 마지막 섹션(`localStorage["devbox.lastSection"]`, 기본 `/agents`)
  - `Frame`: 제목 표시줄 36px, 활동 막대 48px, 목록 영역, 본문, 상태 표시줄 24px
  - `ConnectionBanner`: `reconnecting`이면 "데몬에 다시 연결하고 있습니다 · 마지막 갱신 hh:mm", `versionMismatch`면 "앱과 데몬 버전이 다릅니다"

- [ ] **Step 1: 실패하는 테스트**

```ts
// app/src/shell/keymap.test.ts
import { allowedWhileFocused, type Binding, matches } from "./keymap";

const palette: Binding = { id: "palette", code: "KeyP", ctrl: true, shift: true, scope: "app", label: "명령 팔레트", run: () => {} };
const quickOpen: Binding = { id: "quickOpen", code: "KeyP", ctrl: true, scope: "app", label: "빠른 열기", run: () => {} };

const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

test("matches by physical key so Korean layout works", () => {
  expect(matches(key({ code: "KeyP", key: "ㅔ", ctrlKey: true, shiftKey: true }), palette)).toBe(true);
  expect(matches(key({ code: "KeyP", key: "P", ctrlKey: true }), palette)).toBe(false);
});

test("ignores keys while an IME composition is in progress", () => {
  expect(matches(key({ code: "KeyP", ctrlKey: true, shiftKey: true, isComposing: true }), palette)).toBe(false);
});

test("terminal and editor focus only lets modified chords through", () => {
  expect(allowedWhileFocused(palette, "terminal")).toBe(true);
  expect(allowedWhileFocused(quickOpen, "terminal")).toBe(false);
  expect(allowedWhileFocused({ ...quickOpen, code: "Digit2" }, "editor")).toBe(true);
  expect(allowedWhileFocused({ ...quickOpen, code: "F1", ctrl: false }, "terminal")).toBe(true);
  expect(allowedWhileFocused(quickOpen, null)).toBe(true);
});
```

```tsx
// app/src/shell/CommandPalette.test.tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCommands } from "./commands";
import { CommandPalette } from "./CommandPalette";

test("filters commands by Korean text and runs the chosen one", async () => {
  const run = vi.fn();
  useCommands.getState().register({ id: "projects.switch", label: "프로젝트 전환", keys: ["Ctrl", "Shift", "O"], run });
  useCommands.getState().register({ id: "settings.open", label: "설정 열기", run: () => {} });
  render(<CommandPalette open onOpenChange={() => {}} />);
  await userEvent.type(screen.getByRole("combobox"), "전환");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await userEvent.keyboard("{Enter}");
  expect(run).toHaveBeenCalledOnce();
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/shell`

- [ ] **Step 3: 구현 — keymap·commands**

```ts
// app/src/shell/keymap.ts
import { useEffect } from "react";

export type Scope = "global" | "app" | "section" | "terminal" | "editor";
export interface Binding {
  id: string;
  code: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  scope: Scope;
  label: string;
  run: () => void;
}

export function matches(e: KeyboardEvent, b: Binding): boolean {
  if (e.isComposing || e.keyCode === 229) return false;
  return e.code === b.code && e.ctrlKey === !!b.ctrl && e.shiftKey === !!b.shift && e.altKey === !!b.alt;
}

export function allowedWhileFocused(b: Binding, focus: "terminal" | "editor" | null): boolean {
  if (focus === null) return true;
  if (/^F\d+$/.test(b.code)) return true;
  if (b.ctrl && /^Digit\d$/.test(b.code)) return true;
  return !!b.ctrl && (!!b.shift || !!b.alt);
}

function focusScope(): "terminal" | "editor" | null {
  const el = document.activeElement?.closest("[data-keyscope]");
  const v = el?.getAttribute("data-keyscope");
  return v === "terminal" || v === "editor" ? v : null;
}

export function useKeymap(bindings: Binding[]): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const scope = focusScope();
      for (const b of bindings) {
        if (matches(e, b) && allowedWhileFocused(b, scope)) {
          e.preventDefault();
          e.stopPropagation();
          b.run();
          return;
        }
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [bindings]);
}
```

```ts
// app/src/shell/commands.ts
import { create } from "zustand";

export interface Command {
  id: string;
  label: string;
  keys?: string[];
  run: () => void;
}

interface Commands {
  items: Map<string, Command>;
  register(c: Command): () => void;
}

export const useCommands = create<Commands>((set, get) => ({
  items: new Map(),
  register: (c) => {
    set({ items: new Map(get().items).set(c.id, c) });
    return () => {
      const next = new Map(get().items);
      next.delete(c.id);
      set({ items: next });
    };
  },
}));
```

- [ ] **Step 4: 구현 — 명령 팔레트**

```tsx
// app/src/shell/CommandPalette.tsx
import { useMemo, useState } from "react";
import { Dialog, Kbd } from "../ui";
import { useCommands } from "./commands";

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const items = useCommands((s) => s.items);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const list = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return [...items.values()].filter((c) => !needle || c.label.toLowerCase().includes(needle) || c.id.includes(needle));
  }, [items, q]);
  const run = (i: number) => {
    const c = list[i];
    if (!c) return;
    onOpenChange(false);
    setQ("");
    c.run();
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="명령" width={560}>
      <input
        role="combobox"
        aria-expanded
        aria-controls="palette-list"
        aria-label="명령 검색"
        autoFocus
        value={q}
        placeholder="명령 이름을 입력하세요"
        onChange={(e) => {
          setQ(e.target.value);
          setActive(0);
        }}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return;
          if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, list.length - 1));
          if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
          if (e.key === "Enter") run(active);
        }}
        style={{ height: "var(--control-lg)", padding: "0 var(--space-3)", background: "var(--bg-1)", border: "1px solid var(--border)", borderRadius: "var(--radius-md)" }}
      />
      <ul id="palette-list" role="listbox" style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 360, overflow: "auto" }}>
        {list.map((c, i) => (
          <li
            key={c.id}
            role="option"
            aria-selected={i === active}
            onMouseEnter={() => setActive(i)}
            onClick={() => run(i)}
            style={{ display: "flex", justifyContent: "space-between", padding: "6px var(--space-3)", borderRadius: "var(--radius-sm)", background: i === active ? "var(--bg-3)" : undefined, cursor: "pointer" }}
          >
            <span>{c.label}</span>
            {c.keys && <Kbd keys={c.keys} />}
          </li>
        ))}
        {list.length === 0 && <li style={{ padding: "var(--space-3)", color: "var(--fg-muted)" }}>일치하는 명령이 없습니다</li>}
      </ul>
    </Dialog>
  );
}
```

- [ ] **Step 5: 구현 — 프레임·섹션·라우터·App**

```tsx
// app/src/shell/sections.tsx
import { Bot, FolderGit2, NotebookPen, Play, SquareTerminal, Wrench } from "lucide-react";
import type { ReactNode } from "react";

export interface Section {
  path: string;
  label: string;
  icon: ReactNode;
  digit: number;
}

export const SECTIONS: Section[] = [
  { path: "/agents", label: "에이전트", icon: <Bot size={20} />, digit: 1 },
  { path: "/terminal", label: "터미널", icon: <SquareTerminal size={20} />, digit: 2 },
  { path: "/runs", label: "실행", icon: <Play size={20} />, digit: 3 },
  { path: "/source", label: "소스", icon: <FolderGit2 size={20} />, digit: 4 },
  { path: "/notes", label: "노트", icon: <NotebookPen size={20} />, digit: 5 },
  { path: "/tools", label: "도구", icon: <Wrench size={20} />, digit: 6 },
];
```

```tsx
// app/src/shell/ActivityBar.tsx
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Settings } from "lucide-react";
import { IconButton } from "../ui";
import { SECTIONS } from "./sections";

export function ActivityBar() {
  const navigate = useNavigate();
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <nav aria-label="섹션" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, padding: "8px 0", background: "var(--bg-0)", borderRight: "1px solid var(--border)" }}>
      {SECTIONS.map((s) => (
        <IconButton key={s.path} label={s.label} icon={s.icon} shortcut={["Ctrl", String(s.digit)]} pressed={path.startsWith(s.path)} onClick={() => void navigate({ to: s.path })} />
      ))}
      <span style={{ flex: 1 }} />
      <IconButton label="설정" icon={<Settings size={20} />} pressed={path.startsWith("/settings")} onClick={() => void navigate({ to: "/settings" })} />
    </nav>
  );
}
```

```tsx
// app/src/shell/StatusBar.tsx
import { useConnection } from "../rpc/connection";
import { StatusDot } from "../ui";

const LABEL = { connecting: "연결 중", ready: "데몬 연결됨", reconnecting: "다시 연결 중", versionMismatch: "버전 불일치" } as const;
const TONE = { connecting: "progress", ready: "ok", reconnecting: "warn", versionMismatch: "danger" } as const;

export function StatusBar() {
  const { state, daemonVersion } = useConnection();
  return (
    <footer style={{ display: "flex", alignItems: "center", gap: "var(--space-3)", padding: "0 var(--space-3)", fontSize: "var(--text-xs)", color: "var(--fg-muted)", background: "var(--bg-0)", borderTop: "1px solid var(--border)" }}>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
        <StatusDot tone={TONE[state]} label={LABEL[state]} />
        {LABEL[state]}
      </span>
      <span style={{ flex: 1 }} />
      {daemonVersion && <span>데몬 {daemonVersion}</span>}
    </footer>
  );
}
```

```tsx
// app/src/shell/ConnectionBanner.tsx
import { useConnection } from "../rpc/connection";
import { Banner } from "../ui";

const hhmm = (ms?: number) => (ms ? new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" }) : "—");

export function ConnectionBanner() {
  const { state, lastReadyMs } = useConnection();
  if (state === "reconnecting") return <Banner tone="warn">데몬에 다시 연결하고 있습니다 · 마지막 갱신 {hhmm(lastReadyMs)}</Banner>;
  if (state === "versionMismatch") return <Banner tone="danger">앱과 데몬 버전이 다릅니다. 앱을 다시 시작하면 데몬을 맞춥니다.</Banner>;
  return null;
}
```

```tsx
// app/src/shell/TitleBar.tsx
import { Search } from "lucide-react";
import type { ReactNode } from "react";
import { Button, Kbd } from "../ui";

export function TitleBar({ project, onOpenPalette }: { project: ReactNode; onOpenPalette: () => void }) {
  return (
    <header style={{ display: "flex", alignItems: "center", gap: "var(--space-3)", padding: "0 var(--space-3)", background: "var(--bg-0)", borderBottom: "1px solid var(--border)" }}>
      <strong style={{ fontSize: "var(--text-sm)" }}>devbox</strong>
      {project}
      <span style={{ flex: 1 }} />
      <Button variant="ghost" size="sm" onClick={onOpenPalette} aria-label="명령 팔레트 열기">
        <Search size={14} /> 명령·검색 <Kbd keys={["Ctrl", "Shift", "P"]} />
      </Button>
    </header>
  );
}
```

```tsx
// app/src/shell/Frame.tsx
import { Outlet } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { ActivityBar } from "./ActivityBar";
import { ConnectionBanner } from "./ConnectionBanner";
import s from "./Frame.module.css";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

export function Frame({ project, onOpenPalette }: { project: ReactNode; onOpenPalette: () => void }) {
  return (
    <div className={s.frame}>
      <div className={s.title}><TitleBar project={project} onOpenPalette={onOpenPalette} /></div>
      <div className={s.activity}><ActivityBar /></div>
      <main className={s.main}>
        <ConnectionBanner />
        <div className={s.content}><Outlet /></div>
      </main>
      <div className={s.status}><StatusBar /></div>
    </div>
  );
}
```

```css
/* app/src/shell/Frame.module.css */
.frame { display: grid; height: 100vh; grid-template-columns: var(--activitybar) 1fr; grid-template-rows: var(--titlebar) 1fr var(--statusbar); grid-template-areas: "title title" "activity main" "status status"; }
.title { grid-area: title; display: grid; }
.activity { grid-area: activity; display: grid; }
.main { grid-area: main; display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--bg-1); }
.content { flex: 1; min-height: 0; overflow: hidden; }
.status { grid-area: status; display: grid; }
```

```tsx
// app/src/shell/router.tsx
import { createRootRoute, createRoute, createRouter, redirect } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { EmptyState } from "../ui";
import { SECTIONS } from "./sections";

export function buildRouter(root: () => ReactNode, settings: () => ReactNode) {
  const rootRoute = createRootRoute({ component: root });
  const index = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    beforeLoad: () => {
      throw redirect({ to: localStorage.getItem("devbox.lastSection") ?? "/agents" });
    },
  });
  const sectionRoutes = SECTIONS.map((s) =>
    createRoute({
      getParentRoute: () => rootRoute,
      path: s.path,
      onEnter: () => localStorage.setItem("devbox.lastSection", s.path),
      component: () => <EmptyState title={s.label} body="이 화면은 다음 하위 프로젝트에서 채워집니다." />,
    }),
  );
  const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: settings });
  return createRouter({ routeTree: rootRoute.addChildren([index, ...sectionRoutes, settingsRoute]) });
}
```

`App.tsx`는 RPC 클라이언트를 만들고, `TooltipProvider` → `RpcProvider` → `RouterProvider` 순으로 감싼다. 전송은 이렇게 고른다.
- Tauri 안이면 `tauriTransport()`(Task 23).
- 아니면 `?gateway=` 쿼리 → `import.meta.env.VITE_DEVBOX_WS` 순서의 URL로 `wsTransport`.

전역 단축키는 다음을 `useKeymap`으로 등록한다.
- Ctrl+1–6: 섹션 이동
- Ctrl+Shift+P·F1: 팔레트
- Ctrl+K: 팔레트(app 범위)
- Ctrl+Shift+O: 프로젝트 전환(Task 20)
- Alt+←/→: 뒤로·앞으로

같은 명령을 `useCommands`에도 등록한다.

```tsx
// app/src/App.tsx
import { RouterProvider } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { RpcClient } from "./rpc/client";
import { RpcProvider } from "./rpc/RpcProvider";
import { wsTransport } from "./rpc/transport-ws";
import { useCommands } from "./shell/commands";
import { CommandPalette } from "./shell/CommandPalette";
import { Frame } from "./shell/Frame";
import { type Binding, useKeymap } from "./shell/keymap";
import { buildRouter } from "./shell/router";
import { SECTIONS } from "./shell/sections";
import { TooltipProvider } from "./ui";
import { ProjectSwitcher, ProjectTitle } from "./features/projects/ProjectSwitcher";
import { SettingsPage } from "./features/settings/SettingsPage";

declare const __APP_VERSION__: string;

function pickTransport() {
  if ("__TAURI_INTERNALS__" in window) return import("./rpc/transport-tauri").then((m) => m.tauriTransport());
  const url = new URLSearchParams(location.search).get("gateway") ?? import.meta.env.VITE_DEVBOX_WS;
  if (!url) throw new Error("VITE_DEVBOX_WS 또는 ?gateway= 가 필요합니다(scripts/dev.sh 사용)");
  return Promise.resolve(wsTransport(url));
}

export function App() {
  const [client, setClient] = useState<RpcClient | null>(null);
  useEffect(() => {
    void pickTransport().then((t) => setClient(new RpcClient(t, { version: __APP_VERSION__, instance: import.meta.env.VITE_DEVBOX_INSTANCE ?? "dev", client: "__TAURI_INTERNALS__" in window ? "app" : "dev" })));
  }, []);
  if (!client) return null;
  return (
    <TooltipProvider>
      <RpcProvider client={client}>
        <Shell />
      </RpcProvider>
    </TooltipProvider>
  );
}

function Shell() {
  const [palette, setPalette] = useState(false);
  const [switcher, setSwitcher] = useState(false);
  const router = useMemo(
    () => buildRouter(() => <Frame project={<ProjectTitle onOpen={() => setSwitcher(true)} />} onOpenPalette={() => setPalette(true)} />, () => <SettingsPage />),
    [],
  );
  const bindings = useMemo<Binding[]>(
    () => [
      ...SECTIONS.map((s) => ({ id: `go.${s.path}`, code: `Digit${s.digit}`, ctrl: true, scope: "app" as const, label: `${s.label}(으)로 이동`, run: () => void router.navigate({ to: s.path }) })),
      { id: "palette", code: "KeyP", ctrl: true, shift: true, scope: "app", label: "명령 팔레트", run: () => setPalette(true) },
      { id: "palette.f1", code: "F1", scope: "app", label: "명령 팔레트", run: () => setPalette(true) },
      { id: "palette.k", code: "KeyK", ctrl: true, scope: "app", label: "명령 팔레트", run: () => setPalette(true) },
      { id: "projects.switch", code: "KeyO", ctrl: true, shift: true, scope: "app", label: "프로젝트 전환", run: () => setSwitcher(true) },
      { id: "nav.back", code: "ArrowLeft", alt: true, scope: "app", label: "뒤로", run: () => router.history.back() },
      { id: "nav.forward", code: "ArrowRight", alt: true, scope: "app", label: "앞으로", run: () => router.history.forward() },
    ],
    [router],
  );
  useKeymap(bindings);
  const register = useCommands((s) => s.register);
  useEffect(() => {
    const offs = [
      register({ id: "projects.switch", label: "프로젝트 전환", keys: ["Ctrl", "Shift", "O"], run: () => setSwitcher(true) }),
      register({ id: "settings.open", label: "설정 열기", run: () => void router.navigate({ to: "/settings" }) }),
      ...SECTIONS.map((s) => register({ id: `go.${s.path}`, label: `${s.label} 열기`, keys: ["Ctrl", String(s.digit)], run: () => void router.navigate({ to: s.path }) })),
    ];
    return () => offs.forEach((off) => off());
  }, [register, router]);
  return (
    <>
      <RouterProvider router={router} />
      <CommandPalette open={palette} onOpenChange={setPalette} />
      <ProjectSwitcher open={switcher} onOpenChange={setSwitcher} />
    </>
  );
}
```

`vite.config.ts`에 `define: { __APP_VERSION__: JSON.stringify(process.env.npm_package_version) }`를 추가한다.

```tsx
// app/src/features/settings/SettingsPage.tsx (S0b 최소: 데몬 정보와 점검)
import { useRpcQuery } from "../../rpc/query";
import { StatusDot } from "../../ui";

export function SettingsPage() {
  const info = useRpcQuery("system.info", {});
  const doctor = useRpcQuery("system.doctor", {});
  return (
    <div style={{ padding: "var(--space-6)", overflow: "auto", height: "100%" }}>
      <h2 style={{ marginTop: 0, fontSize: "var(--text-xl)" }}>앱 진단</h2>
      {info.data && (
        <p style={{ color: "var(--fg-muted)" }}>
          데몬 {info.data.version} · 인스턴스 {info.data.instance} · 데이터 {info.data.dataDir}
        </p>
      )}
      <ul style={{ listStyle: "none", padding: 0, display: "grid", gap: 6 }}>
        {doctor.data?.checks.map((c) => (
          <li key={c.id} style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
            <StatusDot tone={c.status === "ok" ? "ok" : c.status === "warn" ? "warn" : "danger"} label={c.status} />
            <span>{c.message}</span>
            {c.fix && <code style={{ color: "var(--fg-muted)" }}>{c.fix}</code>}
          </li>
        ))}
      </ul>
    </div>
  );
}
```

```tsx
// app/src/main.tsx (최종)
import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./ui/tokens.css";
import "./ui/global.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
```

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/shell && pnpm --filter app typecheck
git add app && git commit -m "feat(app): add the frame, routing, keymap and command palette"
```

---

### Task 20: 프로젝트 전환기와 추가·제거

**Files:**
- Create: `app/src/features/projects/ProjectSwitcher.tsx`, `app/src/features/projects/AddProjectDialog.tsx`, `app/src/features/projects/store.ts`
- Test: `app/src/features/projects/ProjectSwitcher.test.tsx`

**Interfaces:**
- Consumes: Task 18의 `useRpcQuery`, `useRpcMutation`, `useTopicInvalidation`, `confirmPolicy`, Task 17의 `messageFor`
- Produces:
  - `useCurrentProject`(Zustand, `localStorage` 유지): `{ id?, set(id) }`
  - `ProjectTitle({ onOpen })`: 제목 표시줄의 현재 프로젝트 버튼
  - `ProjectSwitcher({ open, onOpenChange })`: 검색 + 목록, Enter 선택, [프로젝트 추가], 행마다 [제거](확인 정책 `always`)
  - `AddProjectDialog({ open, onOpenChange })`: `projects.scan` 후보 + 직접 입력
  - S1이 프로젝트 카드(브랜치·실행·에이전트)와 시작 구성을 덧붙인다.

- [ ] **Step 1: 실패하는 테스트**

```tsx
// app/src/features/projects/ProjectSwitcher.test.tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RpcClient } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { useCurrentProject } from "./store";

function fakeClient(calls: Record<string, unknown>) {
  const client = {
    start: () => {},
    onState: () => () => {},
    onReset: () => () => {},
    subscribe: () => () => {},
    call: vi.fn((method: string) => Promise.resolve(calls[method])),
  };
  return client as unknown as RpcClient & { call: ReturnType<typeof vi.fn> };
}

test("lists projects, filters, selects with Enter and removes after confirmation", async () => {
  const client = fakeClient({
    "projects.list": { rev: "3", items: [{ id: "a", name: "devbox", path: "/home/u/projects/devbox", favorite: false, addedMs: 1 }, { id: "b", name: "family-care", path: "/home/u/projects/family-care", favorite: false, addedMs: 2 }] },
    "projects.remove": { rev: "4" },
  });
  render(
    <RpcProvider client={client}>
      <ProjectSwitcher open onOpenChange={() => {}} />
    </RpcProvider>,
  );
  await screen.findByText("family-care");
  await userEvent.type(screen.getByRole("combobox"), "family");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await userEvent.keyboard("{Enter}");
  expect(useCurrentProject.getState().id).toBe("b");

  await userEvent.clear(screen.getByRole("combobox"));
  await userEvent.click(screen.getByRole("button", { name: "devbox 제거" }));
  await userEvent.click(screen.getByRole("button", { name: "프로젝트 제거" }));
  await waitFor(() => expect(client.call).toHaveBeenCalledWith("projects.remove", { id: "a" }, expect.anything()));
});
```

- [ ] **Step 2: 실패 확인** — `pnpm --filter app exec vitest run src/features/projects`

- [ ] **Step 3: 구현**

```ts
// app/src/features/projects/store.ts
import { create } from "zustand";
import { persist } from "zustand/middleware";

export const useCurrentProject = create<{ id?: string; set(id: string | undefined): void }>()(
  persist((set) => ({ id: undefined, set: (id) => set({ id }) }), { name: "devbox.currentProject" }),
);
```

```tsx
// app/src/features/projects/ProjectSwitcher.tsx
import { ChevronDown, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Banner, Button, ConfirmDialog, Dialog } from "../../ui";
import { AddProjectDialog } from "./AddProjectDialog";
import { useCurrentProject } from "./store";

const LIST_KEY = [["projects.list", {}]];

export function ProjectTitle({ onOpen }: { onOpen: () => void }) {
  const current = useCurrentProject((s) => s.id);
  const list = useRpcQuery("projects.list", {});
  useTopicInvalidation("projects.changed", LIST_KEY);
  const p = list.data?.items.find((x) => x.id === current);
  return (
    <Button variant="ghost" size="sm" onClick={onOpen} aria-label="프로젝트 전환">
      {p?.name ?? "프로젝트 선택"} <ChevronDown size={14} />
    </Button>
  );
}

export function ProjectSwitcher({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const list = useRpcQuery("projects.list", {}, { enabled: open });
  useTopicInvalidation("projects.changed", LIST_KEY);
  const setCurrent = useCurrentProject((s) => s.set);
  const remove = useRpcMutation("projects.remove");
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const [adding, setAdding] = useState(false);
  const [confirm, setConfirm] = useState<{ id: string; name: string } | null>(null);
  const items = useMemo(() => {
    const n = q.trim().toLowerCase();
    return (list.data?.items ?? []).filter((p) => !n || p.name.toLowerCase().includes(n) || p.path.toLowerCase().includes(n));
  }, [list.data, q]);
  const pick = (i: number) => {
    const p = items[i];
    if (!p) return;
    setCurrent(p.id);
    onOpenChange(false);
  };
  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange} title="프로젝트" width={620} footer={<Button variant="primary" onClick={() => setAdding(true)}>프로젝트 추가</Button>}>
        {remove.error && <Banner tone="danger">{messageFor(remove.error.code, remove.error.detail).title}</Banner>}
        <input
          role="combobox"
          aria-expanded
          aria-controls="project-list"
          aria-label="프로젝트 검색"
          autoFocus
          value={q}
          placeholder="이름이나 경로"
          onChange={(e) => {
            setQ(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, items.length - 1));
            if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
            if (e.key === "Enter") pick(active);
          }}
          style={{ height: "var(--control-lg)", padding: "0 var(--space-3)", background: "var(--bg-1)", border: "1px solid var(--border)", borderRadius: "var(--radius-md)" }}
        />
        <ul id="project-list" role="listbox" style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 380, overflow: "auto" }}>
          {items.map((p, i) => (
            <li key={p.id} role="option" aria-selected={i === active} onMouseEnter={() => setActive(i)} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px", borderRadius: "var(--radius-sm)", background: i === active ? "var(--bg-3)" : undefined }}>
              <button type="button" onClick={() => pick(i)} style={{ all: "unset", flex: 1, cursor: "pointer" }}>
                <div>{p.name}</div>
                <div style={{ color: "var(--fg-subtle)", fontSize: "var(--text-xs)" }}>{p.path}</div>
              </button>
              <Button variant="ghost" size="sm" aria-label={`${p.name} 제거`} onClick={() => setConfirm({ id: p.id, name: p.name })}>
                <Trash2 size={14} />
              </Button>
            </li>
          ))}
          {list.isSuccess && items.length === 0 && <li style={{ padding: "var(--space-3)", color: "var(--fg-muted)" }}>프로젝트가 없습니다. [프로젝트 추가]로 ~/projects 아래 폴더를 고르세요.</li>}
        </ul>
      </Dialog>
      <AddProjectDialog open={adding} onOpenChange={setAdding} />
      <ConfirmDialog
        open={confirm !== null}
        title="프로젝트 제거"
        body={`${confirm?.name ?? ""}을(를) devbox 목록에서만 지웁니다. 폴더와 파일은 그대로 둡니다.`}
        confirmLabel="프로젝트 제거"
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          if (confirm) remove.mutate({ id: confirm.id });
          setConfirm(null);
        }}
      />
    </>
  );
}
```

```tsx
// app/src/features/projects/AddProjectDialog.tsx
import { useState } from "react";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Banner, Button, Dialog } from "../../ui";
import { useCurrentProject } from "./store";

export function AddProjectDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const scan = useRpcQuery("projects.scan", {}, { enabled: open });
  const add = useRpcMutation("projects.add");
  const setCurrent = useCurrentProject((s) => s.set);
  const [path, setPath] = useState("");
  const submit = (p: string) =>
    add.mutate(
      { path: p },
      {
        onSuccess: (project) => {
          setCurrent(project.id);
          setPath("");
          onOpenChange(false);
        },
      },
    );
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="프로젝트 추가" description="WSL 안의 폴더를 고르거나 경로를 입력하세요." width={620}>
      {add.error && <Banner tone="danger">{messageFor(add.error.code, add.error.detail).title} {messageFor(add.error.code, add.error.detail).action}</Banner>}
      <ul style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 280, overflow: "auto" }}>
        {scan.data?.candidates
          .filter((c) => !c.registered)
          .map((c) => (
            <li key={c.path}>
              <Button variant="ghost" style={{ width: "100%", justifyContent: "space-between" }} onClick={() => submit(c.path)}>
                <span>{c.name}</span>
                <span style={{ color: "var(--fg-subtle)" }}>{c.isGit ? "Git" : ""}</span>
              </Button>
            </li>
          ))}
      </ul>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) submit(path.trim());
        }}
        style={{ display: "flex", gap: 8 }}
      >
        <input aria-label="폴더 경로" value={path} onChange={(e) => setPath(e.target.value)} placeholder="/home/<user>/projects/..." style={{ flex: 1, height: "var(--control-md)", padding: "0 var(--space-2)", background: "var(--bg-1)", border: "1px solid var(--border)", borderRadius: "var(--radius-md)" }} />
        <Button type="submit" variant="primary" disabled={add.isPending}>추가</Button>
      </form>
    </Dialog>
  );
}
```

- [ ] **Step 4: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/features/projects && pnpm --filter app typecheck
git add app && git commit -m "feat(app): add the project switcher with add and remove"
```

---

### Task 21: 개발 스크립트·E2E·전체 검사·CI

**Files:**
- Create: `scripts/dev.sh`, `scripts/check.sh`
- Create: `app/playwright.config.ts`, `app/e2e/support.ts`, `app/e2e/global-setup.ts`, `app/e2e/projects.spec.ts`, `app/e2e/reconnect.spec.ts`
- Modify: `.github/workflows/ci.yml`(frontend·e2e job 추가)

**Interfaces:**
- Consumes:
  - 03-s0b의 `devbox daemon --foreground`·`dev-gateway --token`
  - Task 16–20의 화면
- Produces:
  - `pnpm dev`: dev 인스턴스 데몬(없으면 실행) + gateway + Vite. 주소 `http://localhost:1450`
  - `pnpm check`: 전체 검사 한 번
  - E2E 2개(화면 1460·gateway 1461, `pnpm dev`를 켜 둔 채로 돌 수 있음)
  - CI: `rust`, `frontend`, `e2e` 세 job + 집계 `ci-ok`(필수 검사)

- [ ] **Step 1: 개발·검사 스크립트**

```bash
# scripts/dev.sh
#!/usr/bin/env bash
# WSL에서 실제 데몬(dev 인스턴스)에 붙은 화면을 띄운다.
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.cargo/env
export DEVBOX_INSTANCE=dev
cargo build -q -p devbox-cli
BIN="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/debug/devbox"
if ! systemctl --user is-active -q devbox@dev.service 2>/dev/null; then
  echo "devbox@dev.service 가 꺼져 있어 포그라운드 데몬을 띄웁니다(종료하면 함께 꺼짐)."
  "$BIN" daemon --foreground & DAEMON=$!
fi
TOKEN=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
"$BIN" dev-gateway --port 1451 --token "$TOKEN" & GW=$!
trap 'kill ${GW:-} ${DAEMON:-} 2>/dev/null || true' EXIT
VITE_DEVBOX_WS="ws://127.0.0.1:1451/ws?token=$TOKEN" VITE_DEVBOX_INSTANCE=dev pnpm --filter app dev
```

```bash
# scripts/check.sh
#!/usr/bin/env bash
# PR 전에 한 번 돌리는 전체 검사. CI와 같은 순서.
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.cargo/env
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p xtask -- gen-ts --check
bash scripts/banned-words.sh
pnpm exec biome ci .
pnpm --filter app typecheck
pnpm --filter app test
pnpm --filter app e2e
echo "check: all passed"
```

- [ ] **Step 2: E2E 지원 코드**

```ts
// app/e2e/support.ts
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export interface E2EEnv {
  home: string;
  runtime: string;
  instance: string;
  bin: string;
  gatewayUrl: string;
}

export function devboxBin(): string {
  execFileSync("cargo", ["build", "-q", "-p", "devbox-cli"], { stdio: "inherit" });
  const meta = JSON.parse(execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"]).toString());
  return join(meta.target_directory, "debug", "devbox");
}

export function startDaemon(env: Omit<E2EEnv, "gatewayUrl">): ChildProcess {
  return spawn(env.bin, ["daemon", "--foreground"], {
    env: { ...process.env, HOME: env.home, XDG_RUNTIME_DIR: env.runtime, DEVBOX_INSTANCE: env.instance, XDG_DATA_HOME: "", XDG_STATE_HOME: "", XDG_CONFIG_HOME: "" },
    stdio: "ignore",
  });
}

export function makeEnv(): Omit<E2EEnv, "gatewayUrl"> {
  const home = mkdtempSync(join(tmpdir(), "devbox-e2e-"));
  const runtime = join(home, "run");
  mkdirSync(runtime);
  mkdirSync(join(home, "projects", "alpha", ".git"), { recursive: true });
  mkdirSync(join(home, "projects", "beta"), { recursive: true });
  return { home, runtime, instance: `test-${Math.random().toString(16).slice(2, 10)}`, bin: devboxBin() };
}
```

```ts
// app/e2e/global-setup.ts
import { spawn } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { makeEnv, startDaemon } from "./support";

export default async function globalSetup() {
  const env = makeEnv();
  const daemon = startDaemon(env);
  const sock = join(env.runtime, `devbox-${env.instance}.sock`);
  for (let i = 0; i < 200 && !existsSync(sock); i++) await new Promise((r) => setTimeout(r, 50));
  const token = "e2e";
  // E2E는 화면 1460·gateway 1461을 쓴다. 사용자가 켜 둔 `pnpm dev`(1450·1451)와 겹치지 않게 한다(IR-7).
  const port = 1461;
  const gateway = spawn(env.bin, ["dev-gateway", "--port", String(port), "--token", token, "--origin", "http://localhost:1460"], {
    env: { ...process.env, HOME: env.home, XDG_RUNTIME_DIR: env.runtime, DEVBOX_INSTANCE: env.instance },
    stdio: "ignore",
  });
  await new Promise((r) => setTimeout(r, 500));
  const gatewayUrl = `ws://127.0.0.1:${port}/ws?token=${token}`;
  const state = { ...env, gatewayUrl, daemonPid: daemon.pid };
  writeFileSync(join(env.home, "e2e.json"), JSON.stringify(state));
  process.env.DEVBOX_E2E = JSON.stringify(state);
  return async () => {
    gateway.kill();
    try {
      // reconnect 시험이 데몬을 다시 띄우면 e2e.json의 pid가 바뀐다.
      const latest = JSON.parse(readFileSync(join(env.home, "e2e.json"), "utf8"));
      process.kill(latest.daemonPid);
    } catch {}
  };
}
```

```ts
// app/playwright.config.ts
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30_000,
  globalSetup: "./e2e/global-setup.ts",
  use: { baseURL: "http://localhost:1460", locale: "ko-KR", viewport: { width: 1280, height: 800 } },
  // `pnpm dev`는 `vite --port 1450`이라 인자를 덧붙이면 포트가 두 번 들어간다. vite를 직접 부른다.
  webServer: { command: "pnpm exec vite --port 1460 --strictPort", port: 1460, reuseExistingServer: false, env: { VITE_DEVBOX_INSTANCE: "test" } },
});
```

- [ ] **Step 3: E2E 시나리오**

```ts
// app/e2e/projects.spec.ts
import { expect, test } from "@playwright/test";
import { join } from "node:path";

const e2e = () => JSON.parse(process.env.DEVBOX_E2E ?? "{}");

test("add a project from the scan list, see it in the switcher, then remove it", async ({ page }) => {
  const { gatewayUrl, home } = e2e();
  await page.goto(`/?gateway=${encodeURIComponent(gatewayUrl)}`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가" }).click();
  await page.getByRole("button", { name: /alpha/ }).click();
  await expect(page.getByRole("button", { name: "프로젝트 전환" })).toHaveText(/alpha/);

  await page.keyboard.press("Control+Shift+O");
  await expect(page.getByText(join(home, "projects", "alpha"))).toBeVisible();
  await page.getByRole("button", { name: "alpha 제거" }).click();
  await page.getByRole("button", { name: "프로젝트 제거" }).click();
  await expect(page.getByText(join(home, "projects", "alpha"))).toHaveCount(0);
});

test("a relative path shows the typed Korean error", async ({ page }) => {
  await page.goto(`/?gateway=${encodeURIComponent(e2e().gatewayUrl)}`);
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가" }).click();
  await page.getByLabel("폴더 경로").fill("relative/path");
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await expect(page.getByText(/경로를 열 수 없습니다/)).toBeVisible();
});
```

```ts
// app/e2e/reconnect.spec.ts
import { expect, test } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startDaemon } from "./support";

test("daemon restart shows the reconnect banner, then recovers and reloads data", async ({ page }) => {
  const state = JSON.parse(process.env.DEVBOX_E2E ?? "{}");
  await page.goto(`/?gateway=${encodeURIComponent(state.gatewayUrl)}`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  const current = JSON.parse(readFileSync(join(state.home, "e2e.json"), "utf8"));
  process.kill(current.daemonPid);
  await expect(page.getByText(/다시 연결하고 있습니다/)).toBeVisible();
  const next = startDaemon(state);
  writeFileSync(join(state.home, "e2e.json"), JSON.stringify({ ...current, daemonPid: next.pid }));
  await expect(page.getByText("데몬 연결됨")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText(/다시 연결하고 있습니다/)).toHaveCount(0);
});
```

gateway는 WebSocket마다 데몬에 새로 연결한다. 그래서 데몬을 다시 띄우면 화면의 `wsTransport` 재연결만으로 복구된다.

- [ ] **Step 4: CI**

```yaml
# .github/workflows/ci.yml 의 jobs 아래에 추가
  frontend:
    needs: changes
    if: needs.changes.outputs.code == 'true'
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: pnpm exec biome ci .
      - run: pnpm --filter app typecheck
      - run: pnpm --filter app test
  e2e:
    needs: changes
    if: needs.changes.outputs.code == 'true'
    runs-on: ubuntu-24.04
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.98.1
      - uses: Swatinem/rust-cache@v2
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: pnpm --filter app exec playwright install --with-deps chromium
      - run: pnpm --filter app e2e
```

같은 파일의 집계 job을 고친다(IR-1). 필수 검사는 `ci-ok` 하나뿐이므로, 새 job을 여기에 넣지 않으면 그 job이 실패해도 머지된다.

```yaml
  ci-ok:
    if: always()
    needs: [changes, rust, frontend, e2e]
```

- [ ] **Step 5: 확인·커밋·PR D**

```bash
chmod +x scripts/*.sh
pnpm check
git add -A && git commit -m "test(app): run end-to-end journeys against a real daemon"
git push -u origin feat/app/skeleton && gh pr create --fill --body "화면 골격(RPC 클라이언트·rev 규칙·프레임·단축키·팔레트·프로젝트 전환기), dev/check 스크립트, E2E 2개. 01-design §5.5·§8.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

CI 통과 후 squash 머지, worktree 정리.

---

### Task 22: Tauri 껍데기 골격

**Files:**
- Create: `app/src-tauri/Cargo.toml`, `app/src-tauri/build.rs`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json`, `app/src-tauri/icons/*`
- Create: `app/src-tauri/src/main.rs`, `app/src-tauri/src/lib.rs`, `app/src-tauri/src/window.rs`
- Modify: `Cargo.toml`(workspace에 `exclude = ["app/src-tauri"]`)

**Interfaces:**
- Produces:
  - 단일 인스턴스(두 번째 실행은 기존 창을 앞으로)
  - 메인 창 크기: 작업 영역의 80%, 최소 1180×760, 최대 1600×1000. 창 최소 크기는 960×600
  - WebView2 브라우저 가속키 끔
  - `--self-check`(종료 코드 0/1)
  - Task 23이 연결·명령을 붙인다.

- [ ] **Step 0: worktree** — `git worktree add ../devbox-wt/feat-app-windows-shell -b feat/app/windows-shell origin/main`

- [ ] **Step 1: 설정 파일**

```toml
# app/src-tauri/Cargo.toml
[package]
name = "devbox-app"
version = "1.0.0-dev"
edition = "2024"
publish = false

[lib]
name = "devbox_app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.7", features = [] }

[dependencies]
devbox-mux = { path = "../../crates/mux" }
devbox-protocol = { path = "../../crates/protocol" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
tauri = { version = "2.12", features = [] }
tauri-plugin-single-instance = "2"

[target.'cfg(windows)'.dependencies]
winreg = "0.55"
webview2-com = "0.39"
windows = { version = "0.62", features = ["Win32_Foundation", "Win32_System_JobObjects", "Win32_System_Threading", "Win32_Security"] }
```

```json
// app/src-tauri/tauri.conf.json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Devbox",
  "version": "1.0.0-dev",
  "identifier": "io.github.jihoon22lee.devbox",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1450",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "label": "main", "title": "devbox", "width": 1440, "height": 900, "minWidth": 960, "minHeight": 600, "visible": false }],
    "security": { "csp": "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost" }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "resources": { "resources/devbox-linux": "devbox-linux", "resources/devbox-version.txt": "devbox-version.txt" },
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.ico"],
    "windows": { "nsis": { "installMode": "currentUser" } }
  }
}
```

```json
// app/src-tauri/capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main", "popout-*"],
  "permissions": ["core:default", "core:event:default", "core:window:default"]
}
```

아이콘은 v0.9.0 태그의 `apps/devbox-control-center/src-tauri/icons/`에서 가져온다:

```bash
git show v0.9.0:apps/devbox-control-center/src-tauri/icons/icon.ico > app/src-tauri/icons/icon.ico
# 32x32.png, 128x128.png도 같은 방식으로
```

```rust
// app/src-tauri/build.rs
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 2: 창 설정과 진입점**

```rust
// app/src-tauri/src/window.rs
use tauri::{AppHandle, Manager, PhysicalSize};

pub fn configure_main(app: &AppHandle) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window("main") else { return Ok(()) };
    if let Some(m) = w.current_monitor()? {
        let area = m.work_area().size;
        let scale = m.scale_factor();
        let (lw, lh) = (area.width as f64 / scale, area.height as f64 / scale);
        let width = (lw * 0.8).clamp(1180.0, 1600.0).min(lw);
        let height = (lh * 0.8).clamp(760.0, 1000.0).min(lh);
        w.set_size(PhysicalSize::new((width * scale) as u32, (height * scale) as u32))?;
        w.center()?;
    }
    #[cfg(windows)]
    disable_browser_accelerators(&w);
    w.show()?;
    Ok(())
}

#[cfg(windows)]
fn disable_browser_accelerators(w: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;
    let _ = w.with_webview(|wv| {
        // SAFETY: COM calls on the WebView2 controller owned by this window, on its UI thread.
        #[allow(unsafe_code)]
        unsafe {
            if let Ok(core) = wv.controller().CoreWebView2() {
                if let Ok(settings) = core.Settings() {
                    if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                        let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                    }
                }
            }
        }
    });
}

pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

```rust
// app/src-tauri/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    devbox_app_lib::run()
}
```

```rust
// app/src-tauri/src/lib.rs (Task 23에서 연결·명령을 추가)
mod window;

pub fn run() {
    if std::env::args().any(|a| a == "--self-check") {
        std::process::exit(self_check());
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| window::focus_main(app)))
        .setup(|app| {
            window::configure_main(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("devbox app failed to start");
}

/// 설치본이 내장 Linux 바이너리와 같은 버전인지 확인한다(01-design §10).
fn self_check() -> i32 {
    let exe = std::env::current_exe().ok();
    let dir = exe.as_deref().and_then(std::path::Path::parent);
    let Some(dir) = dir else { return 1 };
    let bin = dir.join("devbox-linux");
    let version = std::fs::read_to_string(dir.join("devbox-version.txt")).unwrap_or_default();
    let ok = bin.is_file() && version.trim() == env!("CARGO_PKG_VERSION");
    println!("devbox self-check: binary={} version={} expected={} → {}", bin.is_file(), version.trim(), env!("CARGO_PKG_VERSION"), if ok { "ok" } else { "fail" });
    if ok { 0 } else { 1 }
}
```

설치본에서 리소스는 실행 파일 옆에 놓인다. 위치가 다르면 `tauri::path::BaseDirectory::Resource`로 찾도록 고친다(빌드 결과에서 확인).

- [ ] **Step 3: Windows에서 빌드 확인(수동)** — Task 24의 `scripts/win-dev.ps1`이 생긴 뒤 함께 확인한다. 이 단계에서는 커밋만 한다.

```bash
git add app/src-tauri Cargo.toml && git commit -m "feat(app): add the Tauri shell with single instance and window sizing"
```

---

### Task 23: WSL 확인·바이너리 배치·setup·브리지·창별 채널

**Files:**
- Create: `app/src-tauri/src/wsl.rs`, `app/src-tauri/src/deploy.rs`, `app/src-tauri/src/connection.rs`, `app/src-tauri/src/job.rs`, `app/src-tauri/src/settings.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Create: `app/src/rpc/transport-tauri.ts`, `app/src/shell/ConnectionGate.tsx`
- Modify: `app/src/App.tsx`(Tauri면 `ConnectionGate`로 감쌈)
- Test: `app/src-tauri/src/wsl.rs`의 `#[cfg(test)]`(UTF-16 판별·출력 해석), `app/src/shell/ConnectionGate.test.tsx`

**Interfaces:**
- Consumes:
  - `devbox_mux::{Mux, FrameSplitter, Backoff}`
  - 데몬의 `devbox version`·`devbox setup --json`·`devbox bridge`
- Produces:
  - Tauri 명령:
    - `gate_status() -> GateStatus`
    - `gate_retry(distro: Option<String>)`
    - `rpc_attach(on_frame: Channel)`
    - `rpc_send(raw body)`
    - `rpc_detach()`
  - 이벤트: `devbox://gate`(GateStatus), `devbox://transport`(`"open"|"closed"`, 창별 emit)
  - `GateStatus { state, detail?, distros? }`. `state`는 다음 중 하나다: `checking`, `noWsl`, `chooseDistro`, `wsl1`, `systemdOff`, `deploying`, `setupFailed`, `connecting`, `ready`, `bridgeFailed`.
  - `tauriTransport()`
  - `ConnectionGate({ children })`

- [ ] **Step 1: 실패하는 Rust 테스트(플랫폼 독립 부분)**

```rust
// app/src-tauri/src/wsl.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_decodes_utf16le_wsl_errors() {
        let text = "Wsl/Service/WSL_E_DISTRO_NOT_FOUND\r\n";
        let utf16: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert!(looks_utf16le(&utf16));
        assert_eq!(decode_output(&utf16).trim(), text.trim());
        assert!(!looks_utf16le(b"DEVBOX-BRIDGE/1\n"));
        assert_eq!(error_code(&decode_output(&utf16)), Some("WSL_E_DISTRO_NOT_FOUND".into()));
    }

    #[test]
    fn picks_the_default_or_only_distro() {
        let d = |name: &str, default| Distro { name: name.into(), version: 2, default };
        assert_eq!(pick(&[d("Ubuntu", false)], None).as_deref(), Some("Ubuntu"));
        assert_eq!(pick(&[d("A", false), d("B", true)], None).as_deref(), None, "several distros need a choice");
        assert_eq!(pick(&[d("A", false), d("B", true)], Some("A")).as_deref(), Some("A"));
        assert_eq!(pick(&[d("A", false)], Some("Gone")).as_deref(), Some("A"));
        assert_eq!(pick(&[d("Ubuntu", true), d("docker-desktop", false)], None).as_deref(), Some("Ubuntu"), "internal distros are ignored");
        assert_eq!(pick(&[d("docker-desktop", true)], None), None, "only internal distros means no usable distro");
    }
}
```

Linux에서 돌리려면 `cargo test --manifest-path app/src-tauri/Cargo.toml --lib wsl`이다. 단, Tauri가 Linux GTK를 요구하므로 이 시험은 Windows CI(Task 24)와 Windows 로컬에서만 돌린다.

- [ ] **Step 2: 구현 — wsl.rs**

```rust
// app/src-tauri/src/wsl.rs (테스트 위)
use serde::Serialize;
use std::process::{Command, Output};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Distro {
    pub name: String,
    pub version: u32,
    pub default: bool,
}

#[cfg(windows)]
pub fn distros() -> Vec<Distro> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let Ok(lxss) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss") else { return vec![] };
    let default: String = lxss.get_value("DefaultDistribution").unwrap_or_default();
    lxss.enum_keys()
        .flatten()
        .filter_map(|guid| {
            let k = lxss.open_subkey(&guid).ok()?;
            Some(Distro { name: k.get_value("DistributionName").ok()?, version: k.get_value("Version").unwrap_or(2), default: guid == default })
        })
        .collect()
}

#[cfg(not(windows))]
pub fn distros() -> Vec<Distro> {
    vec![]
}

/// 도구가 내부용으로 등록하는 배포판. 사용자 작업용이 아니므로 고르기 목록에서 뺀다.
/// (이 PC는 Ubuntu와 docker-desktop이 함께 있다. 빼지 않으면 첫 실행마다 고르기 화면이 뜬다.)
pub fn is_internal(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("docker-desktop") || n.starts_with("rancher-desktop") || n.starts_with("podman-machine")
}

/// 저장된 선택이 있으면 그것, 내부용을 뺀 뒤 하나뿐이면 그것, 여러 개면 사용자가 고르게 한다.
pub fn pick(list: &[Distro], saved: Option<&str>) -> Option<String> {
    let user: Vec<&Distro> = list.iter().filter(|d| !is_internal(&d.name)).collect();
    if let Some(s) = saved {
        if user.iter().any(|d| d.name == s) {
            return Some(s.into());
        }
    }
    match user.as_slice() {
        [only] => Some(only.name.clone()),
        _ => None,
    }
}

pub fn wsl(distro: &str) -> Command {
    let mut c = Command::new("wsl.exe");
    c.args(["-d", distro]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

pub fn looks_utf16le(b: &[u8]) -> bool {
    b.len() >= 4 && b.len() % 2 == 0 && b.iter().skip(1).step_by(2).take(16).filter(|x| **x == 0).count() >= 6
}

pub fn decode_output(b: &[u8]) -> String {
    if looks_utf16le(b) {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(b).into_owned()
    }
}

pub fn error_code(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).find(|w| w.starts_with("WSL_E_")).map(String::from)
}

pub fn run(distro: &str, args: &[&str]) -> std::io::Result<Output> {
    wsl(distro).args(args).output()
}

pub fn systemd_on(distro: &str) -> bool {
    run(distro, &["--exec", "cat", "/proc/1/comm"]).map(|o| decode_output(&o.stdout).trim() == "systemd").unwrap_or(false)
}
```

- [ ] **Step 3: 구현 — deploy.rs·job.rs·settings.rs**

```rust
// app/src-tauri/src/deploy.rs
use crate::wsl::{decode_output, wsl};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

const HOME_BIN: &str = "\"$HOME/.local/share/devbox/bin/devbox\"";

pub fn installed_version(distro: &str) -> Option<String> {
    let o = wsl(distro).args(["--exec", "sh", "-c", &format!("{HOME_BIN} version")]).output().ok()?;
    o.status.success().then(|| decode_output(&o.stdout).trim().to_owned())
}

/// 표준 입력으로 바이너리를 보내고 sha256을 확인한 뒤 교체한다(01-design §4.1, PL-11).
pub fn deploy(distro: &str, bin: &Path, version: &str) -> Result<(), String> {
    let bytes = std::fs::read(bin).map_err(|e| format!("내장 바이너리를 읽지 못했습니다: {e}"))?;
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let script = r#"set -e; d="$HOME/.local/share/devbox/bin"; mkdir -p "$d"; t="$d/.devbox-$1.tmp"; cat > "$t"; chmod 755 "$t"; echo "$2  $t" | sha256sum -c --status; mv -f "$t" "$d/devbox-$1"; ln -sfn "devbox-$1" "$d/devbox""#;
    let mut child = wsl(distro)
        .args(["--exec", "sh", "-c", script, "sh", version, &sha])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wsl.exe 실행 실패: {e}"))?;
    child.stdin.take().ok_or("stdin 없음")?.write_all(&bytes).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(decode_output(&out.stderr)) }
}

pub fn setup(distro: &str, instance: &str) -> Result<String, String> {
    let out = wsl(distro)
        .args(["--exec", "env", &format!("DEVBOX_INSTANCE={instance}"), "sh", "-c", &format!("{HOME_BIN} setup --json")])
        .output()
        .map_err(|e| e.to_string())?;
    let text = decode_output(&out.stdout);
    if out.status.success() { Ok(text) } else { Err(if text.is_empty() { decode_output(&out.stderr) } else { text }) }
}
```

```rust
// app/src-tauri/src/job.rs
//! wsl.exe 자식을 앱 수명에 묶는다(앱이 죽으면 같이 종료).
#[cfg(windows)]
pub struct Job(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
#[allow(unsafe_code)]
impl Job {
    pub fn kill_on_close_for(child: &std::process::Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::*;
        // SAFETY: standard Job Object setup; the handle is owned by this struct and closed on drop.
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32).ok()?;
            AssignProcessToJobObject(job, windows::Win32::Foundation::HANDLE(child.as_raw_handle() as _)).ok()?;
            Some(Self(job))
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing our own job handle.
        unsafe { let _ = windows::Win32::Foundation::CloseHandle(self.0); }
    }
}

#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn kill_on_close_for(_: &std::process::Child) -> Option<Self> {
        Some(Self)
    }
}
```

```rust
// app/src-tauri/src/settings.rs
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub distro: Option<String>,
}

fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &Settings) {
    if let Some(p) = path(app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
        let _ = std::fs::write(p, serde_json::to_vec_pretty(s).unwrap_or_default());
    }
}
```

- [ ] **Step 4: 구현 — connection.rs(게이트·브리지·mux)**

```rust
// app/src-tauri/src/connection.rs
use crate::{deploy, job::Job, settings, wsl};
use devbox_mux::{Backoff, FrameSplitter, Mux};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{ChildStdin, Stdio};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody, Request};
use tauri::{AppHandle, Emitter, Manager, Window};

pub const INSTANCE: &str = if cfg!(debug_assertions) { "dev" } else { "prod" };
const MAGIC: &[u8; 16] = b"DEVBOX-BRIDGE/1\n";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GateStatus {
    pub state: &'static str,
    pub detail: Option<String>,
    pub distros: Option<Vec<wsl::Distro>>,
}

impl GateStatus {
    fn of(state: &'static str) -> Self {
        Self { state, detail: None, distros: None }
    }
}

#[derive(Default)]
struct Inner {
    gate: Option<GateStatus>,
    mux: Mux<String>,
    channels: HashMap<String, Channel<InvokeResponseBody>>,
    stdin: Option<ChildStdin>,
    open: bool,
}

#[derive(Clone, Default)]
pub struct Connection(Arc<Mutex<Inner>>);

impl Connection {
    fn set_gate(&self, app: &AppHandle, g: GateStatus) {
        self.0.lock().expect("conn").gate = Some(g.clone());
        let _ = app.emit("devbox://gate", g);
    }

    fn write(&self, bytes: &[u8]) {
        let mut inner = self.0.lock().expect("conn");
        let failed = inner.stdin.as_mut().is_some_and(|s| s.write_all(bytes).and_then(|_| s.flush()).is_err());
        if failed {
            inner.stdin = None;
        }
    }
}

pub fn start(app: AppHandle) {
    let conn = app.state::<Connection>().inner().clone();
    std::thread::Builder::new()
        .name("devbox-connection".into())
        .spawn(move || supervise(app, conn, None))
        .expect("spawn connection thread");
}

fn supervise(app: AppHandle, conn: Connection, forced: Option<String>) {
    conn.set_gate(&app, GateStatus::of("checking"));
    // 내부용 배포판(docker-desktop 등)은 화면에도 보이지 않는다. 그것뿐이면 WSL 배포판이 없는 것과 같다.
    let list: Vec<wsl::Distro> = wsl::distros().into_iter().filter(|d| !wsl::is_internal(&d.name)).collect();
    if list.is_empty() {
        return conn.set_gate(&app, GateStatus::of("noWsl"));
    }
    let mut s = settings::load(&app);
    let Some(distro) = forced.or_else(|| wsl::pick(&list, s.distro.as_deref())) else {
        return conn.set_gate(&app, GateStatus { state: "chooseDistro", detail: None, distros: Some(list) });
    };
    if list.iter().any(|d| d.name == distro && d.version == 1) {
        return conn.set_gate(&app, GateStatus { state: "wsl1", detail: Some(distro), distros: None });
    }
    s.distro = Some(distro.clone());
    settings::save(&app, &s);
    if !wsl::systemd_on(&distro) {
        return conn.set_gate(&app, GateStatus { state: "systemdOff", detail: Some(distro), distros: None });
    }
    let version = env!("CARGO_PKG_VERSION");
    if deploy::installed_version(&distro).as_deref() != Some(version) {
        conn.set_gate(&app, GateStatus::of("deploying"));
        let bin = app.path().resolve("devbox-linux", tauri::path::BaseDirectory::Resource);
        let result = bin.map_err(|e| e.to_string()).and_then(|b| deploy::deploy(&distro, &b, version)).and_then(|_| deploy::setup(&distro, INSTANCE));
        if let Err(e) = result {
            return conn.set_gate(&app, GateStatus { state: "setupFailed", detail: Some(e), distros: None });
        }
    }
    let mut backoff = Backoff::default();
    loop {
        conn.set_gate(&app, GateStatus::of("connecting"));
        match bridge_once(&app, &conn, &distro) {
            Ok(()) => backoff.reset(),
            Err(e) => conn.set_gate(&app, GateStatus { state: "bridgeFailed", detail: Some(e), distros: None }),
        }
        broadcast_state(&app, &conn, false);
        std::thread::sleep(backoff.next());
    }
}

fn bridge_once(app: &AppHandle, conn: &Connection, distro: &str) -> Result<(), String> {
    let mut child = wsl::wsl(distro)
        .args(["--exec", "env", &format!("DEVBOX_INSTANCE={INSTANCE}"), "sh", "-c", "exec \"$HOME/.local/share/devbox/bin/devbox\" bridge"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let _job = Job::kill_on_close_for(&child);
    let mut out = child.stdout.take().ok_or("no stdout")?;
    let mut magic = [0u8; 16];
    if out.read_exact(&mut magic).is_err() || &magic != MAGIC {
        let mut rest = magic.to_vec();
        let _ = out.read_to_end(&mut rest);
        let text = wsl::decode_output(&rest);
        let _ = child.kill();
        return Err(wsl::error_code(&text).unwrap_or(text));
    }
    {
        let mut inner = conn.0.lock().expect("conn");
        inner.stdin = child.stdin.take();
        inner.open = true;
    }
    let reopen = conn.0.lock().expect("conn").mux.reopen_all();
    for f in reopen {
        conn.write(&f);
    }
    conn.set_gate(app, GateStatus::of("ready"));
    broadcast_state(app, conn, true);
    let mut split = FrameSplitter::default();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = out.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let frames = split.push(&buf[..n]).map_err(|e| e.to_string())?;
        let inner = conn.0.lock().expect("conn");
        for f in frames {
            if let Some(label) = inner.mux.inbound(&f) {
                if let Some(ch) = inner.channels.get(label) {
                    let _ = ch.send(InvokeResponseBody::Raw(f));
                }
            }
        }
    }
    let mut inner = conn.0.lock().expect("conn");
    inner.stdin = None;
    inner.open = false;
    let _ = child.kill();
    Ok(())
}

fn broadcast_state(app: &AppHandle, conn: &Connection, open: bool) {
    let labels: Vec<String> = conn.0.lock().expect("conn").channels.keys().cloned().collect();
    for l in labels {
        let _ = app.emit_to(&l, "devbox://transport", if open { "open" } else { "closed" });
    }
}

#[tauri::command]
pub fn gate_status(conn: tauri::State<'_, Connection>) -> Option<GateStatus> {
    conn.0.lock().expect("conn").gate.clone()
}

#[tauri::command]
pub fn gate_retry(app: AppHandle, distro: Option<String>) {
    let conn = app.state::<Connection>().inner().clone();
    std::thread::spawn(move || supervise(app, conn, distro));
}

#[tauri::command]
pub fn rpc_attach(app: AppHandle, window: Window, on_frame: Channel<InvokeResponseBody>, conn: tauri::State<'_, Connection>) {
    let label = window.label().to_owned();
    let (frames, open) = {
        let mut inner = conn.0.lock().expect("conn");
        inner.channels.insert(label.clone(), on_frame);
        let (_, frames) = inner.mux.open(label.clone());
        (frames, inner.open)
    };
    if open {
        for f in frames {
            conn.write(&f);
        }
        let _ = app.emit_to(&label, "devbox://transport", "open");
    }
}

#[tauri::command]
pub fn rpc_send(window: Window, request: Request<'_>, conn: tauri::State<'_, Connection>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("binary body expected".into()) };
    let mut frame = bytes.clone();
    conn.0.lock().expect("conn").mux.outbound(&window.label().to_owned(), &mut frame).map_err(|e| e.to_string())?;
    conn.write(&frame);
    Ok(())
}

#[tauri::command]
pub fn rpc_detach(window: Window, conn: tauri::State<'_, Connection>) {
    detach(&conn, window.label());
}

pub fn detach(conn: &Connection, label: &str) {
    let close = {
        let mut inner = conn.0.lock().expect("conn");
        inner.channels.remove(label);
        inner.mux.close(&label.to_owned())
    };
    if let Some(f) = close {
        conn.write(&f);
    }
}
```

```rust
// app/src-tauri/src/lib.rs (최종)
mod connection;
mod deploy;
mod job;
mod settings;
mod window;
mod wsl;

use tauri::Manager;

pub fn run() {
    if std::env::args().any(|a| a == "--self-check") {
        std::process::exit(self_check());
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| window::focus_main(app)))
        .manage(connection::Connection::default())
        .invoke_handler(tauri::generate_handler![connection::gate_status, connection::gate_retry, connection::rpc_attach, connection::rpc_send, connection::rpc_detach])
        .setup(|app| {
            window::configure_main(app.handle())?;
            connection::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::Destroyed = e {
                connection::detach(w.app_handle().state::<connection::Connection>().inner(), w.label());
            }
        })
        .run(tauri::generate_context!())
        .expect("devbox app failed to start");
}

// self_check()는 Task 22 그대로
```

- [ ] **Step 5: 구현 — Tauri 전송과 게이트 화면(테스트 먼저)**

```tsx
// app/src/shell/ConnectionGate.test.tsx
import { render, screen } from "@testing-library/react";
import { GateView } from "./ConnectionGate";

test("each gate state explains the next step in Korean", () => {
  const { rerender } = render(<GateView status={{ state: "noWsl" }} onRetry={() => {}} />);
  expect(screen.getByText(/WSL이 필요합니다/)).toBeInTheDocument();
  rerender(<GateView status={{ state: "systemdOff", detail: "Ubuntu" }} onRetry={() => {}} />);
  expect(screen.getByText(/systemd/)).toBeInTheDocument();
  expect(screen.getByText(/wsl --shutdown/)).toBeInTheDocument();
  rerender(<GateView status={{ state: "chooseDistro", distros: [{ name: "Ubuntu", version: 2, default: true }, { name: "Debian", version: 2, default: false }] }} onRetry={() => {}} />);
  expect(screen.getByRole("button", { name: "Ubuntu 사용" })).toBeInTheDocument();
  rerender(<GateView status={{ state: "setupFailed", detail: "units FAIL" }} onRetry={() => {}} />);
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
});
```

```ts
// app/src/rpc/transport-tauri.ts
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Transport, TransportState } from "./client";

export function tauriTransport(): Transport {
  const frameCbs = new Set<(f: Uint8Array) => void>();
  const stateCbs = new Set<(s: TransportState) => void>();
  let unlisten: (() => void) | undefined;
  return {
    connect() {
      const ch = new Channel<ArrayBuffer>();
      ch.onmessage = (buf) => {
        const u = new Uint8Array(buf);
        for (const cb of frameCbs) cb(u);
      };
      void listen<TransportState>("devbox://transport", (e) => stateCbs.forEach((cb) => cb(e.payload))).then((u) => {
        unlisten = u;
        return invoke("rpc_attach", { onFrame: ch });
      });
    },
    close() {
      unlisten?.();
      void invoke("rpc_detach");
    },
    send(frame) {
      void invoke("rpc_send", frame);
    },
    onFrame(cb) {
      frameCbs.add(cb);
      return () => frameCbs.delete(cb);
    },
    onState(cb) {
      stateCbs.add(cb);
      return () => stateCbs.delete(cb);
    },
  };
}
```

```tsx
// app/src/shell/ConnectionGate.tsx
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type ReactNode, useEffect, useState } from "react";
import { Button, EmptyState } from "../ui";

export interface GateStatus {
  state: "checking" | "noWsl" | "chooseDistro" | "wsl1" | "systemdOff" | "deploying" | "setupFailed" | "connecting" | "ready" | "bridgeFailed";
  detail?: string | null;
  distros?: { name: string; version: number; default: boolean }[] | null;
}

export function GateView({ status, onRetry }: { status: GateStatus; onRetry: (distro?: string) => void }) {
  const retry = <Button variant="primary" onClick={() => onRetry()}>다시 시도</Button>;
  switch (status.state) {
    case "checking":
    case "connecting":
      return <EmptyState title="WSL의 devbox에 연결하고 있습니다" />;
    case "deploying":
      return <EmptyState title="devbox를 WSL에 설치하고 있습니다" body="처음 실행하거나 업데이트한 뒤 한 번만 합니다." />;
    case "noWsl":
      return <EmptyState title="WSL이 필요합니다" body="관리자 PowerShell에서 wsl --install 을 실행하고 PC를 다시 시작한 뒤 devbox를 여세요." action={retry} />;
    case "wsl1":
      return <EmptyState title={`${status.detail ?? ""}은(는) WSL1입니다`} body="WSL2만 지원합니다. PowerShell에서 wsl --set-version <배포판> 2 를 실행하세요." action={retry} />;
    case "systemdOff":
      return <EmptyState title="WSL에서 systemd를 켜야 합니다" body={`${status.detail ?? "배포판"}의 /etc/wsl.conf 에 [boot] systemd=true 를 넣고, PowerShell에서 wsl --shutdown 을 실행하세요. 실행 중인 WSL 프로그램이 모두 종료됩니다.`} action={retry} />;
    case "chooseDistro":
      return (
        <EmptyState
          title="devbox가 쓸 WSL 배포판을 고르세요"
          action={
            <div style={{ display: "flex", gap: 8, justifyContent: "center" }}>
              {status.distros?.map((d) => (
                <Button key={d.name} variant={d.default ? "primary" : "secondary"} onClick={() => onRetry(d.name)}>
                  {d.name} 사용
                </Button>
              ))}
            </div>
          }
        />
      );
    case "setupFailed":
      return <EmptyState title="WSL 쪽 설치에 실패했습니다" body={status.detail ?? ""} action={retry} />;
    case "bridgeFailed":
      return <EmptyState title="데몬에 연결하지 못했습니다" body={status.detail ?? ""} action={retry} />;
    case "ready":
      return null;
  }
}

export function ConnectionGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<GateStatus>({ state: "checking" });
  useEffect(() => {
    void invoke<GateStatus | null>("gate_status").then((s) => s && setStatus(s));
    const off = listen<GateStatus>("devbox://gate", (e) => setStatus(e.payload));
    return () => void off.then((u) => u());
  }, []);
  if (status.state === "ready" || status.state === "connecting") return <>{children}</>;
  return <GateView status={status} onRetry={(distro) => void invoke("gate_retry", { distro: distro ?? null })} />;
}
```

`App.tsx`에서 Tauri일 때 `<ConnectionGate>`로 `<RpcProvider>`를 감싼다. 연결이 이미 된 뒤 일시적으로 끊기는 상황(`connecting`)에서는 앱 화면을 유지하고 `ConnectionBanner`가 알린다.

- [ ] **Step 6: 통과·커밋**

```bash
pnpm --filter app exec vitest run src/shell/ConnectionGate.test.tsx && pnpm --filter app typecheck
git add app && git commit -m "feat(app): connect the Windows shell to the WSL daemon through a stdio bridge"
```

---

### Task 24: Windows 개발·빌드 스크립트, Windows CI, S0b 실사용 확인

**Files:**
- Create: `scripts/win-sync.ps1`, `scripts/win-dev.ps1`, `scripts/win-build.ps1`, `scripts/dogfood.sh`, `.github/workflows/windows.yml`
- Modify: `docs/superpowers/plans/2026-10-08-devbox-v1/03b-s0b-app.md`(상태 줄), `PROGRESS.md`(S0b 완료·PR 행·현재 위치)

**Interfaces:**
- Produces:
  - `scripts/win-sync.ps1`: WSL 저장소를 `C:\dev\devbox`로 동기화한다. `node_modules`·`target`·`dist`·`.git`은 제외한다. WSL의 `node_modules`는 Linux용이라 Windows에서 따로 설치해야 하기 때문이다(FS-14).
  - `scripts/win-dev.ps1`: 동기화 → WSL에서 Linux 바이너리 빌드 → 리소스 복사 → `pnpm install` → `pnpm --filter app tauri dev`
  - `scripts/win-build.ps1`: 같은 준비 → `tauri build` → 설치 파일 경로 출력(Windows 쪽 Rust·MSVC·pnpm 필요, 00-roadmap §3.3)
  - `scripts/dogfood.sh [브랜치]`(WSL): 그 브랜치 최신 커밋의 `windows.yml` 실행을 찾고(없으면 `workflow_dispatch`로 시작) 끝나기를 기다린 뒤 `devbox-setup` 산출물을 Windows `Downloads\devbox-dogfood\`에 받는다. **직접 써 보기의 기본 경로**다. Windows 쪽 빌드 도구가 없어도 된다
  - `windows.yml`: 다음 중 하나일 때 실행한다.
    - 태그 push
    - 수동 실행
    - `app/src-tauri/**`·`crates/mux/**`·`crates/protocol/**`·워크플로 자체를 바꾼 PR

    하는 일: Linux 바이너리(musl) 빌드 → Windows 빌드 → `--self-check`

- [ ] **Step 1: 스크립트**

```powershell
# scripts/win-sync.ps1
param([string]$WinRoot = "C:\dev\devbox")
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..").Path
New-Item -ItemType Directory -Force $WinRoot | Out-Null
robocopy $repo $WinRoot /MIR /XD node_modules target dist .git gen\schemas /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)" }
$global:LASTEXITCODE = 0
Write-Host "synced $repo -> $WinRoot"
```

```powershell
# scripts/win-dev.ps1
param([ValidateSet("dev", "build")][string]$Mode = "dev", [string]$WinRoot = "C:\dev\devbox", [string]$Distro = "Ubuntu", [switch]$Release)
$ErrorActionPreference = "Stop"
& "$PSScriptRoot\win-sync.ps1" -WinRoot $WinRoot
$linuxRepo = (wsl.exe -d $Distro --exec wslpath -a ((Resolve-Path "$PSScriptRoot\..").Path)).Trim()
$flag = if ($Release) { "--release" } else { "" }
$dir = if ($Release) { "release" } else { "debug" }
$target = (wsl.exe -d $Distro --cd $linuxRepo --exec bash -lc "source ~/.cargo/env && cargo build -q -p devbox-cli --target x86_64-unknown-linux-musl $flag && cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)[`"target_directory`"])'" | Select-Object -Last 1).Trim()
$res = Join-Path $WinRoot "app\src-tauri\resources"
New-Item -ItemType Directory -Force $res | Out-Null
$winBin = (wsl.exe -d $Distro --exec wslpath -w "$target/x86_64-unknown-linux-musl/$dir/devbox").Trim()
Copy-Item $winBin (Join-Path $res "devbox-linux") -Force
(Get-Content (Join-Path $WinRoot "app\package.json") -Raw | ConvertFrom-Json).version | Set-Content -NoNewline (Join-Path $res "devbox-version.txt")
Push-Location $WinRoot
try {
  pnpm install --frozen-lockfile
  if ($Mode -eq "build") { pnpm --filter app tauri build } else { pnpm --filter app tauri dev }
} finally { Pop-Location }
```

```powershell
# scripts/win-build.ps1
param([string]$WinRoot = "C:\dev\devbox", [string]$Distro = "Ubuntu")
& "$PSScriptRoot\win-dev.ps1" -Mode build -Release -WinRoot $WinRoot -Distro $Distro
Get-ChildItem -Recurse "$WinRoot\app\src-tauri\target" -Filter "*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
```

Windows 쪽 Tauri 빌드는 `C:\dev\devbox\app\src-tauri\target`에 산출물을 만든다. WSL의 전역 `CARGO_TARGET_DIR`은 Linux 바이너리에만 쓰이므로, 위 스크립트가 `cargo metadata`로 실제 위치를 찾는다.

- [ ] **Step 2: Windows 워크플로**

```yaml
# .github/workflows/windows.yml
name: Windows
on:
  workflow_dispatch:
  push:
    tags: ["v*"]
  pull_request:
    paths: ["app/src-tauri/**", "crates/mux/**", "crates/protocol/**", ".github/workflows/windows.yml"]
jobs:
  linux-bin:
    runs-on: ubuntu-24.04
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.98.1
        with: { targets: x86_64-unknown-linux-musl }
      - uses: Swatinem/rust-cache@v2
      - run: sudo apt-get update && sudo apt-get install -y musl-tools
      - run: cargo build --release -p devbox-cli --target x86_64-unknown-linux-musl
      - run: |
          mkdir out && cp target/x86_64-unknown-linux-musl/release/devbox out/devbox-linux
          node -p "require('./app/package.json').version" | tr -d '\n' > out/devbox-version.txt
      - uses: actions/upload-artifact@v4
        with: { name: devbox-linux, path: out/, retention-days: 7 }
  windows:
    needs: linux-bin
    runs-on: windows-2025
    timeout-minutes: 40
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { name: devbox-linux, path: app/src-tauri/resources }
      - uses: dtolnay/rust-toolchain@1.98.1
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: app/src-tauri }
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: cargo test --manifest-path app/src-tauri/Cargo.toml --lib
      - run: pnpm --filter app tauri build
      - shell: pwsh
        run: |
          $exe = Get-ChildItem -Recurse app/src-tauri/target/release -Filter Devbox.exe | Select-Object -First 1
          Copy-Item app/src-tauri/resources/* $exe.Directory
          & $exe.FullName --self-check
          if ($LASTEXITCODE -ne 0) { throw "self-check failed" }
      - uses: actions/upload-artifact@v4
        with: { name: devbox-setup, path: app/src-tauri/target/release/bundle/nsis/*.exe, retention-days: 14 }
```

```bash
# scripts/dogfood.sh
#!/usr/bin/env bash
# CI(windows.yml)가 만든 설치 파일을 Windows의 Downloads\devbox-dogfood\ 로 받는다.
# Windows 쪽 Rust·MSVC·pnpm 없이 개발 빌드를 직접 써 보는 기본 경로다.
# 사용: bash scripts/dogfood.sh [브랜치]   (기본: 지금 브랜치. 원격에 push되어 있어야 함)
set -euo pipefail
branch="${1:-$(git rev-parse --abbrev-ref HEAD)}"
git fetch -q origin "$branch"
sha="$(git rev-parse "origin/$branch")"
find_run() {
  gh run list --workflow windows.yml --branch "$branch" --limit 20 --json databaseId,headSha \
    -q "[.[] | select(.headSha == \"$sha\")][0].databaseId // empty"
}
run="$(find_run)"
if [ -z "$run" ]; then
  echo "이 커밋($sha)의 Windows 빌드가 없어 새로 시작합니다. windows.yml이 main에 있어야 합니다."
  gh workflow run windows.yml --ref "$branch"
  for _ in $(seq 1 36); do sleep 5; run="$(find_run)"; [ -n "$run" ] && break; done
  [ -n "$run" ] || { echo "Windows 빌드 실행을 찾지 못했습니다." >&2; exit 1; }
fi
gh run watch "$run" --exit-status
win_user="$(powershell.exe -NoProfile -Command '$env:USERNAME' | tr -d '\r')"
dest="/mnt/c/Users/$win_user/Downloads/devbox-dogfood"
rm -rf "$dest" && mkdir -p "$dest"
gh run download "$run" -n devbox-setup -D "$dest"
ls -1 "$dest"
echo "Windows 탐색기에서 Downloads\\devbox-dogfood 의 setup 파일을 실행하세요."
```

`bash -n scripts/dogfood.sh`(있으면 `shellcheck`도)로 문법을 확인한다.

- [ ] **Step 3: 커밋·push·PR E 열기**

PR E의 `pull_request`가 `windows.yml`을 돌려 설치 파일 산출물을 만든다. 사용자 확인은 그 산출물로 하므로 PR을 먼저 연다. 머지는 Step 5 뒤에 한다.

```bash
git add scripts .github && git commit -m "build: add Windows sync, dev, build and dogfood scripts and the Windows workflow"
git push -u origin feat/app/windows-shell && gh pr create --fill --body "Windows 껍데기(WSL 확인·배치·setup·브리지·창별 채널·게이트 화면), Windows 스크립트·워크플로. 01-design §5.4·§6·§10. 머지는 사용자 실사용 확인 뒤.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

- [ ] **Step 4: S0b 실사용 확인(사용자, 6개)**

WSL에서 `bash scripts/dogfood.sh feat/app/windows-shell`로 설치 파일을 받아 설치한 뒤 확인한다(Windows 쪽 빌드 도구가 있으면 `scripts/win-build.ps1`도 된다).

1. 첫 실행 화면이 "연결하고 있습니다/설치하고 있습니다"를 거쳐 앱 화면으로 바뀌고, 상태 표시줄에 "데몬 연결됨"이 보인다.
2. Ctrl+Shift+O → [프로젝트 추가] → `~/projects`의 폴더 하나 → 제목 표시줄에 이름이 보인다.
3. 앱을 닫았다가 다시 열면 프로젝트가 그대로 있다.
4. PowerShell에서 `wsl --shutdown` → 앱에 "다시 연결하고 있습니다" 배너가 뜨고, 잠시 뒤 사라진다.
5. 앱에서 Ctrl+P는 인쇄 창을 열지 않고, F5는 화면을 새로 고치지 않는다.
6. 바로가기로 두 번째 실행을 하면 기존 창이 앞으로 온다.

- [ ] **Step 5: 상태 갱신·머지**

```bash
sed -i 's/^- 상태: 계획 · 미착수 · 시작 조건: \[03-s0b-skeleton\].*/- 상태: 완료(PR D·E 머지, 실사용 확인 6\/6)/' docs/superpowers/plans/2026-10-08-devbox-v1/03b-s0b-app.md
# PROGRESS.md: S0b 행 완료, PR E 행, 현재 위치(다음: S1 PR F)를 고친다
git add docs && git commit -m "docs(plan): mark S0b complete" && git push
```

필수 CI와 Windows 워크플로가 통과하면 squash 머지, worktree 정리. 사용자에게 S0b 완료와 실사용 확인 결과를 보고하고 S1을 시작한다.
