// Ansicht `#/glow`: das Fenster „Glow einstellen“ (Strg+Alt+G). Zwei Spalten, hell und
// dunkel, je Glow-Art, Animation, zwei Farben, Regler und Farbvorlagen. Jede Änderung geht
// sofort an Rust (set_glow) und wirkt am Fokus-Rahmen des zuletzt gewählten Agenten; das
// Fenster nimmt ihm den Rahmen nicht weg. Die Vorschau hier ist nur eine Annäherung.
// Das Fenster ist so hoch wie sein Inhalt (useDialogFit) und wächst mit „Gemerkt“.

import { useRef, useState } from "react";
import type { GlowSettings } from "../../bindings/GlowSettings";
import type { GlowStyle } from "../../bindings/GlowStyle";
import type { SavedGlow } from "../../bindings/SavedGlow";
import { commands, isNative } from "../../ipc";
import { useGlow } from "../../ipc/hooks";
import {
  ANIMS,
  DARK_DEFAULT,
  DARK_PRESETS,
  KINDS,
  LIGHT_DEFAULT,
  LIGHT_PRESETS,
  previewShadow,
  remember,
  sameStyle,
  stamp,
  type Preset,
} from "../../logic/glow";
import { langFromView, t, type Lang } from "../../logic/i18n";
import { useDialogFit } from "../useDialogFit";
import { ColorField } from "./ColorField";
import "../../styles/settings.css";
import "../../styles/ticket.css";
import "../../styles/glow.css";

type Side = "light" | "dark";

export function GlowView() {
  const [view, setView] = useGlow();
  const [error, setError] = useState<string | null>(null);
  // Beim Ziehen kommen viele Werte: immer nur einer unterwegs, danach der neueste.
  const busy = useRef(false);
  const queued = useRef<GlowSettings | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useDialogFit(root, view != null);

  if (!view) return <div className="settings" />;
  const lang = langFromView(view.lang);

  const send = (glow: GlowSettings) => {
    if (busy.current) {
      queued.current = glow;
      return;
    }
    busy.current = true;
    commands
      .setGlow(glow)
      .then(() => setError(null))
      .catch((err: unknown) => setError(String(err)))
      .finally(() => {
        busy.current = false;
        const next = queued.current;
        queued.current = null;
        if (next) send(next);
      });
  };

  const patch = (part: Partial<GlowSettings>) => {
    const glow = { ...view.glow, ...part };
    setView({ ...view, glow });
    send(glow);
  };

  const body = (
    <div ref={root} className="settings glow">
      <header className="set-header">
        <h1 className="set-title">{t(lang, "glowTitle")}</h1>
        <span className="set-spacer" />
        {error ? (
          <span className="set-status set-status--error">{error}</span>
        ) : (
          <span className="set-status">{t(lang, "glowLive")}</span>
        )}
      </header>
      <div className="glow-columns">
        <Column
          lang={lang}
          side="light"
          active={!view.night}
          style={view.glow.light}
          presets={LIGHT_PRESETS}
          saved={view.glow.savedLight}
          onChange={(light) => patch({ light })}
          onSaved={(savedLight) => patch({ savedLight })}
        />
        <Column
          lang={lang}
          side="dark"
          active={view.night}
          style={view.glow.dark}
          presets={DARK_PRESETS}
          saved={view.glow.savedDark}
          onChange={(dark) => patch({ dark })}
          onSaved={(savedDark) => patch({ savedDark })}
        />
      </div>
    </div>
  );
  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}

interface ColumnProps {
  lang: Lang;
  side: Side;
  active: boolean;
  style: GlowStyle;
  presets: Preset[];
  saved: SavedGlow[];
  onChange(style: GlowStyle): void;
  onSaved(list: SavedGlow[]): void;
}

