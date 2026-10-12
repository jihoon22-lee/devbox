import * as T from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";

export function Tooltip({ content, children }: { content: ReactNode; children: ReactNode }) {
  return (
    <T.Root delayDuration={400}>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content
          side="right"
          sideOffset={6}
          style={{
            background: "var(--bg-3)",
            color: "var(--fg)",
            padding: "4px 8px",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            boxShadow: "var(--shadow-1)",
          }}
        >
          {content}
        </T.Content>
      </T.Portal>
    </T.Root>
  );
}
export const TooltipProvider = T.Provider;
