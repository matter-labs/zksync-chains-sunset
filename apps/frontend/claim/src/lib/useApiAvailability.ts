import { useEffect, useState } from "react";

import { apiBaseUrl } from "./api";

export type ApiAvailability = "checking" | "available" | "unavailable";

// Probes the backend `/readyz` endpoint. A 200 response means the backend is
// up and its dependencies (DB, RPC) are healthy; anything else (non-200, 503,
// network failure) is treated as unavailable and triggers the "withdrawals
// not available" banner.
export function useApiAvailability(): ApiAvailability {
  const [status, setStatus] = useState<ApiAvailability>("checking");

  useEffect(() => {
    const controller = new AbortController();
    let cancelled = false;

    async function probe() {
      try {
        const res = await fetch(`${apiBaseUrl()}/readyz`, {
          method: "GET",
          signal: controller.signal,
        });
        if (cancelled) return;
        setStatus(res.status === 200 ? "available" : "unavailable");
      } catch (err) {
        if (cancelled) return;
        if (err instanceof DOMException && err.name === "AbortError") return;
        setStatus("unavailable");
      }
    }

    void probe();

    return () => {
      cancelled = true;
      controller.abort();
    };
  }, []);

  return status;
}
