//! Caricamento a finestre per file di grandi dimensioni.
//!
//! Il `gtk::TextBuffer` contiene solo una porzione contigua del file. Il resto
//! resta su disco e viene riletto quando la finestra scorre, quindi la memoria
//! occupata non cresce con la dimensione del file.
//!
//! Regola di sicurezza: la finestra non scorre se la porzione che verrebbe
//! abbandonata contiene righe modificate dall'utente. In quel caso l'editor
//! segnala di salvare: così non si perdono mai dati.

use gtk4::prelude::*;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Righe tenute nel buffer.
pub const WINDOW_LINES: usize = 20_000;
/// Righe di margine che fanno scorrere la finestra.
const MARGIN_LINES: i64 = 2_000;
/// Margine pubblico per la logica di scorrimento.
pub const MARGIN: i32 = MARGIN_LINES as i32;

pub struct FileWindow {
    path: PathBuf,
    file: File,
    len: u64,
    /// Offset iniziale di ogni riga; l'ultimo elemento è `len`.
    offsets: Vec<u64>,
    total_lines: usize,
    trailing_newline: bool,
    /// Riga assoluta della prima riga nel buffer.
    window_start: usize,
    /// Quante righe del file copre il buffer.
    window_count: usize,
    /// Righe della finestra così come sono sul disco al momento del caricamento.
    base: Vec<String>,
    /// L'utente ha modificato il contenuto della finestra.
    edited: bool,
}

impl FileWindow {
    /// Indicizza il file. Va eseguito fuori dal thread principale.
    pub fn open(path: &Path) -> std::io::Result<(Self, String)> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();

