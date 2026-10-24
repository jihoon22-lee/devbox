import { useState } from "react";
import type { ResponseRule } from "../../../rpc/gen/rpc";
import { useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../../rpc/query";
import { Button, ConfirmDialog } from "../../../ui";
import { useApiTabs } from "../api/store";
import { fixtureToRule } from "./fixtureToRule";
import { ListenerForm } from "./ListenerForm";
import { ReceivedDetail } from "./ReceivedDetail";
import { ReceivedList } from "./ReceivedList";
import { RuleEditor } from "./RuleEditor";
import { toRequest } from "./toRequest";
export function WebhooksPanel({ openApi }: { openApi: () => void }) {
  const [selected, setSelected] = useState("");
  const [received, setReceived] = useState("");
  const [deleting, setDeleting] = useState<string>();
  const [notice, setNotice] = useState("");
  const [preset, setPreset] = useState<{ key: string; rule: ResponseRule }>();
  const listeners = useRpcQuery("webhooks.listeners", {});
  const id = selected || listeners.data?.items[0]?.id || "";
  const listener = listeners.data?.items.find((l) => l.id === id);
  const history = useRpcQuery("webhooks.history", { listenerId: id, before: null, limit: 200 }, { enabled: !!id });
  const detail = useRpcQuery("webhooks.history_get", { id: received }, { enabled: !!received });
  const rules = useRpcQuery("webhooks.rules", { id }, { enabled: !!id });
  useTopicInvalidation("webhooks.received", [["webhooks.history"]]);
  const start = useRpcMutation("webhooks.start");
  const stop = useRpcMutation("webhooks.stop");
  const upsert = useRpcMutation("webhooks.rule_upsert");
  const remove = useRpcMutation("webhooks.rule_delete");
  const fixture = useRpcMutation("webhooks.fixture_save");
  const error =
    listeners.error ||
    history.error ||
    detail.error ||
    rules.error ||
    start.error ||
    stop.error ||
    upsert.error ||
    remove.error ||
    fixture.error;
  function replay() {
    if (!detail.data || !listener) return;
    const draft = toRequest(detail.data, `http://127.0.0.1:${listener.port}`);
    const tabs = useApiTabs.getState();
    const key = tabs.openNew({ kind: "personal" });
    tabs.update(key, {
      draft,
      name: "받은 웹훅",
      notice: "전송 대상을 확인해 주세요. 저장할 때 인증 정보 등 비밀 값은 가려집니다.",
    });
    openApi();
  }
  return (
    <section aria-label="웹훅">
      <h2>웹훅</h2>
      <ListenerForm
        busy={start.isPending}
        onStart={(port, lan) =>
          start.mutate(
            { port, lan },
            {
              onSuccess: (l) => {
                setSelected(l.id);
                setReceived("");
                setPreset(undefined);
                setDeleting(undefined);
                void listeners.refetch();
              },
            },
          )
        }
      />
      {error && <p role="alert">웹훅 작업에 실패했습니다. 입력과 연결 상태를 확인해 주세요.</p>}
      {notice && <p role="status">{notice}</p>}
      <label>
        리스너{" "}
        <select
          aria-label="리스너"
          value={id}
          onChange={(e) => {
            setSelected(e.target.value);
            setReceived("");
            setPreset(undefined);
            setDeleting(undefined);
          }}
        >
          {listeners.data?.items.map((l) => (
            <option key={l.id} value={l.id}>
              {l.port} · {l.lan ? "LAN" : "이 PC"} · {l.running ? "수신 중" : "중지됨"}
            </option>
          ))}
        </select>
      </label>
      {listener && (
        <>
          <p>{listener.addresses.join(", ")}</p>
          {listener.running ? (
            <Button onClick={() => stop.mutate({ id }, { onSuccess: () => void listeners.refetch() })}>
              수신 중지
            </Button>
          ) : (
            <Button
              onClick={() =>
                start.mutate({ port: listener.port, lan: listener.lan }, { onSuccess: () => void listeners.refetch() })
              }
            >
              다시 시작
            </Button>
          )}
          <ReceivedList items={history.data?.items ?? []} select={setReceived} />
          {detail.data && (
            <ReceivedDetail
              detail={detail.data}
              onReplay={replay}
              onFixture={(name) =>
                fixture.mutate(
                  { receivedId: received, name },
                  {
                    onSuccess: (f) => {
                      setPreset({ key: f.id, rule: fixtureToRule(f) });
                      setNotice("fixture를 저장했습니다. 아래 응답 규칙 초안을 편집할 수 있습니다.");
                    },
                  },
                )
              }
            />
          )}
          <RuleEditor
            key={preset?.key ?? id}
            initialRule={preset?.rule}
            items={rules.data?.items ?? []}
            onSave={async (rule) => {
              const saved = await upsert.mutateAsync({ listenerId: id, rule });
              void rules.refetch();
              return saved;
            }}
            onDelete={setDeleting}
          />
        </>
      )}
      <ConfirmDialog
        open={!!deleting}
        title="응답 규칙 삭제"
        body="이 규칙을 삭제할까요?"
        confirmLabel="삭제"
        onCancel={() => setDeleting(undefined)}
        onConfirm={() => {
          if (deleting)
            remove.mutate(
              { listenerId: id, id: deleting },
              {
                onSuccess: () => {
                  setDeleting(undefined);
                  void rules.refetch();
                },
              },
            );
        }}
      />
    </section>
  );
}
