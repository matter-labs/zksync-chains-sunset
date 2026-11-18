import styles from "./ActionButton.module.css";
import { forwardRef } from "react";
import type { ButtonHTMLAttributes } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary";
  block?: boolean;
  animated?: boolean;
};

export const ActionButton = forwardRef<HTMLButtonElement, Props>(
  function ActionButton(
    {
      variant = "secondary",
      block = false,
      animated = false,
      className,
      type = "button",
      ...props
    },
    ref,
  ) {
    const classes = [
      styles.root,
      variant === "primary" ? styles.primary : styles.secondary,
      block ? styles.block : "",
      animated ? "widget__animated" : "",
      className ?? "",
    ]
      .filter(Boolean)
      .join(" ");

    return <button ref={ref} type={type} className={classes} {...props} />;
  },
);
