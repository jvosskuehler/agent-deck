// Ansicht `#/comment`: das kleine Kommentar-Fenster (Entwurf N1, 17.11.2026). Rust zeigt
// es, wenn ein Mensch auf einem eigenen PR kommentiert hat und das noch nicht gemeldet
// ist (`DashView.popup`), und versteckt es, sobald `popup` leer ist. Das Fenster wird nie
// aktiv; Texteingabe gibt es darum keine, „Ändern“ öffnet den Entwurf im Editor.

import type { CommentPopup } from "../../bindings/CommentPopup";
import { commands, isNative } from "../../ipc";
import { useDash, useDeckView } from "../../ipc/hooks";
import { langFromView, type Lang } from "../../logic/i18n";
import { Avatar, CardButtons, DraftBlock, tr } from "../dash/FeedParts";
import "../../styles/comment.css";

export function CommentView() {
  const dash = useDash();
  const lang = langFromView(useDeckView()?.lang);
  const popup = dash?.popup ?? null;
  if (!popup) return <main className="ncomment" data-empty="" />;
  return <CommentCard popup={popup} lang={lang} />;
}

/**
 * Headless-Chrome lässt kein Fenster unter 500 px Breite zu; im Browser-Mock steht die
 * Karte darum in der echten Fenstergröße oben links, damit Bildschirmfotos stimmen.
 */
const MOCK_FRAME = { right: "auto", bottom: "auto", width: 420, height: 190 } as const;

function CommentCard({ popup, lang }: { popup: CommentPopup; lang: Lang }) {
  const { card } = popup;
  const close = () => commands.dashAction(`popupClose:${card.key}`);
  return (
    <main className="ncomment" data-draft={card.draft ? "" : undefined} style={isNative() ? undefined : MOCK_FRAME}>
      <div className="ncomment-head">
        <Avatar card={card} size={24} />
        <b className="ncomment-login">{card.login}</b>
        <span className="ncomment-on">
          {tr(lang, "auf", "on")}{" "}
          <button type="button" className="flabel" onClick={() => commands.openDashUrl(card.url)}>
            {card.label}
          </button>
        </span>
        <button type="button" className="ncomment-close" aria-label={tr(lang, "Schließen", "Close")} title={tr(lang, "Schließen (bleibt im Feed)", "Close (stays in the feed)")} onClick={close}>
          ×
        </button>
      </div>
      <div className="ncomment-text" title={popup.text}>
        „{popup.text}“
      </div>
      {card.agent && <div className="fcard-agent">◐ {card.agent}</div>}
      {card.draft ? (
        <DraftBlock cardKey={card.key} draft={card.draft} lang={lang} onAction={commands.dashAction} compact />
      ) : (
        <div className="ncomment-acts">
          <CardButtons card={card} lang={lang} onOpen={commands.openDashUrl} onAction={commands.dashAction} afterOpen={close} />
        </div>
      )}
    </main>
  );
}
