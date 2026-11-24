# Startet einen Claude-Agenten in WSL, im Windows-Terminal-Fenster, das das Agent Deck
# geoeffnet hat. Gegenstueck zu wt_agent.ps1 fuer Agenten in WSL.
#
# Aufruf (deck-wsl.exe, deck_core::wsl::wsl_argv):
#   wsl.exe -d <Distro> --cd <Ordner> -e bash -li <dieses Skript> <Argumente fuer claude>
# `bash -li` laedt .profile und .bashrc wie ein normales Terminal: so findet sich claude
# auch unter ~/.local/bin oder ueber nvm. AGENT_SLOT, die Herkunftsmarke und der
# Zustandsordner kommen ueber WSLENV von deck-wsl.exe.

# 1) Saubere Umgebung (wie wt_agent.ps1): CLAUDE_CODE_CHILD_SESSION schaltet das
#    Transcript ab (dann findet --resume nichts), NO_COLOR nimmt alle Farben.
unset CLAUDECODE CLAUDE_CODE_CHILD_SESSION CLAUDE_CODE_SESSION_ID CLAUDE_CODE_SESSION_ATTENDED \
      CLAUDE_CODE_ENTRYPOINT CLAUDE_CODE_EXECPATH CLAUDE_CODE_MESSAGING_SOCKET \
      CLAUDE_CODE_MESSAGING_TOKEN NO_COLOR
export COLORTERM=truecolor

# 2) Claudes Ordner fuer die Hooks: deck-statusline liest dort die Sprache. `/p` laesst WSL
#    den Pfad auf dem Weg zu den Windows-Programmen in \\wsl.localhost\... umrechnen.
claude_dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
export AGENT_DECK_CLAUDE_DIR="$claude_dir"
case ":$WSLENV:" in
  *:AGENT_DECK_CLAUDE_DIR/p:*) ;;
  *) export WSLENV="${WSLENV:+$WSLENV:}AGENT_DECK_CLAUDE_DIR/p" ;;
esac

# 3) claude mit den Argumenten des Decks. Die Einstellungen nur fuer die Agenten des Decks
#    (Theme passend zum Profil "Agent Deck") legt der Installer an (deck-setup look --wsl).
if [ -f "$claude_dir/agent-deck-wt.settings.json" ]; then
  set -- --settings "$claude_dir/agent-deck-wt.settings.json" "$@"
fi
if command -v claude >/dev/null 2>&1; then
  claude "$@"
else
  echo "Claude Code ist in dieser WSL-Distro nicht installiert."
  echo "Installieren:  curl -fsSL https://claude.ai/install.sh | bash"
fi

# 4) Die Shell bleibt stehen wie bei -NoExit: das Fenster laesst sich weiter benutzen.
exec "${SHELL:-/bin/bash}" -l
