import { isWslContext } from "../lib/documentPresentation";
import { isProductHosted } from "../../transport";
import { isImeComposing } from "@devbox/a11y";
import { pickFiles } from "../api";
import type * as React from "react";

interface Props {
  pathInput: string;
  hydrated: boolean;
  setPathInput: React.Dispatch<React.SetStateAction<string>>;
  handleOpen: () => void;
  busy: boolean;
  runFileOperation: <T>(operation: () => Promise<T>) => Promise<T | undefined>;
  openPath: (
    path: string,
    metadata?: import("../types").SessionDoc | undefined,
    receivedReference?: string | undefined,
  ) => Promise<import("../types").Doc>;
  contextKey: string;
  renameApplyBusy: boolean;
  recoveryOpen: boolean;
  contextRef: React.RefObject<string>;
  reconnectingRef: React.RefObject<boolean>;
  connectionEpochRef: React.RefObject<number>;
  stateRef: React.RefObject<import("../types").EditorState>;
  dispatchAction: (action: import("../store/documentStore").EditorAction) => import("../types").EditorState;
  enqueueExternalChange: (path: string) => void;
  handleSetWorkspace: () => void;
  handleQuickOpen: () => void;
  state: import("../types").EditorState;
  handleSave: () => void;
  activeDoc: import("../types").Doc | null;
}

export function FileToolbar({
  pathInput,
  hydrated,
  setPathInput,
  handleOpen,
  busy,
  runFileOperation,
  openPath,
  contextKey,
  renameApplyBusy,
  recoveryOpen,
  contextRef,
  reconnectingRef,
  connectionEpochRef,
  stateRef,
  dispatchAction,
  enqueueExternalChange,
  handleSetWorkspace,
  handleQuickOpen,
  state,
  handleSave,
  activeDoc,
}: Props) {
  return (
    <header className="app-header">
      <div className="app-heading">
        <p className="eyebrow">{isProductHosted() ? "WORKSPACE" : "WORKBENCH"}</p>
        <h1>{isProductHosted() ? "파일" : "Code Pad"}</h1>
      </div>
      <div className="file-toolbar">
        <input
          id="path-input"
          className="path-input"
          value={pathInput}
          placeholder="파일 또는 작업 폴더 경로"
          aria-label="열 파일 경로"
          disabled={!hydrated}
          onChange={(event) => setPathInput(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (!isImeComposing(event) && event.key === "Enter") handleOpen();
          }}
        />
        <button type="button" className="toolbar-button" onClick={handleOpen} disabled={busy || !hydrated}>
          파일 열기
        </button>
        {isProductHosted() && (
          <button
            type="button"
            className="toolbar-button"
            disabled={busy || !hydrated}
            onClick={() =>
              void runFileOperation(async () => {
                for (const path of await pickFiles()) await openPath(path);
              })
            }
          >
            파일 선택
          </button>
        )}
        {isProductHosted() && isWslContext(contextKey) && (
          <button
            type="button"
            className="toolbar-button"
            disabled={busy || !hydrated || renameApplyBusy || recoveryOpen}
            onClick={() =>
              void runFileOperation(async () => {
                const context = contextRef.current;
                reconnectingRef.current = true;
                connectionEpochRef.current += 1;
                try {
                  const { reconnectWsl } = await import("../reconnectWsl");
                  await reconnectWsl({
                    current: () => stateRef.current.docs,
                    active: () => contextRef.current === context,
                    replace: (doc) => dispatchAction({ type: "replaceDoc", doc }),
                    conflict: enqueueExternalChange,
                  });
                } finally {
                  reconnectingRef.current = false;
                }
              })
            }
          >
            WSL 다시 연결
          </button>
        )}
        <button type="button" className="toolbar-button" onClick={handleSetWorkspace} disabled={busy || !hydrated}>
          작업 폴더
        </button>
        <button
          type="button"
          className="toolbar-button"
          onClick={handleQuickOpen}
          disabled={!hydrated || !state.workspaceFolder}
        >
          빠른 열기
        </button>
        <button
          type="button"
          className="toolbar-button"
          onClick={handleSave}
          disabled={busy || !hydrated || !activeDoc}
        >
          저장
        </button>
      </div>
    </header>
  );
}
