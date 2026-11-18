function isLocalHostname(hostname: string): boolean {
  return hostname === "127.0.0.1" || hostname === "localhost";
}

function localAppUrl(port: string): string {
  if (typeof window === "undefined") {
    return `http://127.0.0.1:${port}`;
  }

  return `${window.location.protocol}//${window.location.hostname}:${port}`;
}

export function claimAppUrl(): string {
  const configured = import.meta.env.PUBLIC_CLAIM_APP_URL;
  if (configured) {
    return configured;
  }

  if (
    typeof window !== "undefined" &&
    isLocalHostname(window.location.hostname)
  ) {
    return localAppUrl("4321");
  }

  return "/";
}

export function migrateAppUrl(): string {
  const configured = import.meta.env.PUBLIC_MIGRATE_APP_URL;
  if (configured) {
    return configured;
  }

  if (
    typeof window !== "undefined" &&
    isLocalHostname(window.location.hostname)
  ) {
    return localAppUrl("4322");
  }

  return "/";
}
