<div align="center">
  <img src="icon.png" width="140" alt="Simon-Note">
  <h1>Simon-Note</h1>
  <p><b>Blocco note veloce a schede per Ubuntu/GNOME, scritto in Rust</b></p>
  <p>
    <img src="https://img.shields.io/badge/GTK4-4.12%2B-blue" alt="GTK4">
    <img src="https://img.shields.io/badge/libadwaita-1.5%2B-blue" alt="libadwaita">
    <img src="https://img.shields.io/badge/Rust-2021-orange" alt="Rust">
    <img src="https://img.shields.io/badge/licenza-MIT%20%7C%20Apache--2.0-green" alt="licenza">
  </p>
</div>

---

## 🚀 Cos'è

Un editor di testo veloce e pulito, costruito con **GTK4** e **LibAdwaita**.
Apre e modifica file UTF-8 anche di grandi dimensioni senza bloccarsi.

## ✨ Caratteristiche

- 🗂️ **Editor a schede** — ogni file aperto in una tab separata, con pulsante di chiusura e pallino sulle modifiche
- 🪟 **Caricamento a finestre** — i file grandi non finiscono tutti in memoria: si caricano a blocchi di 20.000 righe che scorrono navigando
- 🎨 **Stile GNOME nativo** — header bar, tema chiaro/scuro automatico, finestra "Informazioni"
- 💾 **Chiusura sicura** — se ci sono modifiche non salvate ti chiede *"Vuoi salvare le modifiche?"*; scegliendo **Salva** scrive il file e chiude
- ⚡ **I/O asincrono** — lettura e scrittura in background, l'interfaccia resta sempre reattiva
- 🔎 **Scorciatoie da tastiera** — tutto raggiungibile senza il mouse
- 📂 **Più file da riga di comando** — `simonnote a.txt b.txt` apre una tab per file
- 📊 **Barra di stato** — righe, dimensione e posizione della finestra attuale
- 💬 **Notifiche** — conferma di salvataggio ed errori con toast
- 🧩 **Tema chiaro e scuro** — segue l'impostazione di sistema

## ⌨️ Scorciatoie

- `Ctrl+T` / `Ctrl+N` → nuova scheda
- `Ctrl+O` → apri file
- `Ctrl+S` → salva
- `Ctrl+Shift+S` → salva come
- `Ctrl+W` → chiudi la scheda
- `Ctrl+Shift+W` → chiudi le altre schede
- `Ctrl+Q` → esci

## 🔗 Progetto

https://github.com/simonpagl47-cpu/Simon-Note-Linux

## 📦 Installazione

```bash
sudo dpkg -i simonnote_1.0.0-1_amd64.deb
```

L'applicazione compare nel menu applicazioni come **Simon-Note**.
Dipendenze (di norma già presenti su Ubuntu 24.04): GTK 4.12+, libadwaita 1.5+.

## 🔧 Compilazione da sorgente

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config build-essential
cargo build --release
./target/release/simonnote
```

## 🏗️ Packaging Debian

```bash
dpkg-buildpackage -us -uc -b
# genera ../simonnote_1.0.0-1_amd64.deb
```

Il pacchetto installa l'eseguibile in `/usr/bin`, la voce del menu applicazioni,
l'icona in 8 dimensioni, i metadati AppStream e la pagina di manuale.

## 🪟 Come funziona il caricamento a finestre

Per i file con più di 20.000 righe l'editor non carica tutto in memoria:

1. 🔢 all'apertura costruisce un **indice** degli offset di ogni riga, in un thread separato (la UI non si blocca)
2. 📄 nel buffer finisce solo una **finestra** di 20.000 righe
3. 🖱️ scorrendo oltre i bordi la finestra scorre e il blocco successivo viene letto dal disco
4. 💾 al salvataggio viene riscritto **solo** il byte-range coperto dalla finestra: il resto del file viene ricopiato byte per byte

Misurato su un file da 30 MB (400.000 righe): circa **14 MB** per il file,
contro i ~240 MB che servivano prima. Con un file da 50 MB la memoria resta
sostanzialmente invariata. La barra di stato mostra sempre la posizione:
`finestra 4000-23999 di 400000`.

> [!NOTE]
> Quando la finestra dovrebbe scorrere e la porzione che uscirebbe contiene righe modificate, **lo scorrimento viene rifiutato** e la barra di stato suggerisce di salvare. In questo modo non si perdono mai dati.

## 🧪 Test

Il modulo di caricamento a finestre ha una suite di test:

```bash
cargo test
```

Verifica, tra l'altro, che il salvataggio preservi intatte le righe fuori
finestra e che l'inserimento di righe non corrompa il resto del file.

## 📂 Struttura del progetto

- `src/main.rs` → setup dell'applicazione, azioni, scorciatoie, `--help`
- `src/window.rs` → finestra, schede, documenti, I/O, dialoghi
- `src/document.rs` → caricamento a finestre, indice del file, salvataggio
- `src/style.css` → stile GTK
- `icon.png` → icona originale
- `data/icons/` → icona ritagliata nelle varie dimensioni
- `debian/` → packaging Debian

## 📄 Licenza

MIT OR Apache-2.0
