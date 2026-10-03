import { expect, it } from "vitest";
import { deriveSetupView } from "./setupFlowState";
import type { RestoreInventory } from "@devbox/control-center-features/generated/RestoreInventory";
const inventory: RestoreInventory = {
  checkpoints: [],
  operations: [],
  activeOperation: null,
  update: null,
  installation: {
    phase: "import",
    committed: false,
    recordedOwners: 3,
    clean: false,
    reinstall: false,
    freshHealth: false,
  },
};
it("requires recorded readiness, fresh health and no conflicting operation", () => {
  expect(deriveSetupView(inventory, { state: "none" }).stage).toBe("prepareStores");
  const ready = { ...inventory, installation: { ...inventory.installation, recordedOwners: 4, clean: true } };
  expect(deriveSetupView(ready, { state: "none" }).next).toBe("activateClean");
  expect(deriveSetupView({ ...ready, activeOperation: "other" }, { state: "none" }).next).toBeNull();
  const health = { ...ready, installation: { ...ready.installation, phase: "health" as const } };
  expect(deriveSetupView(health, { state: "none" }).next).toBe("recordHealth");
  expect(
    deriveSetupView({ ...health, installation: { ...health.installation, freshHealth: true } }, { state: "none" }).next,
  ).toBe("commitClean");
  expect(
    deriveSetupView(
      { ...ready, installation: { ...ready.installation, committed: true, phase: "complete" } },
      { state: "none" },
    ).stage,
  ).toBe("complete");
});
it("does not turn reinstall/update/unknown states into clean activation", () => {
  expect(
    deriveSetupView({ ...inventory, installation: { ...inventory.installation, reinstall: true } }, { state: "none" })
      .next,
  ).toBeNull();
  expect(
    deriveSetupView({ ...inventory, installation: { ...inventory.installation, phase: "recover" } }, { state: "none" })
      .stage,
  ).toBe("recover");
});
