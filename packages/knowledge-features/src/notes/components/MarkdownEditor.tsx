import { useEffect, useMemo, useRef, useState } from "react";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { baseEditorExtensions, markdownEditorExtensions } from "@devbox/editor";
import { defaultKeymap } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { readClipboardImage, readClipboardText } from "../api";
import {
  clipboardImageFiles,
  droppedImageFiles,
  IMAGE_BUSY_ERROR,
  IMAGE_MULTIPLE_ERROR,
  IMAGE_STALE_ERROR,
  IMAGE_TOO_LARGE_ERROR,
  imageErrorMessage,
} from "../lib/imageAssets";
import type {
  EditorCursorRequest,
  ImageAsset,
  WikilinkCandidate,
  WikilinkOccurrence,
} from "../types";
import {
  hasSelectedText,
  insertMarkdownLink,
  removeSelectedText,
  selectedText,
} from "./editorActions";
import { setWikilinkOccurrences, wikilinkEditorExtensions } from "./wikilinkEditor";

// 공용 CodeMirror 설정은 packages/editor(@devbox/editor)에서 가져온다 (PR 24).
// LSP는 넣지 않는다 (knowledge-base는 노트 앱이다).
interface Props {
  value: string;
  onChange: (text: string) => void;
  onSave: () => void;
  onError: (message: string | null) => void;
  wikilinks?: readonly WikilinkOccurrence[];
  loadWikilinkCandidates?: (query: string) => Promise<WikilinkCandidate[]>;
  onNavigateWikilink?: (path: string) => void;
  cursorRequest?: EditorCursorRequest | null;
  /** Stable identity of the currently edited note; prevents cross-note stale inserts. */
  documentKey?: string | null;
  onImageImport?: (file: File) => Promise<ImageAsset>;
}

