import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import "./GlassDropdown.css";

export interface GlassDropdownOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface GlassDropdownProps {
  value: string;
  options: GlassDropdownOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  buttonClassName?: string;
  menuClassName?: string;
}

const GlassDropdown: React.FC<GlassDropdownProps> = ({
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "Select",
  className,
  buttonClassName,
  menuClassName,
}) => {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const isOpen = open && !disabled;

  useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", onMouseDown);
    }

    return () => {
      document.removeEventListener("mousedown", onMouseDown);
    };
  }, [isOpen]);

  const resolvedOptions = useMemo(() => {
    if (!value) return options;
    if (options.some((option) => option.value === value)) return options;
    return [{ value, label: value }, ...options];
  }, [options, value]);

  const selectedLabel =
    resolvedOptions.find((option) => option.value === value)?.label ||
    (value ? value : "");

  return (
    <div
      className={`model-selector-container glass-dropdown-container ${className ?? ""}`.trim()}
      ref={containerRef}
    >
      <button
        type="button"
        className={`model-selector-btn glass-dropdown-btn ${open ? "active" : ""} ${buttonClassName ?? ""}`.trim()}
        onClick={() => {
          if (disabled) return;
          setOpen((prev) => !prev);
        }}
        disabled={disabled}
      >
        <span
          className={`model-name ${selectedLabel ? "" : "glass-dropdown-placeholder"}`.trim()}
          title={selectedLabel || placeholder}
        >
          {selectedLabel || placeholder}
        </span>
        <ChevronDown size={16} className="chevron" />
      </button>

      {isOpen && (
        <div className={`model-dropdown glass-dropdown-menu ${menuClassName ?? ""}`.trim()}>
          <div className="model-list">
            {resolvedOptions.map((option) => {
              const isSelected = option.value === value;
              return (
                <button
                  key={`${option.value}-${option.label}`}
                  type="button"
                  className={`model-item glass-dropdown-item ${isSelected ? "selected" : ""}`.trim()}
                  onClick={() => {
                    if (option.disabled) return;
                    onChange(option.value);
                    setOpen(false);
                  }}
                  disabled={option.disabled}
                  title={option.label}
                >
                  <span className="truncate">{option.label}</span>
                  {isSelected && <Check size={14} className="check-icon" />}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};

export default GlassDropdown;
