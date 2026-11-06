import { useState } from "react";
import { Button, EmptyState } from "../../ui";
import { AddProjectDialog } from "./AddProjectDialog";
import { useOptionalProjectData, useProjectList } from "./ProjectData";

/** True only once the project list has loaded and is empty; loading and failed loads look like "has projects". */
export function useNoRegisteredProjects() {
  const shared = useOptionalProjectData();
  // Outside the shell's provider (popups) the same list, with the same removed-project filter, is read directly.
  const own = useProjectList(!shared);
  const list = shared ?? own;
  return !!list.data && list.data.items.length === 0;
}

/** The primary action of every empty screen while no project is registered. */
export function AddProjectAction({ onAdded }: { onAdded?: (projectId: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="primary" onClick={() => setOpen(true)}>
        프로젝트 추가
      </Button>
      {/* Portaled dialog events bubble through React; keep them away from a surrounding form. */}
      <span
        style={{ display: "contents" }}
        onSubmit={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        <AddProjectDialog open={open} onOpenChange={setOpen} onAdded={onAdded} />
      </span>
    </>
  );
}

/** The "no project selected" empty state, with the add action when nothing is registered. */
export function NoProjectState({ body }: { body?: string }) {
  const none = useNoRegisteredProjects();
  return (
    <EmptyState
      title={none ? "등록된 프로젝트가 없습니다" : "프로젝트를 선택하세요"}
      body={body}
      action={none ? <AddProjectAction /> : undefined}
    />
  );
}
