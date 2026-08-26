#!/bin/sh
# Finto motore OpenRouter per le prove di integrazione: ignora modello e
# prompt, stampa sempre lo stesso corpo di successo. Il nome porta `.test.`
# apposta, perché il gate `legacy-script` esenta le batterie da quel segno.
cat <<'JSON'
{"choices":[{"message":{"content":"answer: 42"}}],"usage":{"total_tokens":123}}
JSON
