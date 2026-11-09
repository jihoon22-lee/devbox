import type { Methods } from "../../rpc/gen/rpc";
import { Button } from "../../ui";
import s from "./Agents.module.css";
import { BRANCH_PREFIX, fillTemplate, previewBranch } from "./model";

type Template = Methods["projects.prompts"]["result"]["items"][number];

/** The project's prompt templates in the new task form. A button fills the prompt from the form's current values. */
export function TemplateButtons({
  templates,
  disabled,
  title,
  baseBranch,
  branch,
  project,
  onFill,
}: {
  templates: Template[];
  disabled: boolean;
  title: string;
  baseBranch: string;
  /** The edited branch name the form sends, if any. */
  branch: string | undefined;
  project: string;
  onFill: (prompt: string) => void;
}) {
  return (
    <>
      <div className={s.templates}>
        {templates.map((template) => (
          <Button
            key={template.name}
            size="sm"
            disabled={disabled}
            onClick={() =>
              onFill(
                fillTemplate(template.body, {
                  title: title.trim(),
                  branch:
                    baseBranch.trim() ||
                    (branch ? `${BRANCH_PREFIX}${branch}` : previewBranch(title.trim(), Date.now())),
                  project,
                }),
              )
            }
          >
            템플릿: {template.name}
          </Button>
        ))}
      </div>
      {templates.length > 0 && (
        <p className={s.muted}>
          브랜치 이름을 직접 고치지 않으면 템플릿의 예상 브랜치는 이름이 겹칠 때 바뀔 수 있습니다.
        </p>
      )}
    </>
  );
}
