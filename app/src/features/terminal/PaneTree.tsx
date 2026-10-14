import { SplitPane } from "../../ui";
import { useTerminalFocus } from "./focus";
import type { Pane } from "./store";
import s from "./TerminalPage.module.css";
import { TerminalView } from "./TerminalView";

export function PaneTree({
  pane,
  path = [],
  focused,
  focusRequest,
  onFocus,
  onCwd,
  onRatio,
  onEnded,
}: {
  pane: Pane;
  path?: ("a" | "b")[];
  focused?: string;
  focusRequest?: { session: string; version: number };
  onFocus: (session: string) => void;
  onCwd: (session: string, cwd: string) => void;
  onRatio: (path: ("a" | "b")[], ratio: number) => void;
  onEnded: (session: string) => void;
}) {
  if (pane.kind === "leaf")
    return (
      <div
        className={s.pane}
        data-focused={focused === pane.session || undefined}
        data-terminal-session={pane.session}
        onFocusCapture={() => onFocus(pane.session)}
      >
        <TerminalView
          id={pane.session}
          autoFocus={focused === pane.session}
          focusRequest={
            focused === pane.session && focusRequest?.session === pane.session ? focusRequest.version : undefined
          }
          onBell={() => useTerminalFocus.getState().ring(pane.session)}
          onCwd={(cwd) => onCwd(pane.session, cwd)}
          onClose={() => onEnded(pane.session)}
        />
      </div>
    );
  const props = { focused, focusRequest, onFocus, onCwd, onRatio, onEnded };
  return (
    <SplitPane dir={pane.dir} ratio={pane.ratio} onRatio={(ratio) => onRatio(path, ratio)}>
      <PaneTree pane={pane.a} path={[...path, "a"]} {...props} />
      <PaneTree pane={pane.b} path={[...path, "b"]} {...props} />
    </SplitPane>
  );
}