        let mut offsets: Vec<u64> = vec![0];
        let mut buf = vec![0u8; 1 << 20];
        let mut base: u64 = 0;

        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            for i in memchr::memchr_iter(b'\n', &buf[..n]) {
                offsets.push(base + i as u64 + 1);
            }
            base += n as u64;
        }

        let trailing_newline = len > 0 && offsets.last() == Some(&len);
        if trailing_newline {
            // "a\n" è una sola riga: via la riga vuota finale
            offsets.pop();
        }

        let total_lines = offsets.len();
        offsets.push(len);

        let mut w = FileWindow {
            path: path.to_path_buf(),
            file,
            len,
            offsets,
            total_lines,
            trailing_newline,
            window_start: 0,
            window_count: total_lines.min(WINDOW_LINES),
            base: Vec::new(),
            edited: false,
        };

        let (text, lines) = w.read_window()?;
        w.base = lines;
        Ok((w, text))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn window_start(&self) -> usize {
        self.window_start
    }

    pub fn window_count(&self) -> usize {
        self.window_count
    }

    pub fn is_windowed(&self) -> bool {
        self.total_lines > WINDOW_LINES
    }

    pub fn is_dirty(&self) -> bool {
        self.edited
    }

    /// Segnala che l'utente ha modificato il contenuto della finestra.
    pub fn mark_edited(&mut self) {
        self.edited = true;
    }

    /// Il contenuto è tornato identico a quello su disco.
    pub fn mark_saved(&mut self) {
        self.edited = false;
    }

    /// Riposiziona la finestra su `[start, start+count)` rileggendo da disco.
    pub fn set_window(&mut self, start: usize, count: usize) -> std::io::Result<()> {
        self.window_start = start.min(self.total_lines.saturating_sub(1));
        self.window_count = count
            .max(1)
            .min(self.total_lines - self.window_start);
        let (_, lines) = self.read_window()?;
        self.base = lines;
        self.edited = false;
        Ok(())
    }

    /// Righe assolute attualmente nel buffer.
    pub fn abs_range(&self) -> (usize, usize) {
        (self.window_start, self.window_start + self.window_count)
    }

    /// Come `abs_range`, pensato per la barra di stato.
    pub fn abs_range_for_display(&self) -> (usize, usize) {
        self.abs_range()
    }

    fn read_lines(&mut self, from: usize, to: usize) -> std::io::Result<Vec<String>> {
        if from >= to {
            return Ok(Vec::new());
        }
        let start = self.offsets[from];
        let end = self.offsets[to];
        let mut buf = vec![0u8; (end - start) as usize];
        self.file.seek(SeekFrom::Start(start))?;
        self.file.read_exact(&mut buf)?;

        let s = String::from_utf8_lossy(&buf);
        let mut lines: Vec<String> = s.split('\n').map(str::to_string).collect();
        // se il range finisce con '\n' split produce un pezzo vuoto finale
        if lines.len() > to - from && lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        Ok(lines)
    }

    fn read_window(&mut self) -> std::io::Result<(String, Vec<String>)> {
        let to = self.window_start + self.window_count;
        let lines = self.read_lines(self.window_start, to)?;
        let text = lines.join("\n");
        Ok((text, lines))
    }

    /// Indici di riga (relativi alla finestra) diversi dal contenuto su disco.
    pub fn diff_indices(&self, buffer: &[String]) -> HashSet<usize> {
        let mut out = HashSet::new();
        let n = buffer.len().max(self.base.len());
        for i in 0..n {
            let a = buffer.get(i);
            let b = self.base.get(i);
            if a != b {
                out.insert(i);
            }
        }
        out
    }

    /// Nuovo inizio finestra per mettere in vista la riga `first_visible`
    /// della finestra corrente, se il cursore di visualizzazione è vicino
    /// a uno dei bordi.
    fn target_start(&self, first_visible: i64) -> Option<usize> {
        if !self.is_windowed() {
            return None;
        }
        let window_count = self.window_count as i64;
        let margin = MARGIN_LINES;

        let new_start = if first_visible < margin {
            // siamo in cima: carica più in alto
            let back = (margin - first_visible) * 2;
            self.window_start.saturating_sub(back.max(1) as usize)
        } else if first_visible > window_count - margin {
            // siamo in fondo: carica più in basso
            let abs = (self.window_start as i64 + first_visible + margin)
                .clamp(0, self.total_lines as i64 - 1) as usize;
            abs.saturating_sub(margin as usize)
        } else {
            return None;
        };

        let new_start = new_start.min(self.total_lines.saturating_sub(1));
        if new_start == self.window_start {
            return None;
        }
        Some(new_start)
    }

    /// Tenta di scorrere la finestra mantenendo visibile `first_visible`.
    ///
    /// Restituisce `Ok(None)` se non serve scorrere, oppure se la porzione
    /// che uscirebbe dalla finestra contiene modifiche non ancora salvate:
    /// in quel caso lo scorrimento viene rinviato per non perdere dati.
    pub fn try_shift_visible(
        &mut self,
        first_visible: i64,
        buffer: &[String],
    ) -> std::io::Result<Option<(String, usize)>> {
        let Some(new_start) = self.target_start(first_visible) else {
            return Ok(None);
        };

        // Non perdere modifiche. Quando la finestra scende escono le righe
        // in cima, quando sale escono quelle in fondo.
        let dirty = self.diff_indices(buffer);
        let old_start = self.window_start;
        let leaving: std::ops::Range<usize> = if new_start < old_start {
            new_start..old_start
        } else {
            old_start..new_start
        };
        if leaving.clone().any(|l| dirty.contains(&(l - old_start))) {
            return Ok(None);
        }

        self.window_start = new_start;
        self.window_count = WINDOW_LINES.min(self.total_lines - new_start);
        let (text, lines) = self.read_window()?;
        self.base = lines;
        self.edited = false;
        Ok(Some((text, new_start)))
    }

    /// Scrive il buffer sul disco sostituendo il range occupato dalla finestra.
    pub fn save(&mut self, buffer: &[String]) -> std::io::Result<()> {
        let (start_off, end_off) = self.byte_range();

        let tmp = self.path.with_extension("simonnote.tmp");
        let mut out = File::create(&tmp)?;

        if start_off > 0 {
            let mut buf = vec![0u8; start_off as usize];
            self.file.seek(SeekFrom::Start(0))?;
            self.file.read_exact(&mut buf)?;
            out.write_all(&buf)?;
        }

        let mut chunk = String::new();
        for (i, line) in buffer.iter().enumerate() {
            if i > 0 {
                chunk.push('\n');
            }
            chunk.push_str(line);
            if chunk.len() > 1 << 20 {
                out.write_all(chunk.as_bytes())?;
                chunk.clear();
            }
        }
        // il file terminava con un a capo? mantieni la convenzione
        if self.trailing_newline && !buffer.is_empty() {
            chunk.push('\n');
        }
        if !chunk.is_empty() {
            out.write_all(chunk.as_bytes())?;
        }

        if end_off < self.len {
            let mut buf = vec![0u8; (self.len - end_off) as usize];
            self.file.seek(SeekFrom::Start(end_off))?;
            self.file.read_exact(&mut buf)?;
            out.write_all(&buf)?;
        }

        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, &self.path)?;

        self.file = File::open(&self.path)?;
        self.len = self.file.metadata()?.len();
        self.edited = false;
        self.reindex()
    }

    fn byte_range(&self) -> (u64, u64) {
        (
            self.offsets[self.window_start],
            self.offsets[self.window_start + self.window_count],
        )
    }

    fn reindex(&mut self) -> std::io::Result<()> {
        let (fresh, _) = Self::open(&self.path.clone())?;
        self.offsets = fresh.offsets;
        self.total_lines = fresh.total_lines;
        self.trailing_newline = fresh.trailing_newline;
        self.file = fresh.file;
        self.len = fresh.len;

        let start = self.window_start.min(self.total_lines.saturating_sub(1));
        self.window_start = start;
        self.window_count = WINDOW_LINES.min(self.total_lines - start);
        let (_, lines) = self.read_window()?;
        self.base = lines;
        Ok(())
    }
}

