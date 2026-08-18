#!/usr/bin/env python3
"""L'oracolo dei confronti, conservato su file per quando il Python non ci sara'.

PERCHE' ESISTE (decisione di Theo, 18/08/2026). I ganci sono passati tutti al
binario Rust e i 25 originali in Python non li esegue piu' nessuno — ma sono
l'**oracolo** dei 29 confronti d'equivalenza, che senza di loro non hanno piu'
niente contro cui misurare. Cancellare gli originali cancellerebbe la rete che
oggi trova i difetti veri: solo il 18/08 ne ha trovati tre che rileggendo il
codice non si vedevano.

LA VIA D'USCITA. Le risposte dell'oracolo si registrano **adesso**, finche' il
Python c'e', e da domani il porto si confronta con quelle. Non e' la stessa cosa
— un esito registrato non scopre nulla di nuovo, congela cio' che si sapeva il
giorno in cui e' stato scritto — e per questo il registro porta con se' la data e
l'impronta dell'originale da cui viene: quando il Rust cambiera' comportamento
di proposito, si vedra' contro cosa lo si sta misurando.

LE TRE MODALITA', in ordine di quanto sanno:

    con l'originale sul disco   si interroga il Python, come sempre; se esiste
                                anche il registro si controlla che dica lo stesso,
                                cosi' il registro non marcisce in silenzio
    con `--record`              si interroga il Python e si **riscrive** il
                                registro: e' l'unico modo di crearlo o rinfrescarlo
    senza l'originale           si legge il registro, e un caso che non c'e'
                                dentro e' un errore rumoroso, non un verde

LA TRAPPOLA DEL JSON, che qui e' il difetto piu' probabile: una tupla salvata
torna indietro come lista, e `('go', '') != ['go', '']` renderebbe rosso ogni
caso il giorno in cui si passa al registro. Tutte le risposte si normalizzano
prima di essere confrontate, in entrambe le direzioni.

    python3 <un confronto>.py --record   registra le risposte dell'oracolo
    python3 tools/oracle.py --test       le prove di questo modulo
"""
from __future__ import annotations

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

STORE = Path(__file__).resolve().parent / 'oracle'


def normalise(value):
    """La forma che il valore avra' dopo un giro attraverso JSON.

    Serve da entrambi i lati: la risposta appena calcolata e quella riletta dal
    file devono essere confrontabili senza che una tupla o una chiave numerica
    facciano la differenza.
    """
    return json.loads(json.dumps(value, default=str, sort_keys=True))


def scrubbed(value, patterns):
    """Il valore con i percorsi volatili sostituiti da un segnaposto.

    SENZA QUESTO IL REGISTRO NON VALE NIENTE, e il difetto sarebbe silenzioso al
    contrario: quasi tutti questi confronti fabbricano una `HOME` finta sotto
    `/var/folders/...` che cambia a ogni esecuzione, e quel percorso finisce sia
    nella chiave del caso sia dentro le risposte (i messaggi dei ganci citano i
    file su cui decidono). Registrato cosi', il registro sarebbe buono per una
    sola esecuzione: la successiva non riconoscerebbe nessun caso e direbbe
    «impossibile giudicarli» su tutti.
    """
    if not patterns:
        # NORMALIZZA COMUNQUE. Uscire di qui col valore grezzo rimetterebbe in
        # gioco la trappola delle tuple proprio nel caso piu' comune, quello
        # senza percorsi da ripulire: e' il difetto che le prove hanno preso.
        return normalise(value)
    blob = json.dumps(normalise(value), sort_keys=True, ensure_ascii=False)
    for i, pattern in enumerate(patterns):
        if pattern:
            blob = blob.replace(str(pattern).replace('\\', '\\\\'), f'<volatile-{i}>')
    return json.loads(blob)


def case_key(case) -> str:
    """Un nome stabile per il caso, che non dipenda da come e' fatto dentro.

    L'impronta e non il testo: i casi di questi confronti sono comandi interi,
    payload da migliaia di caratteri, alberi di file. Un dizionario con quelle
    chiavi sarebbe illeggibile e pesante quanto il corpus.
    """
    blob = json.dumps(normalise(case), sort_keys=True, ensure_ascii=False)
    return hashlib.sha1(blob.encode('utf-8')).hexdigest()[:16]


