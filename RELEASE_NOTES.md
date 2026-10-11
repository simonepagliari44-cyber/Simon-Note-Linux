🎉 Prima release di Simon-Note
Blocco note veloce a schede per Ubuntu/GNOME, scritto in Rust con GTK4 e LibAdwaita.   
🍿 Cosa c'è in questa versione
📑 Editor a schede -- ogni file in una tab separata, con pulsante di chiusura e pallino sulle modifiche   
💾 Caricamento a finestre -- i file grandi non vengono caricati interamente in memoria   
🚪 Chiusura sicura -- "Vuoi salvare le modifiche?" e, scegliendo Salva, il file viene scritto e l'editor si chiude   
🎨 Stile GNOME nativo -- header bar, tema chiaro/scuro, finestra Informazioni con l'icona dell'app   
⚡ I/O asinchrono -- lettura e scrittura in background, interfaccia sempre reattiva   
📂 Più file da riga di comando -- simonnote a.txt b.txt apre una tab per file   
🔎 Scorciatoie -- Ctrl+T / Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+W / Ctrl+Q   
📏 Barra di stato -- righe, dimensione e posizione della finestra   
💬 Notifiche -- conferma di salvataggio ed errori   
📊 Numeri sul caricamento a finestre
File da 30 MB (400.000 righe), misurato:   
Prima: ~240 MB di memoria   
Ora: ~14 MB per il file   
Con 50 MB la memoria resta sostanzialmente invariata   
🔐 Sicurezza dei dati
Quando la finestra dovrebbe scorrere e la porzione che uscirebbe contiene righe modificate, lo scorrimento viene rifiutato e l'editor suggerisce di salvare. Non si perdono dati. Il modulo è coperto da 8 test automatici.   
📦 Installazione
```bash
sudo dpkg -i simonnote_1.0.0-1_amd64.deb
```   
Requisiti: Ubuntu 24.04, GTK 4.12+, libadwaita 1.5+ (normalmente già presenti).   
🐛 Problemi noti
   
Nessuno noto. Se ne trovi, apri una issue.   
Grazie a chi ha provato l'editor e segnalato i problemi! 🙏 
