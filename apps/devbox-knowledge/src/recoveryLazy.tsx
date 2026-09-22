import { createContext, lazy, useContext, type ComponentProps, type ComponentType } from "react";

// A boundary owns an attempt even while its Suspense subtree has not committed.
// Weak keys let obsolete attempts (and their rejected promises) be collected.
export const RecoveryAttempt = createContext<object>({});
export function recoveryLazy<Feature extends ComponentType<any>>(load: () => Promise<{ default: Feature }>): ComponentType<ComponentProps<Feature>> {
  type Props = ComponentProps<Feature>;
  const attempts = new WeakMap<object, ComponentType<Props>>();
  return function RecoverableFeature(props: Props) {
    const attempt = useContext(RecoveryAttempt);
    let Feature = attempts.get(attempt);
    if (!Feature) {
      Feature = lazy(load) as ComponentType<Props>;
      attempts.set(attempt, Feature);
    }
    return <Feature {...props}/>;
  };
}
