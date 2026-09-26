import { componentInvoke, type Component } from "./transport";

type ArgsFor<Calls, Method> = Extract<Calls, { method: Method }> extends { args: infer Args } ? Args : never;
type Arguments<Args> = Record<string, never> extends Args ? [args?: Args] : [args: Args];

/** Native call unions own the method/argument relationship and result map. */
export function bindTypedCall<
  Calls extends { method: string; args: unknown },
  Results extends Record<Calls["method"], unknown>,
>(transport: (method: string, args: Record<string, unknown>) => Promise<unknown>) {
  return <Method extends Calls["method"]>(
    method: Method,
    ...[args]: Arguments<ArgsFor<Calls, Method>>
  ): Promise<Results[Method]> => transport(method, (args ?? {}) as Record<string, unknown>) as Promise<Results[Method]>;
}

export function typedCall<
  Calls extends { method: string; args: unknown },
  Results extends Record<Calls["method"], unknown>,
>(component: Component | ((method: string) => Component)) {
  return bindTypedCall<Calls, Results>(componentInvoke(component));
}
