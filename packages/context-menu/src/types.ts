export interface ContextMenuActionItem {
  type: "item";
  id: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
}

export interface ContextMenuSubmenu {
  type: "submenu";
  id: string;
  label: string;
  disabled?: boolean;
  danger?: boolean;
  items: readonly ContextMenuEntry[];
}

export interface ContextMenuSeparator {
  type: "separator";
  id?: string;
}

export type ContextMenuEntry =
  | ContextMenuActionItem
  | ContextMenuSubmenu
  | ContextMenuSeparator;
