import type { ComponentChildren } from "preact";

interface AccountHomeCardButtonProps {
  onClick: () => void;
  ariaLabel: string;
  title?: string;
  className?: string;
  children: ComponentChildren;
}

export function AccountHomeCardButton({
  onClick,
  ariaLabel,
  title,
  className = "",
  children,
}: AccountHomeCardButtonProps) {
  const normalizedClassName = className.trim();
  const classes = normalizedClassName
    ? `account-item account-home-card account-home-button ${normalizedClassName}`
    : "account-item account-home-card account-home-button";

  return (
    <div
      class={classes}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.currentTarget !== event.target) {
          return;
        }
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onClick();
        }
      }}
      aria-label={ariaLabel}
      title={title}
      role="button"
      tabIndex={0}
    >
      {children}
    </div>
  );
}