def fingerprint(path: Path) -> str:
    try:
        return hashlib.sha1(path.read_bytes()).hexdigest()[:12]
    except OSError:
        return ''


class Oracle:
    """Le risposte attese di un confronto, con o senza l'originale sul disco.

    `name` da' il nome al file (`tools/oracle/<name>.json`); `original` e' il
    Python di cui si sta registrando il comportamento, e serve a due cose: sapere
    se si puo' ancora interrogare, e lasciare la sua impronta nel registro.
    """

    def __init__(self, name: str, original: Path | None = None,
                 argv: list[str] | None = None, scrub: list | None = None):
        self.name = name
        # I percorsi volatili da togliere di mezzo, tipicamente la cartella
        # temporanea del confronto. Vanno passati anche al valore del porto, con
        # `oracle.clean(...)`, altrimenti i due lati non si somigliano piu'.
        self.patterns = list(scrub or [])
        self.original = Path(original) if original else None
        argv = sys.argv if argv is None else argv
        self.recording = '--record' in argv
        self.path = STORE / f'{name}.json'
        self.data = self._read()
        self.fresh = {}
        self.stale = []          # i casi in cui il registro non dice piu' il vero
        self.missing = []        # i casi che il registro non conosce

    def _read(self) -> dict:
        try:
            return json.loads(self.path.read_text())
        except (OSError, ValueError):
            return {'cases': {}}

    @property
    def has_original(self) -> bool:
        return bool(self.original and self.original.exists())

    def clean(self, value):
        """Da applicare anche alla risposta del porto: i due lati devono essere
        ripuliti allo stesso modo, o il confronto diventa un confronto fra un
        percorso vero e un segnaposto."""
        return scrubbed(value, self.patterns)

    def answer(self, case, compute):
        """La risposta attesa per questo caso.

        `case` identifica il caso — un comando, un payload, una tupla di
        argomenti — e `compute` e' la chiamata che interroga l'originale. Non si
        chiama mai quando l'originale non c'e': e' proprio il punto.
        """
        key = case_key(scrubbed(case, self.patterns))
        stored = self.data.get('cases', {}).get(key)

        if self.recording or self.has_original:
            value = scrubbed(compute(), self.patterns)
            if self.recording:
                self.fresh[key] = value
            elif stored is not None and stored != value:
                self.stale.append((key, stored, value))
            return value

        if stored is None:
            self.missing.append(key)
            return None
        return stored

    def close(self) -> int:
        """Chiude il giro: salva se si stava registrando, e dice cosa non torna.

        Il codice di uscita non e' il verdetto del confronto — quello lo da' chi
        chiama — ma un registro che non conosce dei casi, o che dice il falso, e'
        un guasto suo e va fuori con un numero diverso da zero.
        """
        if self.recording:
            STORE.mkdir(parents=True, exist_ok=True)
            payload = {
                'oracle': self.name,
                'recorded_at': datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
                'original': str(self.original) if self.original else '',
                'original_sha1': fingerprint(self.original) if self.original else '',
                'cases': self.fresh,
            }
            self.path.write_text(json.dumps(payload, indent=1, ensure_ascii=False, sort_keys=True) + '\n')
            print(f'registrate {len(self.fresh)} risposte in {self.path.relative_to(STORE.parent)}')
            return 0

        problems = 0
        if self.stale:
            print(f'\nIL REGISTRO E\' INVECCHIATO: {len(self.stale)} casi in cui '
                  f'l\'originale non risponde piu\' come registrato')
            for key, stored, value in self.stale[:3]:
                print(f'  caso {key}\n    registrato: {str(stored)[:120]}'
                      f'\n    adesso:     {str(value)[:120]}')
            print(f'  rinfresca con: python3 <questo confronto> --record')
            problems += 1
        if self.missing:
            print(f'\nIL REGISTRO NON CONOSCE {len(self.missing)} casi, e '
                  f'l\'originale non c\'e\' piu\': impossibile giudicarli')
            problems += 1
        return 1 if problems else 0

    def describe(self) -> str:
        """Una riga su cosa si sta usando come verita', da stampare in cima."""
        if self.recording:
            return f'oracolo: registro le risposte di {self.original}'
        if self.has_original:
            n = len(self.data.get('cases', {}))
            extra = f', registro di confronto: {n} casi' if n else ', nessun registro'
            return f'oracolo: {self.original.name} sul disco{extra}'
        when = self.data.get('recorded_at', 'data ignota')
        n = len(self.data.get('cases', {}))
        return (f'oracolo: registrato ({n} casi, {when}) — '
                f'l\'originale non e\' piu\' sul disco')


