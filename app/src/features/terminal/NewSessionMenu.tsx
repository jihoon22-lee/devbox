import { useState } from "react";
import { useRpcQuery } from "../../rpc/query";
import { Button, Dialog, EmptyState, ErrorState, Menu } from "../../ui";

export function NewSessionMenu({
  disabled,
  onNew,
}: {
  disabled: boolean;
  onNew: (command?: string, attachZellij?: string) => Promise<boolean>;
}) {
  const profiles = useRpcQuery("terminal.profiles", {});
  const [zellijOpen, setZellijOpen] = useState(false);
  const zellij = useRpcQuery("terminal.zellij_list", {}, { enabled: zellijOpen });
  return (
    <>
      <Menu
        trigger={<Button disabled={disabled}>새 세션 ▾</Button>}
        items={[
          ...(!profiles.data?.items.length ? [{ label: "기본 셸", onSelect: () => void onNew() }] : []),
          ...(profiles.data?.items.map((profile) => ({
            label: profile.name,
            onSelect: () => void onNew(profile.command),
          })) ?? []),
          ...(profiles.isError ? [{ label: "프로필 다시 불러오기", onSelect: () => void profiles.refetch() }] : []),
          { label: "기존 zellij 세션에 붙기", afterClose: true, onSelect: () => setZellijOpen(true) },
        ]}
      />
      {profiles.isError && <span role="status">프로필을 불러오지 못했습니다</span>}
      <Dialog
        open={zellijOpen}
        onOpenChange={setZellijOpen}
        title="기존 zellij 세션에 붙기"
        footer={<Button onClick={() => setZellijOpen(false)}>닫기</Button>}
      >
        {zellij.isError ? (
          <ErrorState
            title="zellij 목록을 불러오지 못했습니다"
            action={<Button onClick={() => void zellij.refetch()}>다시 시도</Button>}
          />
        ) : !zellij.data ? (
          <p role="status">zellij 세션을 불러오고 있습니다</p>
        ) : !zellij.data.items.length ? (
          <EmptyState title="zellij 세션이 없습니다" />
        ) : (
          zellij.data.items.map((name) => (
            <Button
              key={name}
              disabled={disabled}
              onClick={() => {
                void onNew(undefined, name).then((created) => {
                  if (created) setZellijOpen(false);
                });
              }}
            >
              {name}
            </Button>
          ))
        )}
      </Dialog>
    </>
  );
}
