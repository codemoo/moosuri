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
      onClose: (cleanup:()=>void) => ref.current!.onClose(cleanup),
      localUrl: (blob: Blob) => ref.current!.localUrl(blob),
      releaseUrl: (url: string) => ref.current?.releaseUrl(url),
      imageUrl: (path: string, signal?: AbortSignal) =>
        ref.current!.imageUrl(path, signal),
      upload: <T>(
        id: string,
        blob: Blob,
        progress: (percent: number) => void,
        signal: AbortSignal,
        filename?: string,
      ) => ref.current!.upload<T>(id, blob, progress, signal, filename),
      audioUpload: <T>(id:string,blob:Blob,progress:(percent:number)=>void,signal:AbortSignal,probe?:{provider_id:string;provider_revision:number}) => ref.current!.audioUpload<T>(id,blob,progress,signal,probe),
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
