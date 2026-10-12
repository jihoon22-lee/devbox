import type { ReactNode } from "react";
import { Button, type ButtonProps } from "./Button";
import { Kbd } from "./Kbd";
import { Tooltip } from "./Tooltip";

export function IconButton({
  label,
  icon,
  onClick,
  shortcut,
  pressed,
  ...rest
}: { label: string; icon: ReactNode; onClick: () => void; shortcut?: string[]; pressed?: boolean } & Omit<
  ButtonProps,
  "children" | "aria-label"
>) {
  return (
    <Tooltip
      content={
        <span>
          {label} {shortcut && <Kbd keys={shortcut} />}
        </span>
      }
    >
      <Button
        {...rest}
        variant="ghost"
        aria-label={label}
        aria-pressed={pressed}
        onClick={onClick}
        style={{ width: "var(--control-lg)", height: "var(--control-lg)", padding: 0, justifyContent: "center" }}
      >
        <span aria-hidden="true">{icon}</span>
      </Button>
    </Tooltip>
  );
}
