import { componentInvoke, type Component } from "./transport";

type CallOf<Calls, Method> = Extract<Calls, { method: Method }>;
export function typedCall<
  Calls extends { method: string; args: unknown },
  Results extends Record<Calls["method"], unknown>,
>(component: Component) {
  const invoke = componentInvoke(component);
  return <Method extends Calls["method"]>(
    method: Method,
    args: CallOf<Calls, Method> extends { args: infer Args } ? Args : never,
  ): Promise<Results[Method]> => invoke<Results[Method]>(method, args as Record<string, unknown>);
}
