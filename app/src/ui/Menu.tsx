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
          {items.map((item) => (
            <M.Item
              key={item.label}
              className={s.item}
              data-danger={item.danger || undefined}
              disabled={item.disabled}
              onSelect={item.onSelect}
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
