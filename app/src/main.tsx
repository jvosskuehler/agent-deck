// Einstieg der Team-Ausgabe: alle Fenster laden dieselbe Seite und wählen ihre Ansicht
// über den Hash (`start.tsx`, docs/IPC.md). Eine andere Ausgabe hat ihren eigenen
// Einstieg, registriert ihre Ansichten (`extensions.ts`) und ruft dann `startDeck()`.

import { startDeck } from "./start";

startDeck();
