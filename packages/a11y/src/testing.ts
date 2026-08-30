import axe, { type NodeResult, type Result, type RunOptions } from "axe-core";

const JSDOM_RULES: RunOptions = {
  rules: {
    // jsdom has no layout or canvas implementation, so contrast remains a
    // physical Windows/high-contrast acceptance item. Structural rules stay on.
    "color-contrast": { enabled: false },
  },
};

function describeNode(node: NodeResult): string {
  return `${JSON.stringify(node.target)} (${node.html})`;
}

export async function findA11yViolations(root: Element | Document): Promise<Result[]> {
  const result = await axe.run(root, JSDOM_RULES);
  return result.violations;
}

export async function assertNoA11yViolations(root: Element | Document): Promise<void> {
  const violations = await findA11yViolations(root);
  if (violations.length === 0) return;

  const detail = violations
    .map((violation) => {
      const nodes = violation.nodes.map(describeNode).join("; ");
      return `${violation.id}: ${violation.help} — ${nodes}`;
    })
    .join("\n");
  throw new Error(`접근성 위반 ${violations.length}건\n${detail}`);
}
