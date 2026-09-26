import type { ContextCleared } from "./ContextCleared";
import type { Distro } from "./Distro";
import type { EmptyReply } from "./EmptyReply";
import type { RegistrationApplied } from "./RegistrationApplied";
import type { RegistrationPreview } from "./RegistrationPreview";
import type { RegistrySelection } from "./RegistrySelection";
import type { WorkspaceRegistry } from "./WorkspaceRegistry";

export type RegistryResults = {
  snapshot: WorkspaceRegistry;
  save_template: WorkspaceRegistry;
  archive_template: WorkspaceRegistry;
  unbind_imported_profile: WorkspaceRegistry;
  rename: WorkspaceRegistry;
  remove: WorkspaceRegistry;
  preview_windows: RegistrationPreview;
  preview_wsl: RegistrationPreview;
  preview_template_profile_windows: RegistrationPreview;
  preview_template_profile_wsl: RegistrationPreview;
  select_project: RegistrySelection;
  clear_project: ContextCleared;
  list_wsl_distros: Array<Distro>;
  cancel_registration: EmptyReply;
  apply_registration: RegistrationApplied;
};
