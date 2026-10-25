import { vi } from "vitest";
import type { DocKey, WorkspaceValidation } from "../../../rpc/gen/rpc";
import { keyId } from "../../documents/offlineJournal";
import type { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { commandContext } from "./lspCommands";
import { textHash } from "./lspProof";
import type { EditContext, ProjectUris } from "./lspWorkspace";

/** Fake daemon captures BEFORE the test sends its rename; never from the response's current read. */
export async function capturedContext(
  rpc: FakeDocumentsDaemon,
  uris: ProjectUris,
  keys: DocKey[],
): Promise<EditContext & { rpc: FakeDocumentsDaemon }> {
  const expected = commandContext({ rpc, uris, current: () => true });
  const targets = await Promise.all(
    keys.map(async (key) => {
      const open = expected.expected?.get(keyId(key));
      const disk = rpc.files.get(keyId(key))!;
      return {
        uri: uris.uri(key),
        diskRev: open?.diskRev ?? disk.rev,
        textHash: await textHash(open?.text ?? disk.text),
        ...(open?.leaseId ? { leaseId: open.leaseId, version: 0 } : {}),
      };
    }),
  );
  return {
    ...expected,
    rpc,
    version: (uri) => (expected.expected?.has(keyId(uris.key(uri))) ? 0 : undefined),
    proof: { token: "owned-token", requestId: "1", rootUri: uris.rootUri, generation: "1", targets },
    validate: vi.fn(async (request: WorkspaceValidation) =>
      request.targets.every((target) => {
        const disk = rpc.files.get(keyId(uris.key(target.uri)));
        return JSON.stringify(disk?.rev ?? null) === JSON.stringify(target.diskRev);
      }),
    ),
    finish: vi.fn(),
  };
}
