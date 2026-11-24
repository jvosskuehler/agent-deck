// Wo neue Agenten laufen (Windows oder WSL) und in welcher Distro. Ein Wechsel gilt erst
// nach „Panel neu starten“: Claudes Ordner, Usage und Zusammenfassung hängen daran
// (`app/src-tauri/src/agent_env.rs`).

import type { SettingsView as Settings } from "../../bindings/SettingsView";
import type { Lang } from "../../logic/i18n";
import { t } from "../../logic/i18n";
import { agentEnvOptions, matchOption } from "../../logic/settingsOptions";
import { Segmented, SelectField } from "./controls";

interface Props {
  lang: Lang;
  settings: Settings;
  save(key: "agentEnv" | "wslDistro", value: string): void;
}

export function AgentEnvFields({ lang, settings, save }: Props) {
  const known = settings.wslDistros.includes(settings.wslDistro);
  // Eine eingestellte, aber nicht (mehr) gefundene Distro bleibt sichtbar, statt still
  // die erste der Liste zu zeigen.
  const distros = [
    ...(known ? [] : [{ value: settings.wslDistro, label: settings.wslDistro || "…" }]),
    ...settings.wslDistros.map((d) => ({ value: d, label: d })),
  ];
  return (
    <>
      <Segmented
        label={t(lang, "agentEnv")}
        options={agentEnvOptions()}
        value={matchOption(agentEnvOptions(), settings.agentEnv)}
        hint={t(lang, "agentEnvHint")}
        onChange={(v) => save("agentEnv", v)}
      />
      {settings.agentEnv === "wsl" &&
        (settings.wslDistros.length > 0 ? (
          <SelectField label={t(lang, "wslDistro")} options={distros} value={settings.wslDistro} onChange={(v) => save("wslDistro", v)} />
        ) : (
          <p className="set-row-hint">{t(lang, "wslNone")}</p>
        ))}
    </>
  );
}
