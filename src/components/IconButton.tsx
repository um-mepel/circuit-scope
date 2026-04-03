import type { ButtonHTMLAttributes, ReactNode } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** Accessible name and tooltip */
  label: string;
  children: ReactNode;
  /** Use "toolbar" (default) or "large" for welcome hero actions */
  variant?: "toolbar" | "large";
};

export function IconButton({
  label,
  children,
  variant = "toolbar",
  className,
  type = "button",
  ...rest
}: Props) {
  const cls =
    variant === "large"
      ? `cs-icon-btn-large${className ? ` ${className}` : ""}`
      : `cs-icon-btn${className ? ` ${className}` : ""}`;
  return (
    <button type={type} className={cls} aria-label={label} title={label} {...rest}>
      {children}
    </button>
  );
}