function Column({ lang, side, active, style, presets, saved, onChange, onSaved }: ColumnProps) {
  const [target, setTarget] = useState<"color" | "outer">("color");
  const set = <K extends keyof GlowStyle>(key: K, value: GlowStyle[K]) => onChange({ ...style, [key]: value });
  const night = side === "dark";
  const outerAuto = style.outer.trim() === "";

  return (
    <section className="glow-col" data-side={side} data-active={active || undefined}>
      <div className="set-section-head">
        <h2 className="set-h2">{t(lang, night ? "glowDark" : "glowLight")}</h2>
        {active && <span className="glow-badge">{t(lang, "glowActive")}</span>}
        <span className="set-spacer" />
        <button
          type="button"
          className="tool-btn"
          onClick={() =>
            onSaved(remember(saved, style, `${t(lang, `glowKind_${style.kind}`)} · ${t(lang, `glowAnim_${style.animation}`)} · ${stamp(new Date())}`))
          }
        >
          {t(lang, "glowSave")}
        </button>
        <button type="button" className="tool-btn" onClick={() => onChange({ ...(night ? DARK_DEFAULT : LIGHT_DEFAULT) })}>
          {t(lang, "glowReset")}
        </button>
      </div>

      <div className="glow-preview" data-side={side}>
        <div className="glow-preview-win" style={{ boxShadow: previewShadow(style) }} />
      </div>

      <Pick
        label={t(lang, "glowKind")}
        options={KINDS}
        value={style.kind}
        name={(k) => t(lang, `glowKind_${k}`)}
        onChange={(k) => set("kind", k)}
      />
      <Pick
        label={t(lang, "glowAnim")}
        options={ANIMS}
        value={style.animation}
        name={(a) => t(lang, `glowAnim_${a}`)}
        onChange={(a) => set("animation", a)}
      />

      <div className="seg" role="radiogroup">
        {(["color", "outer"] as const).map((k) => (
          <button
            key={k}
            type="button"
            role="radio"
            aria-checked={target === k}
            className="seg-item"
            data-on={target === k || undefined}
            onClick={() => setTarget(k)}
          >
            {k === "color" ? t(lang, "glowColor") : outerAuto ? t(lang, "glowOuterAuto") : t(lang, "glowOuter")}
          </button>
        ))}
      </div>
      {target === "color" ? (
        <ColorField value={style.color} onChange={(hex) => set("color", hex)} />
      ) : (
        <div className="glow-outer">
          <ColorField value={outerAuto ? style.color : style.outer} onChange={(hex) => set("outer", hex)} />
          <button type="button" className="tool-btn glow-auto" disabled={outerAuto} onClick={() => set("outer", "")}>
            {t(lang, "glowOuterReset")}
          </button>
        </div>
      )}

      <Slider label={t(lang, "glowIntensity")} value={style.intensity} min={0} max={2} step={0.05} onChange={(v) => set("intensity", v)} />
      <Slider label={t(lang, "glowWidth")} value={style.width} min={0.3} max={1.6} step={0.05} onChange={(v) => set("width", v)} />
      <Slider
        label={t(lang, "glowSpeed")}
        value={style.speed}
        min={0.25}
        max={2}
        step={0.05}
        disabled={style.animation === "still"}
        onChange={(v) => set("speed", v)}
      />

      <div className="set-field">
        <span className="set-label">{t(lang, "glowPresets")}</span>
        <div className="glow-presets">
          {presets.map((p) => (
            <button
              key={p.name}
              type="button"
              className="glow-preset"
              title={p.name}
              aria-label={p.name}
              data-on={(p.color === style.color && p.outer === style.outer) || undefined}
              style={{ background: `linear-gradient(135deg, ${p.color} 55%, ${p.outer || p.color} 55%)` }}
              onClick={() => onChange({ ...style, color: p.color, outer: p.outer })}
            />
          ))}
        </div>
      </div>

      <div className="set-field">
        <span className="set-label">{t(lang, "glowSaved")}</span>
        {saved.length === 0 ? (
          <span className="glow-saved-empty">{t(lang, "glowSavedEmpty")}</span>
        ) : (
          <ul className="glow-saved">
            {saved.map((s, i) => (
              <li key={i} className="glow-saved-row" data-on={sameStyle(s.style, style) || undefined}>
                <button
                  type="button"
                  className="glow-saved-swatch"
                  data-side={side}
                  title={t(lang, "glowSavedLoad")}
                  aria-label={`${t(lang, "glowSavedLoad")}: ${s.name}`}
                  onClick={() => onChange({ ...s.style })}
                >
                  <span style={{ boxShadow: previewShadow({ ...s.style, width: s.style.width * 0.45 }) }} />
                </button>
                <input
                  className="ticket-input glow-saved-name"
                  value={s.name}
                  spellCheck={false}
                  onChange={(e) => onSaved(saved.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))}
                />
                <button
                  type="button"
                  className="glow-saved-del"
                  title={t(lang, "glowSavedDelete")}
                  aria-label={`${t(lang, "glowSavedDelete")}: ${s.name}`}
                  onClick={() => onSaved(saved.filter((_, j) => j !== i))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

interface PickProps<T extends string> {
  label: string;
  options: T[];
  value: T;
  name(option: T): string;
  onChange(option: T): void;
}

/** Auswahl einer Glow-Art oder Animation: alle Möglichkeiten als Knöpfe, drei je Zeile. */
function Pick<T extends string>({ label, options, value, name, onChange }: PickProps<T>) {
  return (
    <div className="set-field">
      <span className="set-label">{label}</span>
      <div className="glow-pick" role="radiogroup" aria-label={label}>
        {options.map((o) => (
          <button
            key={o}
            type="button"
            role="radio"
            aria-checked={value === o}
            className="glow-pick-item"
            data-on={value === o || undefined}
            onClick={() => onChange(o)}
          >
            {name(o)}
          </button>
        ))}
      </div>
    </div>
  );
}

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit?: string;
  disabled?: boolean;
  onChange(v: number): void;
}

function Slider({ label, value, min, max, step, unit = "", disabled, onChange }: SliderProps) {
  return (
    <label className="glow-slider" data-disabled={disabled || undefined}>
      <span className="set-label">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="glow-num">
        {value.toFixed(step < 0.5 ? 2 : 1)}
        {unit}
      </span>
    </label>
  );
}
