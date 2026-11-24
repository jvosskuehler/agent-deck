// Ansicht `#/settings`: eigenes Fenster im Papier-Look. Kein Speichern-Knopf: jede
// Änderung geht sofort per set_setting an Rust, ein kurzes „Gespeichert“ bestätigt. Das
// Fenster wächst mit dem Inhalt (useDialogFit).
//
// Kompakt (Vorgabe, eine Spalte, 480 breit) steht nur, was man öfter anfasst: Neue Agenten
// (Modell, Modus, Effort) und Deck (samt Hell/Dunkel). „Erweitert“ klappt auf zwei Spalten
// (960) den Rest auf: Konto und Ordner, Auto-Modus neuer Agenten. Taugt der Repos-Ordner
// nicht, stehen Hinweis und Ordnerzeile auch kompakt da; Rust öffnet das Fenster dann beim
// Start selbst. Auf- oder zugeklappt merkt sich die Oberfläche.
//
// Abschnitte von Erweiterungen (`extensions.ts`) stehen in der rechten Spalte unter
// „Deck“, kompakt nur die ohne `advancedOnly`. Welche es gibt, sagt Rust (`extras`).

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { SettingsView as Settings } from "../../bindings/SettingsView";
import { commands, isNative } from "../../ipc";
import type { SettingKey } from "../../ipc/contract";
import { useSettings } from "../../ipc/hooks";
import { langFromSetting, t } from "../../logic/i18n";
import {
  appearanceOptions,
  edgeOptions,
  effortOptions,
  languageOptions,
  matchOption,
  modelOptions,
  modeOptions,
  onOffOptions,
} from "../../logic/settingsOptions";
import { settingsSection } from "../../extensions";
import { useDialogFit } from "../useDialogFit";
import { AgentEnvFields } from "./AgentEnvFields";
import { PathField, Segmented, SwitchRow } from "./controls";
import "../../styles/settings.css";

/** So lange steht „Gespeichert“ nach einer Änderung (ms). */
const SAVED_MS = 1600;

/** Fensterbreite kompakt bzw. erweitert (CSS-Pixel). */
const WIDTH_COMPACT = 480;
const WIDTH_WIDE = 960;
const EXPANDED_KEY = "agentDeck.settingsExpanded";

