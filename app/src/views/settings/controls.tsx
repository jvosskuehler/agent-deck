// Baustein des Einstellungsfensters: die Segmente.

import type { Option } from "../../logic/settingsOptions";

interface SegmentedProps {
  label: string;
  options: Option[];
  value: string | null;
  onChange(value: string): void;
}

export function Segmented({ label, options, value, onChange }: SegmentedProps) {
  return (
    <div className="set-field">
      <span className="set-label">{label}</span>
      <div className="seg" role="radiogroup" aria-label={label}>
        {options.map((o) => {
          const on = o.value === value;
          return (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={on}
              className="seg-item"
              data-on={on || undefined}
              onClick={() => !on && onChange(o.value)}
            >
              {o.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
