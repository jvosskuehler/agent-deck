// Ansicht `#/settings`: eigenes Fenster im Papier-Look. Kein Speichern-Knopf: jede
// Änderung geht sofort per set_setting an Rust, ein kurzes „Gespeichert“ bestätigt. Das
// Fenster wächst mit dem Inhalt (useDialogFit).
//
// Kompakt (Vorgabe, eine Spalte, 480 breit) steht nur, was man öfter anfasst: Neue Agenten
// (Modell, Modus, Effort) und Deck. „Erweitert“ klappt auf zwei Spalten (960) den Rest auf:
// Konto und Ordner, Auto-Modus neuer Agenten, Automatik samt Reviewern. Taugt der
// Repos-Ordner nicht, stehen Hinweis und Ordnerzeile auch kompakt da; Rust öffnet das
// Fenster dann beim Start selbst. Auf- oder zugeklappt merkt sich die Oberfläche.

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
import { useDialogFit } from "../useDialogFit";
import { PathField, Segmented, SwitchRow, TextField } from "./controls";
import { ReviewerField } from "./ReviewerField";
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
  const savedTimer = useRef<number | null>(null);
  const root = useRef<HTMLDivElement | null>(null);
  useDialogFit(root, settings !== null, expanded ? WIDTH_WIDE : WIDTH_COMPACT);

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

  const toggleExpanded = () => {
    writeExpanded(!expanded);
    setExpanded(!expanded);
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

  const onOff = onOffOptions(lang);
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
      <SwitchRow
        label={t(lang, "dashboard")}
        options={onOff}
        on={settings.dashboard}
        onChange={(v) => save("dashboard", v)}
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
      {reposField}
      <TextField
        label={t(lang, "jiraPrefix")}
        value={settings.jiraPrefix}
        placeholder="ABC"
        onCommit={(v) => save("jiraPrefix", v)}
      />
    </section>
  );

  const automation = (
    <section className="set-section">
      <div className="set-section-head">
        <h2 className="set-h2">{t(lang, "automation")}</h2>
        <span className="set-hint">{t(lang, "automationHint")}</span>
      </div>
      <SwitchRow
        label={t(lang, "storeUpdate")}
        hint={t(lang, "storeUpdateHint")}
        options={onOff}
        on={settings.storeUpdate}
        onChange={(v) => save("storeUpdate", v)}
      />
      <SwitchRow
        label={t(lang, "speechmikeVoice")}
        hint={t(lang, "speechmikeVoiceHint")}
        options={onOff}
        on={settings.speechmikeVoice}
        onChange={(v) => save("speechmikeVoice", v)}
      />
      <SwitchRow
        label={t(lang, "titan")}
        hint={t(lang, "titanHint")}
        options={onOff}
        on={settings.titan}
        onChange={(v) => save("titan", v)}
      />
      <SwitchRow
        label={t(lang, "prAutomation")}
        hint={t(lang, "prAutomationHint")}
        options={onOff}
        on={settings.prAutomation}
        onChange={(v) => save("prAutomation", v)}
      />
      {/* Immer sichtbar: wer die eigenen PRs reviewt, ist eine Frage an jeden, auch
          bevor die Automatik an ist (der Installer fragt es ebenfalls). */}
      <ReviewerField
        lang={lang}
        settings={settings}
        onSaved={(next) => {
          setSettings(next);
          confirmSaved();
        }}
        onError={setError}
      />
      {settings.prAutomation && (
        <TextField
          label={t(lang, "githubOrg")}
          value={settings.githubOrg}
          onCommit={(v) => save("githubOrg", v)}
        />
      )}
    </section>
  );

  const divider = <div className="set-divider" aria-hidden="true" />;

  const body = (
    <div className="settings" data-wide={expanded || undefined} ref={root}>
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
            {divider}
            {automation}
          </div>
        </div>
      ) : (
        <>
          {reposNotice && reposField}
          {agents}
          {divider}
          {deck}
        </>
      )}

      <footer className="set-footer">
        <button type="button" className="tool-btn set-more" aria-expanded={expanded} onClick={toggleExpanded}>
          {t(lang, expanded ? "less" : "more")}
        </button>
      </footer>
    </div>
  );

  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}
