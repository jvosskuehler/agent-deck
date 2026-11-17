// Bausteine des Einstellungsfensters: Segmente, Schalterzeilen (An/Aus als kleine
// Segmente), Textfelder, eine Auswahlliste und die Ordnerzeile mit „Ändern…“.

import { useEffect, useState } from "react";
import type { Option } from "../../logic/settingsOptions";

interface SegmentedProps {
  label: string;
  options: Option[];
  value: string | null;
  onChange(value: string): void;
}

function Segments({ label, options, value, onChange }: SegmentedProps) {
  return (
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
  );
}

interface SegmentedFieldProps extends SegmentedProps {
  /** Kleine Zeile darunter: was die Wahl bewirkt. */
  hint?: string;
}

export function Segmented({ hint, ...props }: SegmentedFieldProps) {
  return (
    <div className="set-field">
      <span className="set-label">{props.label}</span>
      <Segments {...props} />
      {hint && <span className="set-row-hint">{hint}</span>}
    </div>
  );
}

interface SelectFieldProps {
  label: string;
  options: Option[];
  value: string;
  onChange(value: string): void;
}

/** Eine Auswahl aus einer langen Liste (zu viele für Segmente). */
export function SelectField({ label, options, value, onChange }: SelectFieldProps) {
  return (
    <label className="set-field">
      <span className="set-label">{label}</span>
      <select className="set-input set-select" value={value} onChange={(e) => onChange(e.target.value)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

interface SwitchRowProps {
  label: string;
  /** Kleine zweite Zeile: was der Schalter nach außen tut. */
  hint?: string;
  options: Option[];
  on: boolean;
  onChange(on: boolean): void;
}

/** Ein Schalter als Zeile: Text links, An/Aus rechts. Viele Schalter bleiben so kurz. */
export function SwitchRow({ label, hint, options, on, onChange }: SwitchRowProps) {
  return (
    <div className="set-row">
      <div className="set-row-text">
        <span className="set-row-label">{label}</span>
        {hint && <span className="set-row-hint">{hint}</span>}
      </div>
      <div className="set-row-seg">
        <Segments label={label} options={options} value={on ? "on" : "off"} onChange={(v) => onChange(v === "on")} />
      </div>
    </div>
  );
}

interface TextFieldProps {
  label: string;
  value: string;
  placeholder?: string;
  hint?: string;
  /** Gespeichert wird beim Verlassen des Felds oder mit Enter, nicht bei jedem Tastendruck. */
  onCommit(value: string): void;
}

export function TextField({ label, value, placeholder, hint, onCommit }: TextFieldProps) {
  const [draft, setDraft] = useState(value);
  // Neuer Stand aus Rust (gespeichert, normalisiert): ins Feld übernehmen.
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft.trim() !== value) onCommit(draft.trim());
  };
  return (
    <label className="set-field">
      <span className="set-label">{label}</span>
      <input
        className="set-input"
        value={draft}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setDraft(value);
        }}
      />
      {hint && <span className="set-row-hint">{hint}</span>}
    </label>
  );
}

interface PathFieldProps {
  label: string;
  value: string;
  empty: string;
  button: string;
  /** Hervorheben: der Ordner fehlt oder taugt nicht. */
  alert: boolean;
  onPick(): void;
}

/** Ein Ordner: Pfad zum Lesen, ändern nur über den Ordner-Dialog von Windows. */
export function PathField({ label, value, empty, button, alert, onPick }: PathFieldProps) {
  return (
    <div className="set-field" data-alert={alert || undefined}>
      <span className="set-label">{label}</span>
      <div className="set-path">
        <span className="set-path-value" data-empty={!value || undefined} title={value || undefined}>
          {value || empty}
        </span>
        <button type="button" className="tool-btn" onClick={onPick}>
          {button}
        </button>
      </div>
    </div>
  );
}
