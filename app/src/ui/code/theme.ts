import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";

// CSS variables remain live when the app switches between dark and light themes.
// Keep every editor color here, sharing the existing terminal and UI tokens.
export const codeTheme = [
  EditorView.theme({
    "&": {
      color: "var(--term-fg)",
      backgroundColor: "var(--term-bg)",
      fontSize: "var(--text-sm)",
      height: "100%",
    },
    "&.cm-focused": { outline: "none", boxShadow: "var(--focus-ring)" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "var(--leading-code)", overflow: "auto" },
    ".cm-content": { caretColor: "var(--accent)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
      backgroundColor: "var(--term-selection)",
    },
    ".cm-gutters": {
      color: "var(--fg-subtle)",
      backgroundColor: "var(--term-bg)",
      borderColor: "var(--border)",
    },
    ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--bg-3)" },
    ".cm-selectionMatch": { backgroundColor: "var(--selection)" },
    ".cm-searchMatch": { backgroundColor: "var(--selection)", outline: "1px solid var(--warn)" },
    ".cm-searchMatch.cm-searchMatch-selected": { outline: "1px solid var(--accent)" },
    ".cm-matchingBracket": { backgroundColor: "var(--selection)", outline: "1px solid var(--border-strong)" },
    ".cm-panels, .cm-tooltip": {
      color: "var(--fg)",
      backgroundColor: "var(--surface)",
      borderColor: "var(--border)",
    },
    ".cm-textfield, .cm-button": {
      color: "var(--fg)",
      background: "var(--bg-2)",
      borderColor: "var(--border-strong)",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      color: "var(--fg)",
      backgroundColor: "var(--selection)",
    },
  }),
  syntaxHighlighting(
    HighlightStyle.define([
      { tag: tags.comment, color: "var(--fg-muted)", fontStyle: "italic" },
      { tag: [tags.keyword, tags.operator, tags.modifier], color: "var(--term-magenta)" },
      { tag: [tags.string, tags.special(tags.string), tags.regexp], color: "var(--term-green)" },
      { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--term-yellow)" },
      { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--term-blue)" },
      { tag: [tags.typeName, tags.className, tags.namespace], color: "var(--term-yellow)" },
      { tag: [tags.propertyName, tags.attributeName], color: "var(--term-cyan)" },
      { tag: tags.tagName, color: "var(--term-red)" },
      { tag: tags.heading, color: "var(--term-blue)", fontWeight: "bold" },
      { tag: tags.link, color: "var(--term-cyan)", textDecoration: "underline" },
      { tag: tags.emphasis, fontStyle: "italic" },
      { tag: tags.strong, fontWeight: "bold" },
      { tag: tags.invalid, color: "var(--danger)", textDecoration: "underline" },
    ]),
  ),
];
