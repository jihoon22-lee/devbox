import type { DependencyCancelled } from "./DependencyCancelled";
import type { DependencyEnrichmentPreview } from "./DependencyEnrichmentPreview";
import type { DependencyEnrichmentReport } from "./DependencyEnrichmentReport";
import type { DependencyReport } from "./DependencyReport";

export type DependenciesResults = {
  dependency_inventory: DependencyReport;
  dependency_enrichment_preview: DependencyEnrichmentPreview;
  dependency_enrichment_execute: DependencyEnrichmentReport;
  dependency_enrichment_cancel: DependencyCancelled;
};
