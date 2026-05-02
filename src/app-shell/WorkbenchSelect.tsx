import { useEffect, useId, useMemo, useRef, useState } from "react";

export interface WorkbenchSelectOption {
  value: string;
  label: string;
}

export function WorkbenchSelect({
  ariaLabel,
  value,
  options,
  onChange,
  align = "start",
}: {
  ariaLabel: string;
  value: string;
  options: WorkbenchSelectOption[];
  onChange: (value: string) => void;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const listboxId = useId();

  const selectedIndex = useMemo(
    () => Math.max(0, options.findIndex((option) => option.value === value)),
    [options, value],
  );
  const selectedOption = options[selectedIndex] ?? options[0] ?? { value, label: value };

  useEffect(() => {
    if (!open) {
      return;
    }

    setActiveIndex(selectedIndex);

    const onPointerDown = (event: PointerEvent) => {
      if (rootRef.current?.contains(event.target as Node)) {
        return;
      }
      setOpen(false);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    };

    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open, selectedIndex]);

  useEffect(() => {
    if (!open) {
      return;
    }
    optionRefs.current[activeIndex]?.focus();
  }, [activeIndex, open]);

  const moveActive = (direction: 1 | -1) => {
    if (options.length === 0) {
      return;
    }
    setActiveIndex((current) => (current + direction + options.length) % options.length);
  };

  const commitSelection = (index: number) => {
    const next = options[index];
    if (!next) {
      return;
    }
    onChange(next.value);
    setOpen(false);
    buttonRef.current?.focus();
  };

  return (
    <div
      ref={rootRef}
      className={`workbench-select-shell ${open ? "open" : ""} ${align === "end" ? "align-end" : "align-start"}`}
      onBlur={(event) => {
        const nextTarget = event.relatedTarget;
        if (nextTarget instanceof Node && rootRef.current?.contains(nextTarget)) {
          return;
        }
        setOpen(false);
      }}
    >
      <button
        ref={buttonRef}
        className="workbench-select-button"
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-controls={open ? listboxId : undefined}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            setActiveIndex(selectedIndex);
            return;
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
            setActiveIndex(selectedIndex);
          }
        }}
      >
        <span className="workbench-select-value" title={selectedOption.label}>
          {selectedOption.label}
        </span>
        <span className="workbench-select-caret-shell" aria-hidden="true">
          <span className="workbench-select-caret">⌄</span>
        </span>
      </button>

      {open ? (
        <div id={listboxId} className="workbench-select-menu" role="listbox" aria-label={ariaLabel}>
          {options.map((option, index) => {
            const selected = option.value === value;
            const active = index === activeIndex;
            return (
              <button
                key={option.value}
                ref={(element) => {
                  optionRefs.current[index] = element;
                }}
                className={`workbench-select-option ${selected ? "selected" : ""} ${active ? "active" : ""}`}
                type="button"
                role="option"
                aria-selected={selected}
                onMouseDown={(event) => {
                  if (event.button !== 0) {
                    return;
                  }
                  event.preventDefault();
                  commitSelection(index);
                }}
                onMouseEnter={() => setActiveIndex(index)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    moveActive(1);
                    return;
                  }
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    moveActive(-1);
                    return;
                  }
                  if (event.key === "Home") {
                    event.preventDefault();
                    setActiveIndex(0);
                    return;
                  }
                  if (event.key === "End") {
                    event.preventDefault();
                    setActiveIndex(options.length - 1);
                    return;
                  }
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    commitSelection(index);
                    return;
                  }
                  if (event.key === "Escape") {
                    event.preventDefault();
                    setOpen(false);
                    buttonRef.current?.focus();
                  }
                }}
              >
                <span>{option.label}</span>
                {selected ? <span className="workbench-select-check">●</span> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
