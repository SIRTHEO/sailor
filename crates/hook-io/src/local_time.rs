//! L'ora locale con l'offset, cioè `time.strftime('%Y-%m-%dT%H:%M:%S%z')`.
//!
//! PERCHÉ NON BASTA L'UTC. Il registro di `linear-sola-lettura` conta 13.560
//! righe con l'ora di Roma e l'offset scritto accanto
//! (`2026-08-17T00:03:00+0200`). Scriverci sopra righe in UTC le lascerebbe
//! formalmente valide e sbagliate di due ore — la divergenza silenziosa che
//! questa migrazione esiste per non produrre.
//!
//! PERCHÉ A MANO. `chrono` porterebbe questa riga a costare una dipendenza e il
//! suo tempo di compilazione, in binari che esistono per partire in pochi
//! millisecondi. Il fuso sta in `/etc/localtime` in formato TZif (RFC 8536),
//! e leggerlo è meno di cento righe che non cambieranno mai.
//!
//! Fail-open verso UTC: se il file manca o è illeggibile si scrive `+0000`, che
//! è vero per una macchina senza fuso configurato. Un registro non deve poter
//! fermare un gancio.

use std::time::{SystemTime, UNIX_EPOCH};

/// «2026-08-17T00:03:00+0200» — l'ora locale, come la scrive `%z`.
pub fn now_local_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_local(secs, local_offset(secs))
}

fn format_local(epoch: i64, offset: i32) -> String {
    let local = epoch + offset as i64;
    let days = local.div_euclid(86_400);
    let rem = local.rem_euclid(86_400);
    let (y, m, d) = crate::journal::civil_from_days(days);
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.unsigned_abs() / 60;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{sign}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        abs / 60,
        abs % 60
    )
}

/// L'offset in secondi valido a quell'istante, letto da `/etc/localtime`.
///
/// `TZ`, quando c'è, la rispettiamo solo nella forma `TZ=Area/Città`: le forme
/// POSIX complete (`CET-1CEST,M3.5.0,M10.5.0/3`) sono un'altra grammatica, e per
/// quelle si ricade sul file di sistema — che su questa macchina è comunque la
/// sorgente vera.
pub fn local_offset(epoch: i64) -> i32 {
    let path = match std::env::var("TZ") {
        Ok(tz) if tz.contains('/') && !tz.starts_with(':') => {
            format!("/usr/share/zoneinfo/{tz}")
        }
        _ => "/etc/localtime".to_string(),
    };
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| offset_from_tzif(&bytes, epoch))
        .unwrap_or(0)
}

/// Legge il blocco a 64 bit di un file TZif e ne ricava l'offset attivo.
///
/// La versione 1 (32 bit) viene saltata: esiste solo per compatibilità, e ogni
/// file installato da vent'anni a questa parte porta anche il blocco lungo. Se
/// il file è di sola versione 1, qui si ricade su UTC — meglio di un'ora
/// inventata.
fn offset_from_tzif(bytes: &[u8], epoch: i64) -> Option<i32> {
    if bytes.len() < 44 || &bytes[..4] != b"TZif" {
        return None;
    }
    if bytes[4] == 0 {
        return None; // solo versione 1
    }
    let first = block_size(bytes, 4)?; // il blocco a 32 bit, da saltare
    let second = 44 + first;
    if bytes.len() < second + 44 || &bytes[second..second + 4] != b"TZif" {
        return None;
    }

    let head = second + 20;
    let counts: Vec<usize> = (0..6)
        .map(|i| read_u32(bytes, head + i * 4).map(|v| v as usize))
        .collect::<Option<_>>()?;
    let (timecnt, typecnt) = (counts[3], counts[4]);
    if typecnt == 0 {
        return None;
    }

    let times = second + 44;
    let indices = times + timecnt * 8;
    let types = indices + timecnt;

    // Il tipo attivo è quello della transizione più recente non successiva
    // all'istante chiesto. Prima della prima transizione vale il primo tipo.
    let mut chosen = 0usize;
    for i in 0..timecnt {
        let when = read_i64(bytes, times + i * 8)?;
        if when > epoch {
            break;
        }
        chosen = *bytes.get(indices + i)? as usize;
    }
    if chosen >= typecnt {
        return None;
    }
    read_u32(bytes, types + chosen * 6).map(|v| v as i32)
}

/// La lunghezza di un blocco dati, dai sei conteggi che lo precedono.
fn block_size(bytes: &[u8], version_at: usize) -> Option<usize> {
    let head = version_at + 16;
    let c: Vec<usize> = (0..6)
        .map(|i| read_u32(bytes, head + i * 4).map(|v| v as usize))
        .collect::<Option<_>>()?;
    let (isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt) = (c[0], c[1], c[2], c[3], c[4], c[5]);
    Some(timecnt * 5 + typecnt * 6 + charcnt + leapcnt * 8 + isstdcnt + isutcnt)
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes(slice.try_into().ok()?))
}

fn read_i64(bytes: &[u8], at: usize) -> Option<i64> {
    let slice = bytes.get(at..at + 8)?;
    Some(i64::from_be_bytes(slice.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'oracolo è `date`: se i due divergono, è questo parser a sbagliare.
    #[test]
    fn it_agrees_with_the_system_clock() {
        let Ok(out) = std::process::Command::new("date").arg("+%z").output() else {
            return; // niente `date`: il test non ha oracolo, non finge di averlo
        };
        let expected = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if expected.is_empty() {
            return;
        }
        let ours = now_local_iso8601();
        assert!(
            ours.ends_with(&expected),
            "atteso un offset {expected}, ottenuto {ours}"
        );
    }

    #[test]
    fn it_writes_the_shape_python_writes() {
        let t = now_local_iso8601();
        assert_eq!(t.len(), 24, "atteso 2026-08-17T00:03:00+0200, ottenuto {t}");
        assert_eq!(&t[10..11], "T");
        assert!(t[19..].starts_with('+') || t[19..].starts_with('-'));
    }

    #[test]
    fn a_known_instant_renders_the_way_the_register_shows_it() {
        // l'ultima riga vera del registro: 2026-08-17T00:03:00+0200
        assert_eq!(format_local(1_786_917_780, 7200), "2026-08-17T00:03:00+0200");
        assert_eq!(format_local(1_786_917_780, 0), "2026-08-16T22:03:00+0000");
        assert_eq!(format_local(1_786_917_780, -18_000), "2026-08-16T17:03:00-0500");
        // offset non intero di ore: mezz'ora esiste davvero (India, Terranova)
        assert_eq!(format_local(1_786_917_780, 19_800), "2026-08-17T03:33:00+0530");
    }

    #[test]
    fn a_file_that_is_not_a_timezone_gives_no_answer() {
        assert_eq!(offset_from_tzif(b"", 0), None);
        assert_eq!(offset_from_tzif(b"non sono un TZif, sono solo del testo", 0), None);
    }
}
