import * as M from "@radix-ui/react-dropdown-menu";
import { type ReactNode, useRef } from "react";
import { Kbd } from "./Kbd";
import s from "./Menu.module.css";

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  shortcut?: string[];
  afterClose?: boolean;
}
export function Menu({ trigger, items }: { trigger: ReactNode; items: MenuItem[] }) {
  const afterClose = useRef<(() => void) | undefined>(undefined);
  return (
    <M.Root>
      <M.Trigger asChild>{trigger}</M.Trigger>
      <M.Portal>
        <M.Content
          className={s.content}
          sideOffset={4}
          align="end"
          onCloseAutoFocus={() => {
            const action = afterClose.current;
            afterClose.current = undefined;
            if (action) {
              // Restore trigger focus and release this scope before another takes focus.
              queueMicrotask(action);
            }
          }}
        >
          {items.map((item) => (
            <M.Item
              key={item.label}
              className={s.item}
              data-danger={item.danger || undefined}
              disabled={item.disabled}
              onSelect={() => {
                if (item.afterClose) afterClose.current = item.onSelect;
                else item.onSelect();
              }}
            >
              <span>{item.label}</span>
              {item.shortcut && <Kbd keys={item.shortcut} />}
            </M.Item>
          ))}
        </M.Content>
      </M.Portal>
    </M.Root>
  );
}
