import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useRef, useState } from "react";
import type { DefineResult, Definition, DefinitionList, Suggestion } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Banner, Button, Dialog, ErrorState, TextArea } from "../../ui";
import { KeyValueEditor } from "../../ui/KeyValueEditor";
import s from "./DefineDialog.module.css";
import { buildDefinition, type DefinitionForm, initialDefinitionForm, upsertDefinition } from "./definition-form";

export function DefineDialog({
  projectId,
  definition,
  suggestion,
  onClose,
  onSaved,
}: {
  projectId: string;
  definition?: Definition;
  suggestion?: Suggestion;
  onClose: () => void;
  onSaved?: (result: DefineResult) => void;
}) {
  const [form, setForm] = useState(() => initialDefinitionForm(definition, suggestion));
  const [editing, setEditing] = useState(Boolean(definition));
  const [identityLocked, setIdentityLocked] = useState(Boolean(definition));
  const [validation, setValidation] = useState("");
  const [savedNotice, setSavedNotice] = useState(false);
  const locked = useRef(false);
  const mutation = useRpcMutation("runtime.define");
  const list = useRpcQuery("runtime.list", { projectId, cursor: null });
  const qc = useQueryClient();
  const queryKey = ["runtime.list", { projectId, cursor: null }];
  const update = <K extends keyof DefinitionForm>(key: K, value: DefinitionForm[K]) => {
    mutation.reset();
    setValidation("");
    setSavedNotice(false);
    setForm((current) => ({ ...current, [key]: value }));
  };
  const save = () => {
    if (locked.current) return;
    let def: ReturnType<typeof buildDefinition>;
    try {
      def = buildDefinition(form);
    } catch (error) {
      setValidation(error instanceof Error ? error.message : "입력을 확인하세요.");
      return;
    }
    locked.current = true;
    setIdentityLocked(true);
    mutation.mutate(
      { projectId, def, saveTo: form.saveTo },
      {
        onSuccess: async (saved) => {
          // Mutation success commits identity even if the following list refresh fails.
          setEditing(true);
          setForm((current) => ({ ...current, id: saved.definition.def.id }));
          setSavedNotice(true);
          await qc.cancelQueries({ queryKey });
          qc.setQueryData<DefinitionList>(queryKey, (old) => upsertDefinition(old, saved));
          void qc.invalidateQueries({ queryKey });
          onSaved?.(saved);
        },
        onSettled: () => {
          locked.current = false;
        },
      },
    );
  };
  const error = mutation.error ? messageFor(mutation.error.code, mutation.error.detail) : null;
  return (
    <Dialog
      open
      title={editing ? "작업 수정" : "작업 추가"}
      width={660}
      onOpenChange={(open) => {
        if (!open && !locked.current) onClose();
      }}
      description="작업·서비스·예약의 실행 설정을 저장합니다."
    >
      {suggestion && (
        <p>
          자동 감지 제안: {suggestion.id} · {suggestion.from}
        </p>
      )}
      {savedNotice && <p role="status">저장했습니다.</p>}
      {savedNotice &&
        form.saveTo === "thisPc" &&
        list.data?.defs.some(({ def, source }) => def.id === form.id && source === "projectFile") && (
          <Banner tone="info">같은 이름의 프로젝트 파일 정의가 우선합니다. 이 PC 정의는 저장되어 있습니다.</Banner>
        )}
      {list.error && (
        <Banner
          tone="danger"
          actions={
            <Button onClick={() => void list.refetch()} disabled={list.isFetching}>
              다시 불러오기
            </Button>
          }
        >
          실행 목록을 불러오지 못했습니다. 저장한 작업은 수정 모드로 유지됩니다.
        </Banner>
      )}
      {error && <ErrorState title={error.title} action={error.action} diagnosticId={mutation.error?.diagnosticId} />}
      {validation && <Banner tone="danger">{validation}</Banner>}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        <fieldset className={s.form} disabled={mutation.isPending}>
          <Field label="작업 이름 (식별자)">
            <input
              value={form.id}
              readOnly={identityLocked}
              onChange={(event) => update("id", event.target.value)}
              required
            />
          </Field>
          <Field label="종류">
            <select
              value={form.kind}
              disabled={identityLocked}
              onChange={(event) => update("kind", event.target.value as DefinitionForm["kind"])}
            >
              <option value="task">작업</option>
              <option value="service">서비스</option>
              <option value="schedule">예약</option>
            </select>
          </Field>
          {form.kind === "schedule" ? (
            <ScheduleFields form={form} update={update} definitions={list.data?.defs ?? []} />
          ) : (
            <>
              <Field label="명령">
                <TextArea value={form.command} onChange={(event) => update("command", event.target.value)} required />
              </Field>
              <Field label="작업 폴더">
                <input
                  value={form.cwd}
                  onChange={(event) => update("cwd", event.target.value)}
                  placeholder="비우면 프로젝트 폴더"
                />
              </Field>
              <Field label="실행 대상">
                <select
                  value={form.target}
                  onChange={(event) => update("target", event.target.value as DefinitionForm["target"])}
                >
                  <option value="linux">WSL</option>
                  <option value="windows">Windows</option>
                </select>
              </Field>
              <Field label="작업 폴더 참조">
                <input value={form.worktree} onChange={(event) => update("worktree", event.target.value)} />
              </Field>
              <KeyValueEditor
                value={form.env}
                onChange={(value) => update("env", value)}
                disabled={mutation.isPending}
              />
              <Field label="비밀 참조">
                <input
                  value={form.secrets}
                  onChange={(event) => update("secrets", event.target.value)}
                  placeholder="비밀 이름을 쉼표로 구분"
                />
              </Field>
              <Field label="선행 작업·서비스">
                <input
                  value={form.dependencies}
                  onChange={(event) => update("dependencies", event.target.value)}
                  placeholder="식별자를 쉼표로 구분"
                />
              </Field>
              {form.kind === "service" && <ServiceFields form={form} update={update} />}
            </>
          )}
          <Field label="저장 위치">
            <select
              value={form.saveTo}
              onChange={(event) => update("saveTo", event.target.value as DefinitionForm["saveTo"])}
            >
              <option value="thisPc">이 PC에만</option>
              <option value="project">프로젝트 파일에 저장</option>
            </select>
          </Field>
          <div className={s.actions}>
            <Button onClick={onClose}>닫기</Button>
            <Button variant="primary" type="submit">
              {mutation.isPending ? "저장 중…" : "저장"}
            </Button>
          </div>
        </fieldset>
      </form>
    </Dialog>
  );
}
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className={s.field}>
      <span>{label}</span>
      {children}
    </label>
  );
}
type FieldsProps = {
  form: DefinitionForm;
  update: <K extends keyof DefinitionForm>(key: K, value: DefinitionForm[K]) => void;
};
function ScheduleFields({ form, update, definitions }: FieldsProps & { definitions: Definition[] }) {
  return (
    <>
      <Field label="예약할 작업">
        <select required value={form.task} onChange={(event) => update("task", event.target.value)}>
          <option value="">작업 선택</option>
          {form.task && !definitions.some(({ def }) => def.kind === "task" && def.id === form.task) && (
            <option value={form.task}>{form.task}</option>
          )}
          {definitions
            .filter(({ def }) => def.kind === "task")
            .map(({ def }) => (
              <option key={def.id} value={def.id}>
                {def.id}
              </option>
            ))}
        </select>
      </Field>
      <Field label="예약 식">
        <input
          value={form.cron}
          onChange={(event) => update("cron", event.target.value)}
          placeholder="0 9 * * *"
          required
        />
      </Field>
      <Field label="시간대">
        <input
          value={form.tz}
          onChange={(event) => update("tz", event.target.value)}
          placeholder="예: Asia/Seoul (비우면 시스템 시간대)"
        />
      </Field>
      <Field label="놓친 예약">
        <select
          value={form.missed}
          onChange={(event) => update("missed", event.target.value as DefinitionForm["missed"])}
        >
          <option value="skip">건너뛰기</option>
          <option value="runOnce">한 번 실행</option>
        </select>
      </Field>
    </>
  );
}
function ServiceFields({ form, update }: FieldsProps) {
  return (
    <>
      <Field label="재시작 정책">
        <select
          value={form.restart}
          onChange={(event) => update("restart", event.target.value as DefinitionForm["restart"])}
        >
          <option value="no">재시작 안 함</option>
          <option value="onFailure">실패하면 재시작</option>
          <option value="always">항상 재시작</option>
        </select>
      </Field>
      <Field label="포트">
        <input value={form.ports} onChange={(event) => update("ports", event.target.value)} placeholder="3000, 8080" />
      </Field>
      {(
        [
          ["restartSec", "재시작 대기 (초)"],
          ["burst", "재시작 한도 (회)"],
          ["intervalSec", "한도 구간 (초)"],
          ["readyTimeoutSec", "준비 대기 (초)"],
        ] as const
      ).map(([key, label]) => (
        <Field label={label} key={key}>
          <input
            type="number"
            min="0"
            max="4294967295"
            step="1"
            required
            value={form[key]}
            onChange={(event) => update(key, event.target.value)}
          />
        </Field>
      ))}
    </>
  );
}
