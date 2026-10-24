import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";

const INTERPRETERS: Record<string, string> = {
  bash: "shell",
  sh: "shell",
  zsh: "shell",
  fish: "shell",
  python: "python",
  python2: "python",
  python3: "python",
  node: "javascript",
  nodejs: "javascript",
  deno: "typescript",
  ruby: "ruby",
  perl: "perl",
  php: "php",
};

/** File names have priority over a shebang; parsers are still loaded by EditorHost lazily. */
export function detectLanguage(path: string, text: string): string | undefined {
  const match = LanguageDescription.matchFilename(languages, path);
  if (match) return match.name.toLowerCase();
  const first = text.slice(0, 512).split(/\r?\n/, 1)[0] ?? "";
  if (!first.startsWith("#!")) return;
  const words = first.slice(2).trim().split(/\s+/);
  let interpreter = words.shift()?.split("/").at(-1);
  if (interpreter === "env") interpreter = words.find((word) => !word.startsWith("-") && !word.includes("="));
  return interpreter ? INTERPRETERS[interpreter] : undefined;
}

export function detectIndent(text: string): string {
  let tabs = 0;
  const widths = new Map<number, number>();
  for (const line of text.split("\n").slice(0, 500)) {
    if (/^\t+\S/.test(line)) tabs++;
    const spaces = /^( +)\S/.exec(line)?.[1]?.length;
    if (spaces && spaces <= 8) widths.set(spaces, (widths.get(spaces) ?? 0) + 1);
  }
  const candidates = [...widths].sort((a, b) => b[1] - a[1] || a[0] - b[0]);
  if (tabs > (candidates[0]?.[1] ?? 0)) return "\t";
  return " ".repeat(candidates[0]?.[0] ?? 2);
}
