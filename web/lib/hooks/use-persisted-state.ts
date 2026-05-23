"use client";

import { useCallback, useSyncExternalStore } from "react";

/**
 * A string value persisted in localStorage, exposed as React state.
 *
 * Built on `useSyncExternalStore` rather than `useState` + `useEffect` so that:
 *  - the server and the first client render both use `defaultValue` (no
 *    hydration mismatch — React reconciles to the stored value after hydration);
 *  - there is no `setState` inside an effect;
 *  - updates are reactive within the same tab (we dispatch a `storage` event)
 *    and across tabs (the browser dispatches it natively).
 */
export function usePersistedState(
  key: string,
  defaultValue: string
): [string, (value: string) => void] {
  const subscribe = useCallback(
    (onStoreChange: () => void) => {
      const handler = (event: StorageEvent) => {
        if (event.key === null || event.key === key) onStoreChange();
      };
      window.addEventListener("storage", handler);
      return () => window.removeEventListener("storage", handler);
    },
    [key]
  );

  const getSnapshot = useCallback(
    () => localStorage.getItem(key) ?? defaultValue,
    [key, defaultValue]
  );

  const getServerSnapshot = useCallback(() => defaultValue, [defaultValue]);

  const value = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  const setValue = useCallback(
    (next: string) => {
      localStorage.setItem(key, next);
      // Notify same-tab subscribers (native `storage` events only fire in other tabs).
      window.dispatchEvent(new StorageEvent("storage", { key }));
    },
    [key]
  );

  return [value, setValue];
}
