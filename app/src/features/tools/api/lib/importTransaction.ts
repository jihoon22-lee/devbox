/**
 * 가져오기 쓰기(v0.9.0 `importTransaction` 적응). 새 파일만 `expect: absent`로 쓰고, 중간에 실패하면 거기서 멈춘다.
 * 되돌리기는 이번에 쓴 파일을 그 digest일 때만 지운다(그 사이 바뀐 파일은 남긴다).
 * `api.collection_delete`는 확인 정책 always이므로 화면이 되돌리기 전에 확인 창을 한 번 보인다.
 */
import type { CollectionRoot } from "../../../../rpc/gen/rpc";
import type { ApiBackend } from "../backend";

export interface WrittenFile {
  path: string;
  digest: string;
}

export interface ImportWrite {
  written: WrittenFile[];
  failed: { path: string; error: unknown } | null;
}

export async function writeImport(
  backend: Pick<ApiBackend, "write">,
  root: CollectionRoot,
  files: { path: string; text: string }[],
): Promise<ImportWrite> {
  const written: WrittenFile[] = [];
  for (const file of files) {
    try {
      const { digest } = await backend.write(root, file.path, file.text, { kind: "absent" });
      written.push({ path: file.path, digest });
    } catch (error) {
      return { written, failed: { path: file.path, error } };
    }
  }
  return { written, failed: null };
}

export async function rollbackImport(
  backend: Pick<ApiBackend, "remove">,
  root: CollectionRoot,
  written: WrittenFile[],
): Promise<{ removed: string[]; kept: string[] }> {
  const removed: string[] = [];
  const kept: string[] = [];
  for (const file of written) {
    try {
      await backend.remove(root, file.path, file.digest);
      removed.push(file.path);
    } catch {
      kept.push(file.path);
    }
  }
  return { removed, kept };
}
