// Ansicht `#/keys`: das Fenster „Tastenkürzel“, nur zum Ansehen. Der Inhalt kommt aus
// SettingsView.shortcuts (deck_core::shortcuts), so stimmt er mit den Einstellungen überein.

import { isNative } from "../../ipc";
import { useSettings } from "../../ipc/hooks";
import { langFromSetting, t } from "../../logic/i18n";
import "../../styles/settings.css";

export function KeysView() {
  const [settings] = useSettings();
  if (!settings) return <div className="settings" />;
  const lang = langFromSetting(settings.language);
  const body = (
    <div className="settings">
      <header className="set-header">
        <h1 className="set-title">{t(lang, "shortcuts")}</h1>
        <span className="set-spacer" />
        <span className="set-status">{t(lang, "shortcutsHint")}</span>
      </header>
      <dl className="set-keys">
        {settings.shortcuts.map((s) => (
          <div key={s.keys} className="set-key-row">
            <dt>
              <kbd className="set-kbd">{s.keys}</kbd>
            </dt>
            <dd>{s.action}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}
