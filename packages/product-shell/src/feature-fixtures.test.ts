import { describe, expect, it } from "vitest";
import registry from "../../../apps/product-feature-fixtures.json";
import { fixtureDescription, routeStatus, type ProductId } from "./api";

interface Fixture { featureId: string; owner: ProductId; route: string; authority: string; availability: string }


describe("registered feature fixtures", () => {
  it.each(registry.entries)("opens $featureId through the shell adapter", async (entry) => {
    const fixture: Fixture = (await import(`../fixtures/features/${entry.featureId}.json`)).default;
    expect(fixture).toBeDefined();
    const description = fixtureDescription(fixture.owner);
    const feature = description.features.find((f) => f.id === fixture.featureId);
    expect(feature?.route).toBe(fixture.route);
    expect(feature?.authority).toBe(fixture.authority);
    const status = await routeStatus(description, fixture.route);
    expect(status.availability).toBe(fixture.availability);
    expect(status.operation.outcome.state).toBe("succeeded");
  });
});
