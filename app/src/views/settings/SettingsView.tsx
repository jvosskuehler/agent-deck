// Ansicht `#/settings`: eigenes Fenster im Papier-Look. Zwei Bereiche: Neue Agenten und
// Deck. Kein Speichern-Knopf: jede Änderung geht sofort per set_setting an Rust, ein
// kurzes „Gespeichert“ bestätigt. Was bewusst fehlt, steht in docs/DESIGN.md („Bewusst
// nicht“).

import { useEffect, useRef, useState } from "react";
import type { SettingsView as Settings } from "../../bindings/SettingsView";
import { commands, isNative } from "../../ipc";
import type { SettingKey } from "../../ipc/contract";
import { useSettings } from "../../ipc/hooks";
import { langFromSetting, t } from "../../logic/i18n";
import {
  edgeOptions,
  effortOptions,
  languageOptions,
  matchOption,
  modelOptions,
  modeOptions,
  onOffOptions,
} from "../../logic/settingsOptions";
import { Segmented } from "./controls";
import "../../styles/settings.css";

/** So lange steht „Gespeichert“ nach einer Änderung (ms). */
const SAVED_MS = 1600;

export function SettingsView() {
  const [settings, setSettings] = useSettings();
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const savedTimer = useRef<number | null>(null);

  useEffect(() => () => {
    if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
  }, []);

  if (!settings) return <div className="settings" />;

  const lang = langFromSetting(settings.language);

  const save = <K extends SettingKey>(key: K, value: Settings[K]) => {
    // Sofort zeigen, was gewählt wurde; Rust antwortet mit dem gespeicherten Stand.
    setSettings({ ...settings, [key]: value });
    commands
      .setSetting(key, value)
      .then((next) => {
        setSettings(next);
        setError(null);
        setSaved(true);
        if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
        savedTimer.current = window.setTimeout(() => setSaved(false), SAVED_MS);
      })
      .catch((err: unknown) => {
        setSettings(settings);
        setError(String(err));
      });
  };

  const restartPanel = () => {
    commands.restartPanel().catch((err: unknown) => setError(String(err)));
  };

  const body = (
    <div className="settings">
      <header className="set-header">
        <h1 className="set-title">{t(lang, "settings")}</h1>
        <span className="set-spacer" />
        {error ? (
          <span className="set-status set-status--error">{error}</span>
        ) : saved ? (
          <span className="set-status set-status--saved">{t(lang, "savedNote")}</span>
        ) : (
          <span className="set-status">{t(lang, "liveNote")}</span>
        )}
      </header>

      <section className="set-section">
        <div className="set-section-head">
          <h2 className="set-h2">{t(lang, "newAgents")}</h2>
          <span className="set-hint">{t(lang, "newAgentsHint")}</span>
        </div>
        <Segmented
          label={t(lang, "model")}
          options={modelOptions()}
          value={matchOption(modelOptions(), settings.model)}
          onChange={(v) => save("model", v)}
        />
        <Segmented
          label={t(lang, "mode")}
          options={modeOptions(lang)}
          value={matchOption(modeOptions(lang), settings.mode)}
          onChange={(v) => save("mode", v)}
        />
        <Segmented
          label={t(lang, "effort")}
          options={effortOptions()}
          value={matchOption(effortOptions(), settings.effort)}
          onChange={(v) => save("effort", v)}
        />
      </section>

      <div className="set-divider" aria-hidden="true" />

      <section className="set-section">
        <div className="set-section-head">
          <h2 className="set-h2">{t(lang, "deck")}</h2>
          <span className="set-spacer" />
          <button type="button" className="tool-btn" onClick={restartPanel}>
            {t(lang, "restartPanel")}
          </button>
        </div>
        <Segmented
          label={t(lang, "language")}
          options={languageOptions()}
          value={matchOption(languageOptions(), settings.language)}
          onChange={(v) => save("language", v)}
        />
        <Segmented
          label={t(lang, "dockEdge")}
          options={edgeOptions(lang)}
          value={matchOption(edgeOptions(lang), settings.dockEdge)}
          onChange={(v) => save("dockEdge", v)}
        />
        <Segmented
          label={t(lang, "autoCompact")}
          options={onOffOptions(lang)}
          value={settings.autoCompact ? "on" : "off"}
          onChange={(v) => save("autoCompact", v === "on")}
        />
      </section>
    </div>
  );

  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}
