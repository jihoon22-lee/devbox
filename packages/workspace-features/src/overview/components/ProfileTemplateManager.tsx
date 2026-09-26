import { trapModalFocus } from "../lib/profilePresentation";
import { emptyProfileTemplateDraft, templateDraftFromTemplate } from "../lib/profileTemplateEditor";
import type * as React from "react";

interface Props {
  templateDialogRef: React.RefObject<HTMLElement | null>;
  templateBusy: boolean;
  closeTemplateDialog: () => void;
  setTemplateEditing: React.Dispatch<
    React.SetStateAction<import("../lib/profileTemplateEditor").ProfileTemplateDraft | null>
  >;
  templates: import("../api").ProfileTemplate[];
  onDeleteTemplate: (templateId: string) => Promise<void>;
  onSaveTemplate: () => Promise<void>;
  templateEditing: import("../lib/profileTemplateEditor").ProfileTemplateDraft;
  templateValidation: import("../lib/profileTemplateEditor").ProfileTemplateDraftValidation | null;
  patchTemplateDraft: (changes: Partial<import("../lib/profileTemplateEditor").ProfileTemplateDraft>) => void;
  templateError: string | null;
}

export function ProfileTemplateManager({
  templateDialogRef,
  templateBusy,
  closeTemplateDialog,
  setTemplateEditing,
  templates,
  onDeleteTemplate,
  onSaveTemplate,
  templateEditing,
  templateValidation,
  patchTemplateDraft,
  templateError,
}: Props) {
  return (
    <section
      className="template-dialog template-manager"
      ref={templateDialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="template-manager-title"
      aria-describedby="template-manager-description"
      aria-busy={templateBusy}
      onKeyDown={(event) => trapModalFocus(event, closeTemplateDialog, templateBusy)}
    >
      <h2 id="template-manager-title">프로필 템플릿 관리</h2>
      <p id="template-manager-description" className="field-help">
        프로젝트와 환경 파일은 변경하지 않고 Workbench의 재사용 가능한 기본값만 관리합니다.
      </p>
      <div className="template-manager-grid">
        <div className="template-list" aria-label="프로필 템플릿 목록">
          <button
            type="button"
            className="btn"
            disabled={templateBusy}
            onClick={() => setTemplateEditing(emptyProfileTemplateDraft())}
          >
            + 새 템플릿
          </button>
          {templates.map((template) => (
            <div className="template-list-row" key={template.id}>
              <button
                type="button"
                className="template-list-item"
                disabled={templateBusy}
                onClick={() => setTemplateEditing(templateDraftFromTemplate(template))}
              >
                {template.name}
              </button>
              <button
                type="button"
                className="mini"
                disabled={templateBusy}
                aria-label={`${template.name} 템플릿 삭제`}
                onClick={() => void onDeleteTemplate(template.id)}
              >
                ✕
              </button>
            </div>
          ))}
          {templates.length === 0 && <p className="field-help">저장된 템플릿이 없습니다.</p>}
        </div>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void onSaveTemplate();
          }}
        >
          <label className="field" htmlFor="template-name">
            <span>템플릿 이름</span>
            <input
              id="template-name"
              autoFocus
              value={templateEditing.name}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.name)}
              aria-describedby={templateValidation?.errors.name ? "template-name-error" : undefined}
              onChange={(event) => patchTemplateDraft({ name: event.currentTarget.value })}
            />
          </label>
          {templateValidation?.errors.name && (
            <span id="template-name-error" className="field-error" role="alert">
              {templateValidation.errors.name}
            </span>
          )}
          <label className="field" htmlFor="template-windows-path">
            <span>기본 Windows 경로 (선택)</span>
            <input
              id="template-windows-path"
              value={templateEditing.windowsPath}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.projectPath)}
              aria-describedby={templateValidation?.errors.projectPath ? "template-project-path-error" : undefined}
              onChange={(event) => patchTemplateDraft({ windowsPath: event.currentTarget.value })}
            />
          </label>
          <label className="field" htmlFor="template-wsl-distro">
            <span>기본 WSL 배포판 (선택)</span>
            <input
              id="template-wsl-distro"
              value={templateEditing.wslDistro}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.wsl)}
              aria-describedby={templateValidation?.errors.wsl ? "template-wsl-error" : undefined}
              onChange={(event) => patchTemplateDraft({ wslDistro: event.currentTarget.value })}
            />
          </label>
          <label className="field" htmlFor="template-wsl-path">
            <span>기본 WSL 경로 (선택)</span>
            <input
              id="template-wsl-path"
              value={templateEditing.wslPath}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.projectPath)}
              aria-describedby={templateValidation?.errors.projectPath ? "template-project-path-error" : undefined}
              onChange={(event) => patchTemplateDraft({ wslPath: event.currentTarget.value })}
            />
          </label>
          <label className="field" htmlFor="template-git-root">
            <span>기본 Git 루트 (선택)</span>
            <input
              id="template-git-root"
              value={templateEditing.gitRoot}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.gitRoot)}
              aria-describedby={templateValidation?.errors.gitRoot ? "template-git-root-error" : undefined}
              onChange={(event) => patchTemplateDraft({ gitRoot: event.currentTarget.value })}
            />
          </label>
          {templateValidation?.errors.gitRoot && (
            <span id="template-git-root-error" className="field-error" role="alert">
              {templateValidation.errors.gitRoot}
            </span>
          )}
          <label className="field" htmlFor="template-ports">
            <span>기본 예상 포트 (쉼표)</span>
            <input
              id="template-ports"
              value={templateEditing.expectedPortsText}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.expectedPorts)}
              aria-describedby={templateValidation?.errors.expectedPorts ? "template-ports-error" : undefined}
              onChange={(event) => patchTemplateDraft({ expectedPortsText: event.currentTarget.value })}
            />
          </label>
          {templateValidation?.errors.expectedPorts && (
            <span id="template-ports-error" className="field-error" role="alert">
              {templateValidation.errors.expectedPorts}
            </span>
          )}
          <label className="field" htmlFor="template-services">
            <span>기본 서비스 ID (쉼표)</span>
            <input
              id="template-services"
              value={templateEditing.serviceIdsText}
              disabled={templateBusy}
              aria-invalid={Boolean(templateValidation?.errors.services)}
              aria-describedby={templateValidation?.errors.services ? "template-services-error" : undefined}
              onChange={(event) => patchTemplateDraft({ serviceIdsText: event.currentTarget.value })}
            />
          </label>
          {templateValidation?.errors.services && (
            <span id="template-services-error" className="field-error" role="alert">
              {templateValidation.errors.services}
            </span>
          )}
          {templateValidation?.errors.projectPath && (
            <div id="template-project-path-error" className="field-error form-error" role="alert">
              {templateValidation.errors.projectPath}
            </div>
          )}
          {templateValidation?.errors.wsl && (
            <div id="template-wsl-error" className="field-error form-error" role="alert">
              {templateValidation.errors.wsl}
            </div>
          )}
          {templateError && (
            <div className="field-error form-error" role="alert">
              {templateError}
            </div>
          )}
          <div className="actions">
            <button type="submit" className="btn primary" disabled={templateBusy || !templateValidation?.template}>
              {templateBusy ? "저장 중…" : "템플릿 저장"}
            </button>
            <button type="button" className="btn" disabled={templateBusy} onClick={closeTemplateDialog}>
              닫기
            </button>
          </div>
        </form>
      </div>
    </section>
  );
}
