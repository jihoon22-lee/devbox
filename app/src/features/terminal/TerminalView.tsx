import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { useRpc } from "../../rpc/RpcProvider";
import { openExternal } from "../../shell/external";
import { useLayout } from "../../shell/layout";
import { Button, useToasts } from "../../ui";
import { setTerminalFont, storedFontSize, usePreferences } from "../settings/preferences";
import { terminalKey } from "./keys";
import { type LinkState, TerminalLink } from "./link";
import { parseOsc7 } from "./osc";
import { TerminalSearch } from "./TerminalSearch";
import s from "./TerminalView.module.css";
import { terminalTheme } from "./theme";

const FONT = '"Cascadia Mono", "D2Coding", monospace';
const DEFAULT_SIZE = 13;
export interface TerminalViewProps {
  id: string;
  readOnly?: boolean;
  scale?: number;
  onCwd?: (path: string) => void;
  onBell?: () => void;
  onTitle?: (title: string) => void;
  onClose?: () => void;
  autoFocus?: boolean;
  focusRequest?: number;
}
export function TerminalView({
  id,
  readOnly = false,
  scale,
  onCwd,
  onBell,
  onTitle,
  onClose,
  autoFocus,
  focusRequest,
}: TerminalViewProps) {
  const client = useRpc();
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<LinkState>("connecting");
  const [searching, setSearching] = useState(false);
  const searchRef = useRef<SearchAddon | undefined>(undefined);
  const termRef = useRef<Terminal | undefined>(undefined);
  const openedRef = useRef(false);
  const callbacks = useRef({ onCwd, onBell, onTitle, autoFocus, focusRequest });
  callbacks.current = { onCwd, onBell, onTitle, autoFocus, focusRequest };
  // Scaled previews always use the upstream grid, even if readOnly was omitted.
  const preview = readOnly || scale !== undefined;
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    let opened = false;
    setState("connecting");
    setSearching(false);
    const warn = (title: string) => {
      if (!disposed) useToasts.getState().push({ tone: "warn", title });
    };
    const external = (uri: string) => {
      void openExternal(uri).catch(() => warn("링크를 열지 못했습니다"));
    };
    const term = new Terminal({
      fontFamily: FONT,
      fontSize: storedFontSize(),
      allowProposedApi: true,
      scrollback: 10_000,
      theme: terminalTheme(),
      disableStdin: preview,
      cursorBlink: !preview,
      linkHandler: { activate: (_event, uri) => external(uri) },
    });
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon((_event, uri) => external(uri)));
    termRef.current = term;
    searchRef.current = search;
    const fitView = () => {
      if (!disposed && opened && !preview) fit.fit();
    };
    const link = new TerminalLink(client, {
      id,
      readOnly: preview,
      sink: {
        write: (bytes, done) => term.write(bytes, done),
        reset: () => term.reset(),
        resize: (cols, rows) => term.resize(cols, rows),
      },
      onState: (next) => {
        if (!disposed) setState(next);
      },
      onInputDropped: () => warn("연결이 끊겨 입력을 보내지 못했습니다"),
    });
    const subscriptions = [
      term.onData((data) => {
        if (!disposed && !preview) link.input(data);
      }),
      term.onBinary((data) => {
        if (!disposed && !preview) link.input(Uint8Array.from(data, (char) => char.charCodeAt(0)));
      }),
      term.onResize(({ cols, rows }) => {
        if (!disposed && !preview) link.resize(cols, rows);
      }),
      term.onBell(() => {
        if (!disposed) callbacks.current.onBell?.();
      }),
      term.onTitleChange((title) => {
        if (!disposed) callbacks.current.onTitle?.(title);
      }),
      term.parser.registerOscHandler(7, (data) => {
        const path = parseOsc7(data);
        if (!disposed && path !== null) callbacks.current.onCwd?.(path);
        return true;
      }),
    ];
    term.attachCustomKeyEventHandler((event) => {
      if (disposed) return false;
      if (event.type !== "keydown" || event.defaultPrevented) return true;
      const action = terminalKey(event, term.hasSelection());
      if (!action) return true;
      event.preventDefault();
      event.stopPropagation();
      if (action === "copy") {
        void Promise.resolve()
          .then(() => {
            if (!disposed) return navigator.clipboard.writeText(term.getSelection());
          })
          .catch(() => warn("클립보드에 복사하지 못했습니다"));
      }
      if (action === "paste" && !preview) {
        void Promise.resolve()
          .then(() => {
            if (!disposed) return navigator.clipboard.readText();
          })
          .then((text) => {
            if (!disposed && text !== undefined) term.paste(text);
          })
          .catch(() => warn("클립보드에서 붙여넣지 못했습니다"));
      }
      if (action === "zoomIn" || action === "zoomOut" || action === "zoomReset") {
        const size =
          action === "zoomReset"
            ? DEFAULT_SIZE
            : Math.min(28, Math.max(9, (term.options.fontSize ?? DEFAULT_SIZE) + (action === "zoomIn" ? 1 : -1)));
        term.options.fontSize = size;
        setTerminalFont(size, false);
        fitView();
      }
      if (action === "search") setSearching(true);
      if (action === "bottom") useLayout.getState().toggleBottom();
      return false;
    });
    const updateFont = () => {
      if (disposed) return;
      term.options.fontSize = usePreferences.getState().terminalFont;
      fitView();
    };
    window.addEventListener("devbox-terminal-font", updateFont);
    const resize = new ResizeObserver(fitView);
    resize.observe(el);
    const theme = new MutationObserver(() => {
      if (disposed) return;
      term.options.theme = terminalTheme();
      fitView();
    });
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "class", "style"] });
    let fontTimer: ReturnType<typeof setTimeout> | undefined;
    const cleanup = () => {
      if (disposed) return;
      disposed = true;
      if (fontTimer !== undefined) clearTimeout(fontTimer);
      window.removeEventListener("devbox-terminal-font", updateFont);
      resize.disconnect();
      theme.disconnect();
      link.dispose();
      for (const subscription of subscriptions) subscription.dispose();
      termRef.current = undefined;
      openedRef.current = false;
      searchRef.current = undefined;
      term.dispose();
    };
    // Give bundled fonts a short head start, then remain usable on a stalled fetch.
    const font = Promise.resolve()
      .then(() => {
        if (!disposed) return document.fonts?.load(`${term.options.fontSize}px D2Coding`);
      })
      .catch(() => undefined);
    const deadline = new Promise<void>((resolve) => {
      fontTimer = setTimeout(resolve, 1500);
    });
    void Promise.race([font, deadline])
      .then(() => {
        if (disposed) return;
        if (fontTimer !== undefined) clearTimeout(fontTimer);
        term.open(el);
        opened = true;
        openedRef.current = true;
        if (new URLSearchParams(location.search).get("renderer") !== "dom") {
          let webgl: WebglAddon | undefined;
          try {
            webgl = new WebglAddon();
            const addon = webgl;
            subscriptions.push(
              addon.onContextLoss(() => {
                if (!disposed) addon.dispose();
              }),
            );
            term.loadAddon(addon);
          } catch {
            webgl?.dispose(); /* xterm 6 retains its default DOM renderer. */
          }
        }
        fitView();
        void link.open(term.cols, term.rows);
        if ((callbacks.current.autoFocus || callbacks.current.focusRequest !== undefined) && !preview) term.focus();
      })
      .catch(() => {
        if (disposed) return;
        setState("lost");
        warn("터미널 화면을 준비하지 못했습니다");
        cleanup();
      });
    void font
      .then(() => {
        if (disposed || !opened) return;
        // Reset the font option to remeasure cells after a delayed font load.
        term.options.fontFamily = "monospace";
        term.options.fontFamily = FONT;
        fitView();
      })
      .catch(() => warn("터미널 글꼴을 적용하지 못했습니다"));
    return cleanup;
  }, [client, id, preview]);
  useEffect(() => {
    // Selection is an explicit intent. Changing callbacks/autoFocus or opening
    // search must not pull focus away from its controls on an existing view.
    if (focusRequest !== undefined && openedRef.current && !preview) termRef.current?.focus();
  }, [focusRequest, preview]);
  const validScale = scale !== undefined && Number.isFinite(scale) && scale > 0 ? scale : undefined;
  return (
    <div className={s.wrap} data-keyscope="terminal">
      <div
        className={s.viewport}
        style={
          validScale
            ? {
                transform: `scale(${validScale})`,
                transformOrigin: "0 0",
                width: `${100 / validScale}%`,
                height: `${100 / validScale}%`,
              }
            : undefined
        }
      >
        <div ref={host} className={s.term} />
      </div>
      {searching && (
        <TerminalSearch
          addon={searchRef.current}
          onClose={() => {
            setSearching(false);
            if (!preview) termRef.current?.focus();
          }}
        />
      )}
      {state !== "live" && (
        <div className={s.overlay} role="status">
          {state === "connecting" && "연결 중"}
          {state === "lost" && "다시 연결 중 · 입력 전송 안 됨"}
          {state === "ended" && (
            <>
              세션이 끝났습니다
              {onClose && (
                <Button size="sm" onClick={onClose}>
                  닫기
                </Button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
