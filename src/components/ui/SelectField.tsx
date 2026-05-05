import { useEffect, useMemo, useRef, useState } from "preact/hooks";

export interface SelectFieldOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface SelectFieldProps {
  id?: string;
  name?: string;
  value: string;
  options: SelectFieldOption[];
  disabled?: boolean;
  class?: string;
  onValueChange: (value: string) => void;
}

export function SelectField({ id, name, value, options, disabled = false, class: className, onValueChange }: SelectFieldProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const listboxId = useMemo(() => `${id ?? name ?? "select-field"}-listbox`, [id, name]);

  const enabledOptions = useMemo(
    () => options.map((option, index) => ({ option, index })).filter(({ option }) => !option.disabled),
    [options],
  );
  const selectedIndex = useMemo(() => options.findIndex((option) => option.value === value), [options, value]);
  const selectedOption = selectedIndex >= 0 ? options[selectedIndex] : null;

  const closeMenu = () => {
    setIsOpen(false);
    setHighlightedIndex(-1);
  };

  const openMenu = () => {
    if (disabled || enabledOptions.length === 0) {
      return;
    }
    const selectedEnabled = enabledOptions.find(({ index }) => index === selectedIndex);
    setHighlightedIndex(selectedEnabled ? selectedEnabled.index : enabledOptions[0].index);
    setIsOpen(true);
  };

  const commitIndex = (index: number) => {
    const next = options[index];
    if (!next || next.disabled) {
      return;
    }
    onValueChange(next.value);
    closeMenu();
  };

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target || !rootRef.current || rootRef.current.contains(target)) {
        return;
      }
      closeMenu();
    };

    const onEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeMenu();
      }
    };

    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onEscape);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onEscape);
    };
  }, [isOpen]);

  const moveHighlight = (direction: 1 | -1) => {
    if (enabledOptions.length === 0) {
      return;
    }
    if (highlightedIndex < 0) {
      setHighlightedIndex(enabledOptions[0].index);
      return;
    }
    const currentEnabledIndex = enabledOptions.findIndex(({ index }) => index === highlightedIndex);
    const nextEnabledIndex =
      currentEnabledIndex < 0 ? 0 : (currentEnabledIndex + direction + enabledOptions.length) % enabledOptions.length;
    setHighlightedIndex(enabledOptions[nextEnabledIndex].index);
  };

  return (
    <div
      ref={rootRef}
      class={`select-field${className ? ` ${className}` : ""}`}
      onKeyDown={(event) => {
        if (disabled) {
          return;
        }

        if (event.key === "ArrowDown") {
          event.preventDefault();
          if (!isOpen) {
            openMenu();
          } else {
            moveHighlight(1);
          }
          return;
        }

        if (event.key === "ArrowUp") {
          event.preventDefault();
          if (!isOpen) {
            openMenu();
          } else {
            moveHighlight(-1);
          }
          return;
        }

        if (event.key === "Home" && isOpen && enabledOptions.length > 0) {
          event.preventDefault();
          setHighlightedIndex(enabledOptions[0].index);
          return;
        }

        if (event.key === "End" && isOpen && enabledOptions.length > 0) {
          event.preventDefault();
          setHighlightedIndex(enabledOptions[enabledOptions.length - 1].index);
          return;
        }

        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          if (!isOpen) {
            openMenu();
          } else if (highlightedIndex >= 0) {
            commitIndex(highlightedIndex);
          }
        }
      }}
    >
      <button
        id={id}
        type="button"
        class="select-field-control"
        disabled={disabled}
        name={name}
        aria-haspopup="listbox"
        aria-expanded={isOpen ? "true" : "false"}
        aria-controls={listboxId}
        onClick={() => {
          if (isOpen) {
            closeMenu();
          } else {
            openMenu();
          }
        }}
      >
        <span class={`select-field-label${selectedOption ? "" : " select-field-placeholder"}`}>
          {selectedOption ? selectedOption.label : "Select option"}
        </span>
      </button>

      {isOpen ? (
        <div class="select-field-menu" role="listbox" id={listboxId}>
          {options.map((option, index) => {
            const isSelected = option.value === value;
            const isHighlighted = index === highlightedIndex;

            return (
              <button
                key={`${option.value}-${index}`}
                type="button"
                class={`select-field-option${isSelected ? " is-selected" : ""}${isHighlighted ? " is-highlighted" : ""}`}
                role="option"
                aria-selected={isSelected ? "true" : "false"}
                disabled={option.disabled}
                onMouseEnter={() => {
                  if (!option.disabled) {
                    setHighlightedIndex(index);
                  }
                }}
                onClick={() => commitIndex(index)}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