export default function MarkdownEditor({
  value,
  onChange,
  onSave,
  onError,
  wikilinks = [],
  loadWikilinkCandidates,
  onNavigateWikilink,
  cursorRequest = null,
  documentKey = null,
  onImageImport,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const syncingValueRef = useRef(false);
  const [hasSelection, setHasSelection] = useState(false);
  const [imageBusy, setImageBusy] = useState(false);
  const imageBusyRef = useRef(false);
  const imageTokenRef = useRef(0);
  const mountedRef = useRef(true);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const loadCandidatesRef = useRef(loadWikilinkCandidates);
  loadCandidatesRef.current = loadWikilinkCandidates;
  const navigateWikilinkRef = useRef(onNavigateWikilink);
  navigateWikilinkRef.current = onNavigateWikilink;
  const documentKeyRef = useRef<string | null>(documentKey);
  documentKeyRef.current = documentKey;
  const onImageImportRef = useRef(onImageImport);
  onImageImportRef.current = onImageImport;
  const importImageRef = useRef<(file: File, dropPosition?: number) => Promise<boolean>>(
    async () => false,
  );
  const editorMenu = useContextMenu();
  const openMenuRef = useRef(editorMenu.openAt);
  openMenuRef.current = editorMenu.openAt;

  const menuItems = useMemo<readonly ContextMenuEntry[]>(
    () => [
      { type: "item", id: "cut", label: "잘라내기", shortcut: "Ctrl+X", disabled: !hasSelection },
      { type: "item", id: "copy", label: "복사", shortcut: "Ctrl+C", disabled: !hasSelection },
      { type: "item", id: "paste", label: "붙여넣기", shortcut: "Ctrl+V", disabled: imageBusy },
      { type: "separator", id: "link-separator" },
      { type: "item", id: "insert-link", label: "링크 삽입" },
    ],
    [hasSelection, imageBusy],
  );

  useEffect(() => {
    if (!mountRef.current) return;
    mountedRef.current = true;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          baseEditorExtensions(),
          markdownEditorExtensions(),
          wikilinkEditorExtensions(
            (query) => loadCandidatesRef.current?.(query) ?? Promise.resolve([]),
            (path) => navigateWikilinkRef.current?.(path),
          ),
          keymap.of([
            ...defaultKeymap,
            { key: "Mod-s", run: () => { onSaveRef.current(); return true; }, preventDefault: true },
          ]),
          EditorView.domEventHandlers({
            contextmenu(event, currentView) {
              if (currentView.compositionStarted) return false;
              event.preventDefault();
              const point = { x: event.clientX, y: event.clientY };
              try {
                const position = currentView.posAtCoords(point);
                const insideSelection = position !== null && currentView.state.selection.ranges.some(
                  (range) => !range.empty && position >= range.from && position <= range.to,
                );
                if (position !== null && !insideSelection) {
                  currentView.dispatch({ selection: EditorSelection.cursor(position) });
                }
              } catch {
                // jsdom이나 초기 layout처럼 좌표 해석이 불가능한 경우 현재 selection을 유지한다.
              }
              setHasSelection(hasSelectedText(currentView.state));
              openMenuRef.current(point, currentView.contentDOM);
              return true;
            },
            keydown(event, currentView) {
              if (
                event.isComposing
                || event.keyCode === 229
                || !(
                  event.key === "ContextMenu"
                  || event.code === "ContextMenu"
                  || (event.shiftKey && event.key === "F10")
                )
              ) {
                return false;
              }
              event.preventDefault();
              const rect = currentView.contentDOM.getBoundingClientRect();
              setHasSelection(hasSelectedText(currentView.state));
              openMenuRef.current(
                {
                  x: rect.left + Math.min(24, Math.max(0, rect.width / 2)),
                  y: rect.bottom,
                },
                currentView.contentDOM,
              );
              return true;
            },
            paste(event, currentView) {
              const clipboardEvent = event as ClipboardEvent;
              const isComposing = (clipboardEvent as ClipboardEvent & { isComposing?: boolean }).isComposing;
              if (currentView.compositionStarted || isComposing) return false;
              const files = clipboardImageFiles(clipboardEvent.clipboardData);
              if (files.length === 0 || !onImageImportRef.current) return false;
              event.preventDefault();
              if (files.length > 1) {
                onErrorRef.current(IMAGE_MULTIPLE_ERROR);
                return true;
              }
              void importImageRef.current(files[0]);
              return true;
            },
            dragover(event) {
              const files = droppedImageFiles((event as DragEvent).dataTransfer);
              if (files.length === 0 || !onImageImportRef.current) return false;
              event.preventDefault();
              return true;
            },
            drop(event, currentView) {
              const dragEvent = event as DragEvent;
              const files = droppedImageFiles(dragEvent.dataTransfer);
              if (files.length === 0 || !onImageImportRef.current) return false;
              event.preventDefault();
              if (files.length > 1) {
                onErrorRef.current(IMAGE_MULTIPLE_ERROR);
                return true;
              }
              let position = currentView.state.selection.main.head;
              try {
                position = currentView.posAtCoords({
                  x: dragEvent.clientX,
                  y: dragEvent.clientY,
                }) ?? position;
              } catch {
                // WebViews can reject coordinate mapping while the editor has
                // no layout yet. The current selection is a safe fallback.
              }
              currentView.dispatch({ selection: EditorSelection.cursor(position) });
              void importImageRef.current(files[0], position);
              return true;
            },
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !syncingValueRef.current) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
        ],
      }),
      parent: mountRef.current,
    });
    viewRef.current = view;
    return () => {
      mountedRef.current = false;
      imageTokenRef.current += 1;
      view.destroy();
      viewRef.current = null;
    };
    // 마운트 수명은 컴포넌트 수명과 같다 (파일 전환은 value 동기화로 처리)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 외부 값 동기화 (파일 전환/저장 취소)
  useEffect(() => {
    const view = viewRef.current;
    if (!view || view.state.doc.toString() === value) return;
    syncingValueRef.current = true;
    try {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
    } finally {
      syncingValueRef.current = false;
    }
  }, [value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({ effects: setWikilinkOccurrences.of(wikilinks) });
  }, [wikilinks]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !cursorRequest) return;
    const lineNumber = Math.min(Math.max(cursorRequest.line, 1), view.state.doc.lines);
    const line = view.state.doc.line(lineNumber);
    const columnOffset = Math.min(Math.max(cursorRequest.column - 1, 0), line.length);
    const position = line.from + columnOffset;
    view.dispatch({
      selection: EditorSelection.cursor(position),
      effects: EditorView.scrollIntoView(position, { y: "center" }),
    });
    view.focus();
  }, [cursorRequest]);

  const importImage = async (file: File, dropPosition?: number): Promise<boolean> => {
    const callback = onImageImportRef.current;
    const view = viewRef.current;
    if (!callback || !view) return false;
    if (imageBusyRef.current) {
      onErrorRef.current(IMAGE_BUSY_ERROR);
      return true;
    }

    const token = ++imageTokenRef.current;
    const documentBefore = view.state.doc.toString();
    const documentKeyBefore = documentKeyRef.current;
    const selectionBefore = dropPosition === undefined
      ? view.state.selection.main
      : { from: dropPosition, to: dropPosition };
    imageBusyRef.current = true;
    if (mountedRef.current) setImageBusy(true);
    onErrorRef.current(null);
    try {
      const asset = await callback(file);
      if (
        !mountedRef.current
        || token !== imageTokenRef.current
        || viewRef.current !== view
        || documentKeyRef.current !== documentKeyBefore
      ) {
        if (mountedRef.current && token === imageTokenRef.current && documentKeyRef.current !== documentKeyBefore) {
          onErrorRef.current(IMAGE_STALE_ERROR);
        }
        return true;
      }
      if (view.state.doc.toString() !== documentBefore) {
        onErrorRef.current(IMAGE_STALE_ERROR);
        return true;
      }
      const from = Math.min(Math.max(selectionBefore.from, 0), view.state.doc.length);
      const to = Math.min(Math.max(selectionBefore.to, from), view.state.doc.length);
      view.dispatch({ changes: { from, to, insert: asset.markdown } });
      view.focus();
    } catch (cause) {
      if (mountedRef.current && token === imageTokenRef.current) {
        onErrorRef.current(imageErrorMessage(cause));
      }
    } finally {
      if (token === imageTokenRef.current) {
        imageBusyRef.current = false;
        if (mountedRef.current) setImageBusy(false);
      }
    }
    return true;
  };
  importImageRef.current = importImage;

  const runAction = async (id: string) => {
    const view = viewRef.current;
    if (!view) return;
    if (id === "paste" && imageBusyRef.current) {
      onErrorRef.current(IMAGE_BUSY_ERROR);
      return;
    }
    onErrorRef.current(null);
    if (id === "copy" || id === "cut") {
      const text = selectedText(view.state);
      if (!text) return;
      await navigator.clipboard.writeText(text);
      if (id === "cut") view.dispatch(removeSelectedText(view.state));
      return;
    }
    if (id === "paste") {
      let image: File | null = null;
      if (onImageImportRef.current) {
        try {
          image = await readClipboardImage();
        } catch (cause) {
          if (cause instanceof Error && cause.message === IMAGE_TOO_LARGE_ERROR) {
            // A known oversized image must not silently turn into a text paste;
            // the user needs a bounded, actionable result for this explicit
            // image action. Capability/permission failures still use the text
            // fallback below.
            onErrorRef.current(IMAGE_TOO_LARGE_ERROR);
            return;
          }
          // Clipboard API image reads are optional in WebView2; text paste
          // remains the explicit fallback when the capability is absent.
        }
      }
      if (image) {
        await importImageRef.current(image);
        return;
      }
      const text = await readClipboardText();
      view.dispatch(view.state.replaceSelection(text));
      return;
    }
    if (id === "insert-link") {
      const requested = prompt("링크 URL");
      const url = requested?.trim();
      if (url) view.dispatch(insertMarkdownLink(view.state, url));
    }
  };

  const onSelect = (id: string) => {
    void runAction(id).catch((cause: unknown) => {
      onErrorRef.current(cause instanceof Error ? cause.message : String(cause));
    });
  };

  return (
    <>
      <div
        ref={mountRef}
        className="editor codemirror-editor"
        aria-busy={imageBusy}
        aria-label="Markdown 편집기"
      >
        {imageBusy && (
          <div className="image-upload-status" role="status" aria-live="polite">
            이미지 저장 중…
          </div>
        )}
      </div>
      <ContextMenu
        open={editorMenu.open}
        anchor={editorMenu.anchor}
        items={menuItems}
        onSelect={onSelect}
        onClose={editorMenu.close}
        restoreFocusTo={editorMenu.restoreFocusTo}
        ariaLabel="Markdown 편집기 작업"
      />
    </>
  );
}
