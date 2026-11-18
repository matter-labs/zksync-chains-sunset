import type { PropsWithChildren, ReactNode } from "react";

import { HeaderIdentity } from "@shared/ui/HeaderIdentity";

import type { IdentityState } from "./types";

type PillVariant = "default" | "warning";

type Props = PropsWithChildren<{
  identity: IdentityState;
  title: string;
  eyebrow: ReactNode;
  panelCopy: string;
  footerNote: string;
  footerHref: string;
  footerLabel: string;
  onClearIdentity: () => void;
  disabledBanner?: ReactNode;
  pillVariant?: PillVariant;
}>;

export function AppShell({
  identity,
  title,
  eyebrow,
  panelCopy,
  footerNote,
  footerHref,
  footerLabel,
  onClearIdentity,
  disabledBanner,
  pillVariant = "default",
  children,
}: Props) {
  const pillDotSrc =
    pillVariant === "warning"
      ? "/assets/pill-dot-yellow.svg"
      : "/assets/pill-dot.svg";
  return (
    <>
      <header className="topbar">
        <div className="topbar__container">
          <div className="brand">
            <img
              className="brand__logo"
              src="/assets/header-logo.svg"
              width="165"
              height="32"
              alt="zkSync Lite"
              decoding="async"
            />
          </div>
          <HeaderIdentity
            identity={identity}
            onClearIdentity={onClearIdentity}
          />
        </div>
      </header>

      <main className="page">
        <section className="hero">
          <img
            className="hero__corner hero__corner--tl"
            src="/assets/corner-b.svg"
            width="14"
            height="14"
            alt=""
            aria-hidden="true"
            decoding="async"
          />
          <img
            className="hero__corner hero__corner--tr"
            src="/assets/corner-a.svg"
            width="14"
            height="14"
            alt=""
            aria-hidden="true"
            decoding="async"
          />
          <img
            className="hero__corner hero__corner--bl"
            src="/assets/corner-b.svg"
            width="14"
            height="14"
            alt=""
            aria-hidden="true"
            decoding="async"
          />
          <img
            className="hero__corner hero__corner--br"
            src="/assets/corner-a.svg"
            width="14"
            height="14"
            alt=""
            aria-hidden="true"
            decoding="async"
          />

          <div className="pill">
            <img
              className="pill__dot"
              src={pillDotSrc}
              width="12"
              height="12"
              alt=""
              aria-hidden="true"
              decoding="async"
            />
            <span>{eyebrow}</span>
          </div>

          <h1 className="hero__title">{title}</h1>

          <div className="panel">
            <img
              className="panel__corner panel__corner--tl"
              src="/assets/corner-b.svg"
              width="14"
              height="14"
              alt=""
              aria-hidden="true"
              decoding="async"
            />
            <img
              className="panel__corner panel__corner--tr"
              src="/assets/corner-a.svg"
              width="14"
              height="14"
              alt=""
              aria-hidden="true"
              decoding="async"
            />
            <img
              className="panel__corner panel__corner--bl"
              src="/assets/corner-b.svg"
              width="14"
              height="14"
              alt=""
              aria-hidden="true"
              decoding="async"
            />
            <img
              className="panel__corner panel__corner--br"
              src="/assets/corner-a.svg"
              width="14"
              height="14"
              alt=""
              aria-hidden="true"
              decoding="async"
            />

            {disabledBanner ? (
              disabledBanner
            ) : (
              <>
                <p className="panel__copy">{panelCopy}</p>

                <div className="panel__widget">{children}</div>
              </>
            )}
          </div>
        </section>
      </main>
    </>
  );
}