function readExpanded(): boolean {
  try {
    return window.localStorage.getItem(EXPANDED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeExpanded(on: boolean) {
  try {
    window.localStorage.setItem(EXPANDED_KEY, on ? "1" : "0");
  } catch {
    // Ohne Speicher gilt die Wahl nur, bis das Fenster neu lädt.
  }
}

const REPOS_NOTICE = {
  unset: "reposUnset",
  missing: "reposMissing",
  noRepos: "reposNoRepos",
} as const;

export function SettingsView() {
  const [settings, setSettings] = useSettings();
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(readExpanded);
  // Umschalten in zwei Schritten (18.11.2026, sonst sprang und flackerte das Fenster):
  // die neue Ansicht wird unsichtbar vorgemessen (`probe`), Rust bringt das Fenster in
  // einem Schritt auf ihre Größe, erst dann wird sie sichtbar.
  const [pending, setPending] = useState<boolean | null>(null);
  const savedTimer = useRef<number | null>(null);
  const root = useRef<HTMLDivElement | null>(null);
  const probe = useRef<HTMLDivElement | null>(null);
  useDialogFit(root, settings !== null, expanded ? WIDTH_WIDE : WIDTH_COMPACT);

  useLayoutEffect(() => {
    if (pending === null) return;
    const done = () => {
      writeExpanded(pending);
      setExpanded(pending);
      setPending(null);
    };
    const el = probe.current;
    if (!el || !isNative()) {
      done();
      return;
    }
    commands.resizeDialog(Math.ceil(el.getBoundingClientRect().height), pending ? WIDTH_WIDE : WIDTH_COMPACT).then(done, done);
  }, [pending]);

  useEffect(
    () => () => {
      if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    },
    [],
  );

  if (!settings) return <div className="settings" />;

  const lang = langFromSetting(settings.language);

  const confirmSaved = () => {
    setError(null);
    setSaved(true);
    if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => setSaved(false), SAVED_MS);
  };

  const save = <K extends SettingKey>(key: K, value: Settings[K]) => {
    // Sofort zeigen, was gewählt wurde; Rust antwortet mit dem gespeicherten Stand.
    setSettings({ ...settings, [key]: value });
    commands
      .setSetting(key, value)
      .then((next) => {
        setSettings(next);
        confirmSaved();
      })
      .catch((err: unknown) => {
        setSettings(settings);
        setError(String(err));
      });
  };

  const saveExtra = (key: string, value: unknown) => {
    commands
      .setExtraSetting(key, value)
      .then((next) => {
        setSettings(next);
        confirmSaved();
      })
      .catch((err: unknown) => setError(String(err)));
  };

  const toggleExpanded = () => {
    if (pending === null) setPending(!expanded);
  };

  const restartPanel = () => {
    commands.restartPanel().catch((err: unknown) => setError(String(err)));
  };

  const pickRepos = () => {
    commands
      .pickFolder(settings.reposRoot)
      .then((path) => {
        if (path) save("reposRoot", path);
      })
      .catch((err: unknown) => setError(String(err)));
  };

  const renderBody = (expanded: boolean, measuring: boolean) => {
    const onOff = onOffOptions(lang);
    // Was die Wahl über das Deck hinaus tut: fest gilt nur für Deck und Agenten, Sonnenstand
    // schaltet Windows selbst. „System“ braucht keinen Satz.
    const appearanceHint =
      settings.appearance === "sun"
        ? t(lang, "appearanceSunHint", { place: settings.sunPlaceName })
        : settings.appearance === "light" || settings.appearance === "dark"
          ? t(lang, "appearanceFixedHint")
          : undefined;
    const reposNotice = settings.reposState === "ok" ? null : REPOS_NOTICE[settings.reposState];

    const reposField = (
      <PathField
        label={t(lang, "reposRoot")}
        value={settings.reposRoot}
        empty={t(lang, "reposRootEmpty")}
        button={t(lang, "reposRootChange")}
        alert={reposNotice !== null}
        onPick={pickRepos}
      />
    );

    const agents = (
      <section className="set-section">
        <div className="set-section-head">
          <h2 className="set-h2">{t(lang, "newAgents")}</h2>
          <span className="set-hint">{t(lang, "newAgentsHint")}</span>
        </div>
        <Segmented
          label={t(lang, "model")}
          options={modelOptions(lang)}
          value={matchOption(modelOptions(lang), settings.model)}
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
        {expanded && (
          <SwitchRow
            label={t(lang, "autoPermissionMode")}
            hint={t(lang, "autoPermissionModeHint")}
            options={onOff}
            on={settings.autoPermissionMode}
            onChange={(v) => save("autoPermissionMode", v)}
          />
        )}
      </section>
    );

    const deck = (
      <section className="set-section">
        <div className="set-section-head">
          <h2 className="set-h2">{t(lang, "deck")}</h2>
          <span className="set-spacer" />
          <button type="button" className="tool-btn" onClick={commands.openKeys}>
            {t(lang, "shortcuts")}
          </button>
          <button type="button" className="tool-btn" onClick={commands.openGlow}>
            {t(lang, "openGlow")}
          </button>
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
          label={t(lang, "appearance")}
          options={appearanceOptions(lang)}
          value={matchOption(appearanceOptions(lang), settings.appearance)}
          hint={appearanceHint}
          onChange={(v) => save("appearance", v)}
        />
        <SwitchRow
          label={t(lang, "autoCompact")}
          hint={t(lang, "autoCompactHint")}
          options={onOff}
          on={settings.autoCompact}
          onChange={(v) => save("autoCompact", v)}
        />
      </section>
    );

    const account = (
      <section className="set-section">
        <div className="set-section-head">
          <h2 className="set-h2">{t(lang, "account")}</h2>
          <span className="set-hint">{t(lang, "accountHint")}</span>
        </div>
        <AgentEnvFields lang={lang} settings={settings} save={save} />
        {reposField}
      </section>
    );

    // Abschnitte der Erweiterungen, in der Reihenfolge, in der Rust sie meldet.
    const extras = settings.extras.flatMap((x) => {
      const entry = settingsSection(x.id);
      if (!entry || (entry.advancedOnly && !expanded)) return [];
      return [
        <div key={x.id}>
          <div className="set-divider" aria-hidden="true" />
          {entry.render({
            lang,
            values: x.values,
            expanded,
            save: saveExtra,
            onSaved: (next) => {
              setSettings(next);
              confirmSaved();
            },
            onError: setError,
          })}
        </div>,
      ];
    });

    const divider = <div className="set-divider" aria-hidden="true" />;

    const body = (
      <div className="settings" data-wide={expanded || undefined} ref={measuring ? probe : root}>
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

        {reposNotice && (
          <p className="set-notice" role="alert">
            {t(lang, reposNotice)}
          </p>
        )}

        {expanded ? (
          <div className="set-cols">
            <div className="set-col">
              {account}
              {divider}
              {agents}
            </div>
            <div className="set-col">
              {deck}
              {extras}
            </div>
          </div>
        ) : (
          <>
            {reposNotice && reposField}
            {agents}
            {divider}
            {deck}
            {extras}
          </>
        )}

        <footer className="set-footer">
          <button type="button" className="tool-btn set-more" aria-expanded={expanded} onClick={toggleExpanded}>
            {t(lang, expanded ? "less" : "more")}
          </button>
        </footer>
      </div>
    );
    return body;
  };

  const body = (
    <>
      {renderBody(expanded, false)}
      {pending !== null && (
        <div className="settings-probe" aria-hidden="true">
          {renderBody(pending, true)}
        </div>
      )}
    </>
  );

  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}
