import { trapModalFocus } from "../lib/profilePresentation";
import type * as React from "react";

interface Props {
  templateDialogRef: React.RefObject<HTMLElement | null>;
  templateBusy: boolean;
  closeTemplateDialog: () => void;
  wizardTemplateId: string;
  selectWizardTemplate: (templateId: string) => void;
  templates: import("../api").ProfileTemplate[];
  onCreateWizardProfile: () => Promise<void>;
  wizardDraft: import("../lib/profileEditor").ProfileDraft;
  wizardValidation: import("../lib/profileEditor").ProfileDraftValidation | null;
  patchWizardDraft: (changes: Partial<import("../lib/profileEditor").ProfileDraft>) => void;
  templateError: string | null;
}

export function ProjectWizard({
  templateDialogRef,
  templateBusy,
  closeTemplateDialog,
  wizardTemplateId,
  selectWizardTemplate,
  templates,
  onCreateWizardProfile,
  wizardDraft,
  wizardValidation,
  patchWizardDraft,
  templateError,
}: Props) {
  return (
    <section
      className="template-dialog"
      ref={templateDialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="project-wizard-title"
      aria-describedby="project-wizard-description"
      aria-busy={templateBusy}
      onKeyDown={(event) => trapModalFocus(event, closeTemplateDialog, templateBusy)}
    >
      <h2 id="project-wizard-title">새 프로젝트 마법사</h2>
      <p id="project-wizard-description" className="field-help">
        템플릿은 안전한 기본값만 채우며, 기존 프로필이나 프로젝트 파일은 변경하지 않습니다.
      </p>
      <label className="field" htmlFor="wizard-template">
        <span>프로필 템플릿</span>
        <select
          id="wizard-template"
          value={wizardTemplateId}
          disabled={templateBusy}
          onChange={(event) => selectWizardTemplate(event.currentTarget.value)}
        >
          <option value="">직접 입력</option>
          {templates.map((template) => (
            <option key={template.id} value={template.id}>
              {template.name}
            </option>
          ))}
        </select>
      </label>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void onCreateWizardProfile();
        }}
      >
        <label className="field" htmlFor="wizard-name">
          <span>프로젝트 이름</span>
          <input
            id="wizard-name"
            autoFocus
            value={wizardDraft.name}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.name)}
            aria-describedby={wizardValidation?.errors.name ? "wizard-name-error" : undefined}
            onChange={(event) => patchWizardDraft({ name: event.currentTarget.value })}
          />
        </label>
        {wizardValidation?.errors.name && (
          <span id="wizard-name-error" className="field-error" role="alert">
            {wizardValidation.errors.name}
          </span>
        )}
        <label className="field" htmlFor="wizard-windows-path">
          <span>Windows 경로</span>
          <input
            id="wizard-windows-path"
            value={wizardDraft.windowsPath}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.projectPath)}
            aria-describedby={wizardValidation?.errors.projectPath ? "wizard-project-path-error" : undefined}
            onChange={(event) => patchWizardDraft({ windowsPath: event.currentTarget.value })}
          />
        </label>
        <label className="field" htmlFor="wizard-wsl-distro">
          <span>WSL 배포판</span>
          <input
            id="wizard-wsl-distro"
            value={wizardDraft.wslDistro}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.wsl)}
            aria-describedby={wizardValidation?.errors.wsl ? "wizard-wsl-error" : undefined}
            onChange={(event) => patchWizardDraft({ wslDistro: event.currentTarget.value })}
          />
        </label>
        <label className="field" htmlFor="wizard-wsl-path">
          <span>WSL 경로</span>
          <input
            id="wizard-wsl-path"
            value={wizardDraft.wslPath}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.projectPath)}
            aria-describedby={wizardValidation?.errors.projectPath ? "wizard-project-path-error" : undefined}
            onChange={(event) => patchWizardDraft({ wslPath: event.currentTarget.value })}
          />
        </label>
        <label className="field" htmlFor="wizard-git-root">
          <span>Git 루트</span>
          <input
            id="wizard-git-root"
            value={wizardDraft.gitRoot}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.gitRoot)}
            aria-describedby={wizardValidation?.errors.gitRoot ? "wizard-git-root-error" : undefined}
            onChange={(event) => patchWizardDraft({ gitRoot: event.currentTarget.value })}
          />
        </label>
        {wizardValidation?.errors.gitRoot && (
          <span id="wizard-git-root-error" className="field-error" role="alert">
            {wizardValidation.errors.gitRoot}
          </span>
        )}
        <label className="field" htmlFor="wizard-ports">
          <span>예상 포트 (쉼표)</span>
          <input
            id="wizard-ports"
            value={wizardDraft.expectedPortsText}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.expectedPorts)}
            aria-describedby={wizardValidation?.errors.expectedPorts ? "wizard-ports-error" : undefined}
            onChange={(event) => patchWizardDraft({ expectedPortsText: event.currentTarget.value })}
          />
        </label>
        {wizardValidation?.errors.expectedPorts && (
          <span id="wizard-ports-error" className="field-error" role="alert">
            {wizardValidation.errors.expectedPorts}
          </span>
        )}
        <label className="field" htmlFor="wizard-services">
          <span>Run Manager 서비스 ID (쉼표)</span>
          <input
            id="wizard-services"
            value={wizardDraft.serviceRows.map((row) => row.value).join(", ")}
            disabled={templateBusy}
            aria-invalid={Boolean(wizardValidation?.errors.services)}
            aria-describedby={wizardValidation?.errors.services ? "wizard-services-error" : undefined}
            onChange={(event) => {
              const value = event.currentTarget.value;
              patchWizardDraft({
                serviceRows: value.trim()
                  ? value
                      .split(",")
                      .map((service, index) => ({ key: `wizard-service-${index}`, value: service.trim() }))
                  : [],
              });
            }}
          />
        </label>
        {wizardValidation?.errors.services && (
          <span id="wizard-services-error" className="field-error" role="alert">
            {wizardValidation.errors.services}
          </span>
        )}
        {wizardValidation?.errors.projectPath && (
          <div id="wizard-project-path-error" className="field-error form-error" role="alert">
            {wizardValidation.errors.projectPath}
          </div>
        )}
        {wizardValidation?.errors.wsl && (
          <div id="wizard-wsl-error" className="field-error form-error" role="alert">
            {wizardValidation.errors.wsl}
          </div>
        )}
        {templateError && (
          <div className="field-error form-error" role="alert">
            {templateError}
          </div>
        )}
        <div className="actions">
          <button type="submit" className="btn primary" disabled={templateBusy || !wizardValidation?.profile}>
            {templateBusy ? "생성 중…" : "프로젝트 만들기"}
          </button>
          <button type="button" className="btn" disabled={templateBusy} onClick={closeTemplateDialog}>
            취소
          </button>
        </div>
      </form>
    </section>
  );
}
