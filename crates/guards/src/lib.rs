//! I freni: la logica di ciò che si può e non si può fare, senza l'involucro.
//!
//! Ogni modulo qui dentro espone una `judge()` pura — dal comando alla
//! decisione, senza toccare stdin, stderr o l'ambiente. È quello che rende
//! possibile il confronto uno-a-uno con lo script Python che sostituisce: si
//! prova la decisione, non il processo.

pub mod cd_guard;
