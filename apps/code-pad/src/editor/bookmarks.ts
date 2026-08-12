import {
  ChangeDesc,
  EditorState,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type Extension,
  type Text,
} from "@codemirror/state";
import {
  GutterMarker,
  EditorView,
  gutter,
  type BlockInfo,
  type ViewUpdate,
} from "@codemirror/view";

/** Bookmark positions are zero-based line numbers in the session schema. */
export function normalizeBookmarkLines(text: string, lines: readonly number[]): number[] {
  const lineCount = text.split("\n").length;
  return Array.from(new Set(lines))
    .filter((line) => Number.isInteger(line) && line >= 0 && line < lineCount)
    .sort((a, b) => a - b);
}

/**
 * Map line bookmarks through a CodeMirror change description. Anchoring at
 * the start of each old line and associating to the character on the right
 * makes a bookmark follow its line when text/newlines are inserted before it.
 * Deleting a bookmarked line maps it to the surviving line at that position;
 * duplicates are collapsed, keeping every position valid.
 */
export function mapBookmarkLines(
  bookmarks: readonly number[],
  oldDoc: Text,
  changes: ChangeDesc,
  newDoc: Text,
): number[] {
  const mapped: number[] = [];
  for (const lineNumber of bookmarks) {
    const oldLine = oldDoc.line(Math.min(Math.max(1, lineNumber + 1), oldDoc.lines));
    const mappedPosition = Math.min(changes.mapPos(oldLine.from, 1), newDoc.length);
    mapped.push(newDoc.lineAt(mappedPosition).number - 1);
  }
  return Array.from(new Set(mapped)).sort((a, b) => a - b);
}

export const setBookmarkLines = StateEffect.define<readonly number[]>();

/** State field used by the gutter and mapped automatically on edits. */
export const bookmarkField = StateField.define<readonly number[]>({
  create: () => [],
  update(value, transaction) {
    let next = transaction.docChanged
      ? mapBookmarkLines(value, transaction.startState.doc, transaction.changes, transaction.state.doc)
      : value;
    if (
      transaction.docChanged
      && next.length === value.length
      && next.every((line, index) => line === value[index])
    ) {
      next = value;
    }
    for (const effect of transaction.effects) {
      if (effect.is(setBookmarkLines)) {
        next = Array.from(new Set(effect.value))
          .filter((line) => Number.isInteger(line) && line >= 0 && line < transaction.state.doc.lines)
          .sort((a, b) => a - b);
      }
    }
    return next;
  },
});

class BookmarkMarker extends GutterMarker {
  elementClass = "cm-bookmark-marker";

  eq(other: GutterMarker): boolean {
    return other instanceof BookmarkMarker;
  }

  toDOM(): Node {
    const marker = document.createElement("span");
    marker.className = "cm-bookmark-marker-dot";
    marker.textContent = "●";
    marker.setAttribute("aria-label", "북마크");
    return marker;
  }
}

const bookmarkMarker = new BookmarkMarker();

function markerSet(view: EditorView) {
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const lineNumber of view.state.field(bookmarkField)) {
    const line = view.state.doc.line(lineNumber + 1);
    builder.add(line.from, line.from, bookmarkMarker);
  }
  return builder.finish();
}

function lineAtBlock(view: EditorView, block: BlockInfo): number {
  return view.state.doc.lineAt(Math.min(block.from, view.state.doc.length)).number - 1;
}

function currentLines(state: EditorState): number[] {
  return [...state.field(bookmarkField)];
}

function toggleLine(view: EditorView, lineNumber: number): boolean {
  const bookmarks = currentLines(view.state);
  const next = bookmarks.includes(lineNumber)
    ? bookmarks.filter((line) => line !== lineNumber)
    : [...bookmarks, lineNumber];
  view.dispatch({ effects: setBookmarkLines.of(next) });
  return true;
}

export const toggleBookmark = (view: EditorView): boolean => {
  const line = view.state.doc.lineAt(view.state.selection.main.head).number - 1;
  return toggleLine(view, line);
};

export const nextBookmark = (view: EditorView): boolean => {
  const bookmarks = currentLines(view.state);
  if (bookmarks.length === 0) return false;
  const current = view.state.doc.lineAt(view.state.selection.main.head).number - 1;
  const next = bookmarks.find((line) => line > current) ?? bookmarks[0];
  const position = view.state.doc.line(next + 1).from;
  view.dispatch({ selection: { anchor: position }, effects: EditorView.scrollIntoView(position) });
  return true;
};

export const previousBookmark = (view: EditorView): boolean => {
  const bookmarks = currentLines(view.state);
  if (bookmarks.length === 0) return false;
  const current = view.state.doc.lineAt(view.state.selection.main.head).number - 1;
  const previous = [...bookmarks].reverse().find((line) => line < current) ?? bookmarks[bookmarks.length - 1];
  const position = view.state.doc.line(previous + 1).from;
  view.dispatch({ selection: { anchor: position }, effects: EditorView.scrollIntoView(position) });
  return true;
};

export interface BookmarkCommands {
  toggle: () => boolean;
  next: () => boolean;
  previous: () => boolean;
}

export function bookmarkExtension(initialBookmarks: readonly number[] = []): Extension {
  return [
    bookmarkField.init((state) => Array.from(new Set(initialBookmarks))
      .filter((line) => Number.isInteger(line) && line >= 0 && line < state.doc.lines)
      .sort((a, b) => a - b)),
    gutter({
      class: "cm-bookmark-gutter",
      markers: markerSet,
      lineMarkerChange: (update: ViewUpdate) =>
        update.docChanged || update.startState.field(bookmarkField) !== update.state.field(bookmarkField),
      domEventHandlers: {
        mousedown(view, block, event) {
          const mouseEvent = event as MouseEvent;
          if (mouseEvent.button !== 0) return false;
          mouseEvent.preventDefault();
          return toggleLine(view, lineAtBlock(view, block));
        },
      },
    }),
  ];
}
