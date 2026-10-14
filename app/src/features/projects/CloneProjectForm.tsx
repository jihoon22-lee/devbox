import { useState } from "react";
import { Button } from "../../ui";
import { repositoryName } from "./clone-url";
import s from "./Projects.module.css";

export function CloneProjectForm({
  pending,
  onEdit,
  submit,
}: {
  pending: boolean;
  onEdit: () => void;
  submit: (url: string, dir: string) => void;
}) {
  const [url, setUrl] = useState("");
  const [override, setOverride] = useState<string>();
  const name = repositoryName(url.trim());
  const dir = override ?? (name ? `~/projects/${name}` : "");
  return (
    <form
      className={s.cloneForm}
      onSubmit={(event) => {
        event.preventDefault();
        if (name && dir.trim()) submit(url.trim(), dir.trim());
      }}
    >
      <label>
        Git 주소
        <input
          className={s.input}
          aria-label="Git 주소"
          value={url}
          disabled={pending}
          placeholder="https://example.com/team/repo.git"
          onChange={(event) => {
            onEdit();
            setUrl(event.target.value);
          }}
        />
      </label>
      {url.trim() && !name && <p role="alert">저장소 주소가 올바르지 않습니다. 인증 정보가 없는 주소를 입력하세요.</p>}
      <label>
        대상 폴더
        <input
          className={s.input}
          aria-label="대상 폴더"
          value={dir}
          disabled={pending}
          onChange={(event) => {
            onEdit();
            setOverride(event.target.value);
          }}
        />
      </label>
      <Button type="submit" variant="primary" disabled={pending || !name || !dir.trim()}>
        {pending ? "가져오는 중…" : "가져오기"}
      </Button>
    </form>
  );
}
