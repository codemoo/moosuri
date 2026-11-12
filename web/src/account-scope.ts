import { createContext, useContext, useLayoutEffect, useMemo } from "react";
import { RequestScope } from "./api";
import type { StreamFrame } from "./event-stream";

export const ScopeRegistry = createContext<Set<RequestScope> | null>(null);

/** Shared with auth invalidation so another tab fences every private request. */
export function useScope(expectedAccountId?: string, lifetime?: string | null) {
  const registry = useContext(ScopeRegistry);
  const ref = useMemo<{ current: RequestScope | null }>(
    () => ({ current: null }),
    [expectedAccountId, lifetime],
  );
  useLayoutEffect(() => {
    const scope = new RequestScope(expectedAccountId);
    ref.current = scope;
    registry?.add(scope);
    return () => {
      scope.close();
      registry?.delete(scope);
    };
  }, [expectedAccountId, registry, ref]);
  return useMemo(
    () => ({
      request: <T>(
        path: string,
        method?: string,
        body?: unknown,
        signal?: AbortSignal,
      ) => ref.current!.request<T>(path, method, body, signal),
      stream: (
        path: string,
        receive: (frame: StreamFrame) => void,
        signal?: AbortSignal,
        opened?: () => void,
      ) => ref.current!.stream(path, receive, signal, opened),
    }),
    [ref],
  );
}
