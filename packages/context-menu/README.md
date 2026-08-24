# @devbox/context-menu

devbox 앱들이 같은 접근성·위치 규칙을 쓰도록 제공하는 작은 React context menu primitive다.
메뉴 위치, keyboard navigation, focus restore, submenu, separator, disabled/danger 표현만
소유한다. 앱별 메뉴 항목, catalog 조회, destructive confirmation과 실제 action은 소비 앱이
소유한다.

## 기본 사용

```tsx
import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";

const items: readonly ContextMenuEntry[] = [
  { type: "item", id: "copy-path", label: "Copy path", shortcut: "Ctrl+Shift+C" },
  { type: "separator" },
  { type: "item", id: "remove", label: "Remove", danger: true },
];

function Row() {
  const menu = useContextMenu({
    onBeforeOpen: () => selectThisRow(),
  });
  return (
    <>
      <button {...menu.triggerProps}>project</button>
      <ContextMenu
        open={menu.open}
        anchor={menu.anchor}
        restoreFocusTo={menu.restoreFocusTo}
        items={items}
        onSelect={(id) => runAppOwnedAction(id)}
        onClose={menu.close}
        ariaLabel="Project actions"
      />
    </>
  );
}
```

`useContextMenu`는 pointer `contextmenu`, Shift+F10, Menu key만 처리한다. IME composition과
cut/copy/paste/undo 등 다른 keyboard event는 막지 않는다. `onBeforeOpen`은 소비 앱이 우클릭한
row나 selection을 먼저 동기화할 때 사용한다.

같은 menu level 안의 item/submenu `id`는 고유해야 한다. package는 선택된 action ID만 callback에
전달하고 label이나 앱 payload를 저장하지 않는다.

## 동작 계약

- root menu는 pointer 또는 keyboard anchor에서 열고 viewport 밖이면 left/up으로 뒤집은 뒤
  8px safe margin 안으로 clamp한다.
- submenu는 parent 오른쪽을 우선하고 공간이 부족하면 왼쪽으로 뒤집는다.
- 메뉴 밖 pointer 입력, Escape 또는 underlying viewport scroll에서 닫는다. 메뉴 자체의 bounded
  overflow scroll은 유지한다.
- ArrowUp/Down, Home/End, Enter/Space, Escape, ArrowRight/Left와 순환 Tab focus를 지원한다.
- separator와 disabled item은 keyboard 탐색에서 제외한다.
- close 뒤 menu 안에 focus가 남아 있을 때만 원래 trigger로 focus를 복원한다. 앱이 dialog 같은
  다른 target으로 focus를 옮겼다면 덮어쓰지 않는다.
- portal은 `document.body`에 렌더해 app panel의 overflow clipping을 피한다.
- item ID는 callback에만 전달하며 package가 저장·log·clipboard 처리하지 않는다.

## 명시적 비범위

- 메뉴 item 구성과 label 결정
- catalog와 설치 상태 조회
- 위험 action의 confirmation·실행·retry
- row selection과 editor command 구현
- clipboard 내용, raw secret/path masking 정책

이 책임들은 각 앱 적용 PR에서 구현한다.
