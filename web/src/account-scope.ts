import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
} from "react";
import { RequestScope } from "./api";

export const ScopeRegistry = createContext<Set<RequestScope> | null>(null);

/** Shared with auth invalidation so another tab fences every private request. */
export function useScope(expectedAccountId?: string) {
  const registry = useContext(ScopeRegistry);
  const ref = useRef<RequestScope | null>(null);
  useLayoutEffect(() => {
    const scope = new RequestScope(expectedAccountId);
    ref.current = scope;
    registry?.add(scope);
    return () => {
      scope.close();
      registry?.delete(scope);
    };
  }, [expectedAccountId, registry]);
  return useMemo(
    () => ({
      request: <T>(path: string, method?: string, body?: unknown) =>
        ref.current!.request<T>(path, method, body),
    }),
    [],
  );
}
