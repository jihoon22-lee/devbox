import type { DefinitionView } from "./DefinitionView";
import type { EditPreview } from "./EditPreview";
import type { EditSaved } from "./EditSaved";
import type { TrustPreview } from "./TrustPreview";
import type { WorkspaceRegistry } from "./WorkspaceRegistry";

export type DefinitionsResults = {
  load: DefinitionView;
  preview_trust: TrustPreview;
  approve_trust: WorkspaceRegistry;
  revoke_trust: WorkspaceRegistry;
  cancel: null;
  preview_edit: EditPreview;
  apply_edit: EditSaved;
};