/// Righe correnti del buffer.
pub fn buffer_lines(buffer: &gtk4::TextBuffer) -> Vec<String> {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    buffer
        .text(&start, &end, false)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// Righe correnti del buffer, troncate a `max` elementi.
pub fn buffer_lines_capped(buffer: &gtk4::TextBuffer, max: usize) -> Vec<String> {
    let mut v = buffer_lines(buffer);
    v.truncate(max);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_file(dir: &Path, n: usize) -> PathBuf {
        let p = dir.join("t.txt");
        let mut f = File::create(&p).unwrap();
        for i in 0..n {
            writeln!(f, "riga {i}").unwrap();
        }
        p
    }

    fn read_all(p: &Path) -> Vec<String> {
        std::fs::read_to_string(p)
            .unwrap()
            .split('\n')
            .map(str::to_string)
            .collect()
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("simonnote-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_piccolo_intero() {
        let d = tmpdir("small");
        let p = make_file(&d, 100);
        let (fw, text) = FileWindow::open(&p).unwrap();
        assert!(!fw.is_windowed());
        assert_eq!(fw.total_lines(), 100);
        assert_eq!(text.lines().count(), 100);
        assert!(text.starts_with("riga 0\nriga 1\nriga 2\n"));
        assert!(text.ends_with("riga 99"));
    }

    #[test]
    fn file_grande_usa_finestra() {
        let d = tmpdir("big");
        let p = make_file(&d, WINDOW_LINES * 3);
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        assert!(fw.is_windowed());
        assert_eq!(fw.total_lines(), WINDOW_LINES * 3);
        assert_eq!(fw.window_count(), WINDOW_LINES);
        assert_eq!(text.lines().count(), WINDOW_LINES);
        assert!(text.starts_with("riga 0\nriga 1\n"));
        // nessuno scorrimento utile: siamo al centro del file
        assert!(fw
            .try_shift_visible(MARGIN_LINES + 10, &text.lines().map(str::to_string).collect::<Vec<_>>())
            .unwrap()
            .is_none());
    }

    #[test]
    fn scorrimento_verso_il_fondo() {
        let d = tmpdir("down");
        let p = make_file(&d, WINDOW_LINES * 4);
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let near = WINDOW_LINES as i64 - 5;
        let (new_text, new_start) = fw.try_shift_visible(near, &lines).unwrap().unwrap();
        assert!(new_start > 0, "la finestra deve scendere");
        let first: usize = new_text
            .lines()
            .next()
            .unwrap()
            .rsplit(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(first, new_start, "il testo deve partire da new_start");
        assert_eq!(new_text.lines().count(), WINDOW_LINES);
    }

    #[test]
    fn scorrimento_bloccato_se_ci_sono_modifiche() {
        let d = tmpdir("blocked");
        let p = make_file(&d, WINDOW_LINES * 4);
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        // modifica una riga in cima: uscirà dalla finestra quando si scende
        let leaving = 3;
        lines[leaving] = "MODIFICATA".to_string();
        let near = WINDOW_LINES as i64 - 5;
        assert!(
            fw.try_shift_visible(near, &lines).unwrap().is_none(),
            "lo scorrimento deve essere rifiutato per non perdere la modifica"
        );
    }

    #[test]
    fn salvataggio_preserva_il_resto_del_file() {
        let d = tmpdir("save");
        let p = make_file(&d, WINDOW_LINES * 3);
        let total_before = WINDOW_LINES * 3;
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

        lines[10] = "riga 10 MODIFICATA".to_string();
        lines[WINDOW_LINES - 1] = "ultima riga finestra".to_string();
        fw.save(&lines).unwrap();

        let after = read_all(&p);
        // il file ha una riga in più a fine riga (newline finale)
        assert_eq!(after.len(), total_before + 1, "conteggio righe");
        assert_eq!(after[0], "riga 0");
        assert_eq!(after[10], "riga 10 MODIFICATA", "riga modificata nel buffer");
        assert_eq!(after[11], "riga 11", "riga successiva intatta");
        assert_eq!(
            after[WINDOW_LINES - 1],
            "ultima riga finestra",
            "ultima riga della finestra"
        );
        assert_eq!(
            after[WINDOW_LINES],
            format!("riga {WINDOW_LINES}"),
            "prima riga dopo la finestra: deve essere la riga originale"
        );
        assert_eq!(
            after[total_before - 1],
            format!("riga {}", total_before - 1),
            "ultima riga originale intatta"
        );
    }

    #[test]
    fn salvataggio_dopo_righe_aggiunte() {
        let d = tmpdir("add");
        let p = make_file(&d, WINDOW_LINES * 3);
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

        lines.insert(5, "riga nuova A".to_string());
        lines.insert(6, "riga nuova B".to_string());
        fw.save(&lines).unwrap();

        let after = read_all(&p);
        assert_eq!(after.len(), WINDOW_LINES * 3 + 3); // +2 righe, +1 per il newline finale
        assert_eq!(after[5], "riga nuova A");
        assert_eq!(after[6], "riga nuova B");
        assert_eq!(after[7], "riga 5");
        assert_eq!(after[1], "riga 1");
    }

    #[test]
    fn file_senza_newline_finale() {
        let d = tmpdir("nonewline");
        let p = d.join("n.txt");
        std::fs::write(&p, "a\nb\nc").unwrap();
        let (mut fw, text) = FileWindow::open(&p).unwrap();
        assert_eq!(fw.total_lines(), 3);
        assert_eq!(text, "a\nb\nc");
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[2] = "C".to_string();
        fw.save(&lines).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nb\nC");
    }

    #[test]
    fn file_vuoto() {
        let d = tmpdir("empty");
        let p = d.join("e.txt");
        std::fs::write(&p, "").unwrap();
        let (fw, text) = FileWindow::open(&p).unwrap();
        assert_eq!(fw.total_lines(), 1);
        assert_eq!(text, "");
    }
}
