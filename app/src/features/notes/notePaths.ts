import type { DocKey } from "../../rpc/gen/rpc";

/** The documents root of the notes vault (`crates/notes` `VAULT`). */
export const VAULT = "vault";
export const noteKey = (path: string): DocKey => ({ root: VAULT, path });
export const fileName = (path: string) => path.split("/").pop() ?? path;
export const stem = (path: string) => fileName(path).replace(/\.md$/i, "");
export const folderOf = (path: string) => path.split("/").slice(0, -1).join("/");
export const joinPath = (folder: string, name: string) => (folder ? `${folder}/${name}` : name);
