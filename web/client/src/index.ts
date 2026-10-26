import { useCallback, useEffect, useRef, useState } from "react";

export type MutationState<O> =
  | { status: "idle" }
  | { status: "pending" }
  | { status: "success"; data: O }
  | { status: "error"; error: unknown };

/** Per-component request lifecycle shared across rendering hosts. */
export function useMutation<I, O>(
  invoke: (input: I, options: { signal: AbortSignal }) => Promise<O>,
) {
  const [state, setState] = useState<MutationState<O>>({ status: "idle" });
  const current = useRef<AbortController | null>(null);
  useEffect(() => () => {
    current.current?.abort();
    current.current = null;
  }, []);
  const execute = useCallback(async (input: I) => {
    current.current?.abort();
    const controller = new AbortController();
    current.current = controller;
    setState({ status: "pending" });
    try {
      const data = await invoke(input, { signal: controller.signal });
      if (current.current === controller && !controller.signal.aborted) {
        setState({ status: "success", data });
      }
      return data;
    } catch (error) {
      if (current.current === controller && !controller.signal.aborted) {
        setState({ status: "error", error });
      }
      throw error;
    }
  }, [invoke]);
  const reset = useCallback(() => {
    current.current?.abort();
    current.current = null;
    setState({ status: "idle" });
  }, []);
  return { state, execute, reset };
}
