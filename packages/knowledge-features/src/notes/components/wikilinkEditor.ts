import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import type { WikilinkCandidate, WikilinkOccurrence } from "../types";

export const setWikilinkOccurrences = StateEffect.define<readonly WikilinkOccurrence[]>();

function occurrenceTitle(link: WikilinkOccurrence): string {
  switch (link.status) {
    case "resolved":
      return `Ctrl/Cmd+click: ${link.resolved_path ?? "linked note"}`;
    case "ambiguous":
      return "같은 이름의 노트가 여러 개입니다";
    case "invalid":
      return "올바르지 않은 위키링크 대상입니다";
    case "missing":
      return "대상 노트가 없습니다";
  }
}

function occurrenceDecorations(
  docLength: number,
  occurrences: readonly WikilinkOccurrence[],
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const link of [...occurrences].sort((left, right) => left.from - right.from)) {
    if (link.from < 0 || link.to <= link.from || link.to > docLength) continue;
    const attributes: Record<string, string> = {
      title: occurrenceTitle(link),
      "data-wikilink-status": link.status,
    };
    if (link.resolved_path) attributes["data-wikilink-path"] = link.resolved_path;
    builder.add(
      link.from,
      link.to,
      Decoration.mark({
        class: `cm-wikilink cm-wikilink-${link.status}`,
        attributes,
      }),
    );
  }
  return builder.finish();
}

const wikilinkField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    let next = value.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(setWikilinkOccurrences)) {
        next = occurrenceDecorations(transaction.state.doc.length, effect.value);
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

function escapedAt(doc: string, from: number): boolean {
  let slashes = 0;
  for (let cursor = from - 1; cursor >= 0 && doc[cursor] === "\\"; cursor -= 1) slashes += 1;
  return slashes % 2 === 1;
}

export function wikilinkCompletionSource(
  loadCandidates: (query: string) => Promise<WikilinkCandidate[]>,
): CompletionSource {
  return async (context: CompletionContext) => {
    const node = syntaxTree(context.state).resolveInner(context.pos, -1);
    if (node.name.toLowerCase().includes("code")) return null;
    const match = context.matchBefore(/\[\[[^\]\n|]{0,256}$/u);
    if (!match || escapedAt(context.state.doc.toString(), match.from)) return null;
    const query = match.text.slice(2);
    if (new TextEncoder().encode(query).byteLength > 256) return null;

    let candidates: WikilinkCandidate[];
    try {
      candidates = await loadCandidates(query);
    } catch {
      return null;
    }
    return {
      from: match.from + 2,
      validFor: /^[^\]\n|]{0,256}$/u,
      options: candidates.map((candidate): Completion => ({
        label: candidate.title || candidate.link_target,
        detail: candidate.link_target,
        type: "text",
        apply(view, _completion, from, to) {
          const alreadyClosed = view.state.sliceDoc(to, to + 2) === "]]";
          const insert = `${candidate.link_target}${alreadyClosed ? "" : "]]"}`;
          view.dispatch({
            changes: { from, to, insert },
            selection: { anchor: from + candidate.link_target.length + 2 },
          });
        },
      })),
    };
  };
}

export function wikilinkEditorExtensions(
  loadCandidates: (query: string) => Promise<WikilinkCandidate[]>,
  onNavigate: (path: string) => void,
): Extension[] {
  return [
    wikilinkField,
    autocompletion({ override: [wikilinkCompletionSource(loadCandidates)] }),
    EditorView.domEventHandlers({
      mousedown(event) {
        if (!(event.ctrlKey || event.metaKey)) return false;
        const element = (event.target as HTMLElement | null)?.closest<HTMLElement>(
          ".cm-wikilink[data-wikilink-path]",
        );
        const path = element?.dataset.wikilinkPath;
        if (!path) return false;
        event.preventDefault();
        onNavigate(path);
        return true;
      },
    }),
  ];
}