def _test() -> int:
    import tempfile
    global STORE
    failures = []

    def check(name, got, want):
        if got != want:
            failures.append(f'{name}: got {got!r}, want {want!r}')

    with tempfile.TemporaryDirectory() as tmp:
        STORE = Path(tmp)
        original = Path(tmp) / 'fake-original.py'
        original.write_text('# c\'e\'\n')

        # 1. registrazione
        o = Oracle('prova', original, argv=['--record'])
        check('registrando si interroga l\'originale',
              o.answer({'x': 1}, lambda: ('go', 'perche')), ['go', 'perche'])
        check('la registrazione esce 0', o.close(), 0)
        check('il file esiste', (Path(tmp) / 'prova.json').exists(), True)

        # 2. con l'originale ancora sul disco: si interroga, e il registro si controlla
        o = Oracle('prova', original, argv=[])
        o.answer({'x': 1}, lambda: ('go', 'perche'))
        check('un registro che coincide non si lamenta', o.close(), 0)

        o = Oracle('prova', original, argv=[])
        o.answer({'x': 1}, lambda: ('stop', 'cambiato'))
        check('un registro invecchiato si vede', len(o.stale), 1)
        check('e fa uscire non-zero', o.close(), 1)

        # 3. senza l'originale: si legge il registro
        original.unlink()
        o = Oracle('prova', original, argv=[])
        check('senza originale si rilegge il registro',
              o.answer({'x': 1}, lambda: (_ for _ in ()).throw(AssertionError(
                  'non si deve interrogare l\'originale che non c\'e\''))),
              ['go', 'perche'])
        check('e non si lamenta', o.close(), 0)

        # 4. un caso che il registro non conosce e' un guasto, non un verde
        o = Oracle('prova', original, argv=[])
        check('un caso ignoto torna None', o.answer({'y': 2}, lambda: None), None)
        check('e fa uscire non-zero', o.close(), 1)

        # 5. i percorsi volatili spariscono da chiave e valore
        volatile = '/var/folders/ab/T/prova-1234'
        o = Oracle('scrub', original, argv=['--record'], scrub=[volatile])
        original.write_text('# torna\n')
        check('il valore esce ripulito',
              o.answer({'home': volatile},
                       lambda: {'detto': f'ho scritto {volatile}/x.md'}),
              {'detto': 'ho scritto <volatile-0>/x.md'})
        o.close()
        # una cartella diversa, stesso caso: deve riconoscerlo
        altro = '/var/folders/cd/T/prova-9999'
        o = Oracle('scrub', original, argv=[], scrub=[altro])
        o.answer({'home': altro}, lambda: {'detto': f'ho scritto {altro}/x.md'})
        check('un percorso nuovo trova lo stesso caso registrato', len(o.stale), 0)
        check('e non ne mancano', len(o.missing), 0)
        check('clean() vale anche per il porto',
              o.clean({'detto': f'ho scritto {altro}/x.md'}),
              {'detto': 'ho scritto <volatile-0>/x.md'})
        original.unlink()

        # 6. LA TRAPPOLA: tuple e liste devono essere la stessa cosa
        check('una tupla normalizzata e\' una lista',
              normalise(('go', '')), ['go', ''])
        check('la chiave non cambia fra tupla e lista',
              case_key(('a', 1)), case_key(['a', 1]))
        check('due casi diversi hanno chiavi diverse',
              case_key({'a': 1}) != case_key({'a': 2}), True)

    for f in failures:
        print(f'FAIL {f}')
    print(f'{"PASS" if not failures else "FAIL"}: {len(failures)} prove rosse')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(_test() if '--test' in sys.argv else 0)
