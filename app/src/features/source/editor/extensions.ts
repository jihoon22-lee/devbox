import { indentWithTab } from "@codemirror/commands";
import { indentUnit } from "@codemirror/language";
import { gotoLine, openSearchPanel } from "@codemirror/search";
import { Prec } from "@codemirror/state";
import { highlightTrailingWhitespace, keymap } from "@codemirror/view";
export function codeExtensions(indent: string) {
  return [
    indentUnit.of(indent),
    highlightTrailingWhitespace(),
    Prec.high(
      keymap.of([
        { key: "Mod-f", run: openSearchPanel, preventDefault: true },
        {
          key: "Mod-h",
          preventDefault: true,
          run: (view) => {
            openSearchPanel(view);
            view.dom.querySelector<HTMLInputElement>('.cm-search input[name="replace"]')?.focus();
            return true;
          },
        },
        { key: "Mod-g", run: gotoLine, preventDefault: true },
        indentWithTab,
      ]),
    ),
  ];
}
