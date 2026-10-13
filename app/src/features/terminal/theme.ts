import type { ITheme } from "@xterm/xterm";
export function terminalTheme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const value = (name: string) => css.getPropertyValue(name).trim();
  return {
    background: value("--term-bg"),
    foreground: value("--term-fg"),
    cursor: value("--accent"),
    selectionBackground: value("--term-selection"),
    black: value("--term-black"),
    red: value("--term-red"),
    green: value("--term-green"),
    yellow: value("--term-yellow"),
    blue: value("--term-blue"),
    magenta: value("--term-magenta"),
    cyan: value("--term-cyan"),
    white: value("--term-white"),
    brightBlack: value("--term-bright-black"),
    brightRed: value("--term-bright-red"),
    brightGreen: value("--term-bright-green"),
    brightYellow: value("--term-bright-yellow"),
    brightBlue: value("--term-bright-blue"),
    brightMagenta: value("--term-bright-magenta"),
    brightCyan: value("--term-bright-cyan"),
    brightWhite: value("--term-bright-white"),
  };
}
