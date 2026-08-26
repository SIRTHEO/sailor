#!/bin/sh
# Un motore che non torna mai: prova che `call_codex` ha una scadenza
# (difetto 3 del 25/08). `exec` sostituisce la shell col processo di sonno,
# così ucciderlo ammazza davvero il lavoro, non solo un genitore vuoto.
exec sleep 100000
