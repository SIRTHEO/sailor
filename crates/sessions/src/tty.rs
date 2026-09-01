//! Qual è il terminale di **questo** processo.
//!
//! **UNA DOMANDA SUL PROPRIO DESCRITTORE, NON SULLA MACCHINA.** `ttyname` legge
//! un descrittore che il processo ha già in mano: non esegue niente, non
//! attraversa nessun perimetro, e non può rispondere «vuoto» al posto di
//! «negato». È la differenza con `ps`, ed è la ragione per cui l'ancora del
//! tracciamento comincia da qui.

/// Il tty di questo processo, col nome corto che usa `ps`.
///
/// **SI GUARDA PRIMA L'ERRORE STANDARD.** Chi arriva da un gancio ha l'ingresso
/// occupato dalla pipe che porta il payload, e spesso anche l'uscita è
/// catturata: il descrittore 2 è l'ultimo a restare attaccato alla finestra.
/// Provarli in quest'ordine è ciò che fa funzionare il caso vero invece del
/// caso comodo.
pub fn current() -> Option<String> {
    for descriptor in [libc::STDERR_FILENO, libc::STDOUT_FILENO, libc::STDIN_FILENO] {
        if let Some(found) = name_of(descriptor) {
            return Some(found);
        }
    }
    None
}

fn name_of(descriptor: i32) -> Option<String> {
    // `ttyname` scrive in un'area statica del processo: la stringa si copia
    // subito, prima di qualunque altra chiamata che potrebbe riusarla.
    let device = unsafe {
        let raw = libc::ttyname(descriptor);
        if raw.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
    };
    if device.is_empty() {
        return None;
    }
    Some(short_name(&device))
}

/// `/dev/ttys004` diventa `ttys004`.
///
/// **DUE NOMI PER LA STESSA COSA SAREBBERO DUE CHIAVI.** `ps` scrive la forma
/// corta, `ttyname` quella lunga: se le due entrassero nel deposito come sono,
/// lo stesso terminale avrebbe due righe e lo stacco varrebbe su una sola.
pub fn short_name(device: &str) -> String {
    device.strip_prefix("/dev/").unwrap_or(device).to_owned()
}
