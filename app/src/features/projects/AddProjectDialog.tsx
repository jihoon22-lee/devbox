import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Button, Dialog, Tabs } from "../../ui";
import { CloneProjectForm } from "./CloneProjectForm";
import { ProjectError } from "./ProjectError";
import s from "./Projects.module.css";
import { useCurrentProject } from "./store";
export function AddProjectDialog(props: { open: boolean; onOpenChange: (open: boolean) => void }) {
  // Each opening starts with an empty path and no stale mutation error.
  return props.open ? <AddProjectContent onOpenChange={props.onOpenChange} /> : null;
}
function AddProjectContent({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  const scan = useRpcQuery("projects.scan", {});
  const add = useRpcMutation("projects.add");
  const clone = useRpcMutation("projects.clone");
  const [tab, setTab] = useState("folder");
  const pending = add.isPending || clone.isPending;
  const qc = useQueryClient();
  const locked = useRef(false);
  const [path, setPath] = useState("");
  const success = (project: { id: string }) => {
    useCurrentProject.getState().set(project.id);
    // Successful registration never becomes a retryable mutation due to refresh failure.
    void qc.invalidateQueries({ queryKey: ["projects.list"] });
    void qc.invalidateQueries({ queryKey: ["projects.scan"] });
    onOpenChange(false);
  };
  const settled = () => {
    locked.current = false;
  };
  const submit = (value: string) => {
    if (locked.current || !value.trim()) return;
    locked.current = true;
    add.mutate({ path: value.trim() }, { onSuccess: success, onSettled: settled });
  };
  const submitClone = (url: string, dir: string) => {
    if (locked.current) return;
    locked.current = true;
    clone.mutate({ url, dir }, { onSuccess: success, onSettled: settled });
  };
  const candidates = scan.data?.candidates.filter((candidate) => !candidate.registered) ?? [];
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!locked.current) onOpenChange(open);
      }}
      title="프로젝트 추가"
      description="WSL 안의 폴더를 고르거나 경로를 입력하세요."
      width={620}
    >
      <Tabs
        label="프로젝트 추가 방법"
        active={tab}
        items={[
          { id: "folder", label: "폴더에서" },
          { id: "clone", label: "Git 주소로" },
        ]}
        onSelect={(id) => {
          if (!locked.current) {
            add.reset();
            clone.reset();
            setTab(id);
          }
        }}
      />
      {tab === "clone" ? (
        <>
          {clone.error && <ProjectError error={clone.error} />}
          {clone.isPending && <p role="status">저장소를 가져오는 중…</p>}
          <CloneProjectForm pending={pending} onEdit={clone.reset} submit={submitClone} />
        </>
      ) : (
        <>
          {add.error && <ProjectError error={add.error} />}
          {add.isPending && <p role="status">프로젝트 추가 중…</p>}
          {scan.isPending && <p role="status">폴더를 검색하는 중…</p>}
          {scan.error && <ProjectError error={scan.error} retry={() => void scan.refetch()} />}
          {scan.isSuccess && candidates.length === 0 && <p>추가할 폴더가 없습니다. 경로를 직접 입력하세요.</p>}
          <ul className={s.list}>
            {candidates.map((candidate) => (
              <li key={candidate.path}>
                <Button
                  variant="ghost"
                  className={s.candidate}
                  disabled={pending}
                  onClick={() => submit(candidate.path)}
                >
                  <span>
                    {candidate.name}
                    <small className={s.path}>{candidate.path}</small>
                  </span>
                  <span>{candidate.isGit ? "Git" : ""}</span>
                </Button>
              </li>
            ))}
          </ul>
          <form
            className={s.form}
            onSubmit={(event) => {
              event.preventDefault();
              submit(path);
            }}
          >
            <input
              className={s.input}
              aria-label="폴더 경로"
              value={path}
              disabled={pending}
              onChange={(event) => {
                add.reset();
                setPath(event.target.value);
              }}
              placeholder="/home/<user>/projects/..."
            />
            <Button type="submit" variant="primary" disabled={pending || !path.trim()}>
              {add.isPending ? "추가 중…" : "추가"}
            </Button>
          </form>
        </>
      )}
    </Dialog>
  );
}
