use adw::prelude::*;
use adw::subclass::prelude::*;
use crate::document::{self, FileWindow};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk4::Accessible, gtk4::Buildable,
                    gtk4::ConstraintTarget, gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

/// Dimensione in byte in forma leggibile.
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Un documento aperto: buffer di testo, vista e percorso su disco.
pub struct Document {
    pub buffer: gtk4::TextBuffer,
    pub view: gtk4::TextView,
    pub scroller: gtk4::ScrolledWindow,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Presente per i file aperti da disco: permette il caricamento a finestre.
    pub file: RefCell<Option<FileWindow>>,
    /// I segnali sono già stati collegati.
    connected: Cell<bool>,
}

impl Document {
    fn new() -> Self {
        let buffer = gtk4::TextBuffer::new(None);
        // Ctrl+Z / Ctrl+Shift+Z: senza questo GTK4 non tiene lo storico
        buffer.set_enable_undo(true);
        buffer.set_max_undo_levels(200);
        let view = gtk4::TextView::builder()
            .buffer(&buffer)
            .monospace(true)
            .wrap_mode(gtk4::WrapMode::None)
            .cursor_visible(true)
            .left_margin(12)
            .right_margin(12)
            .top_margin(12)
            .bottom_margin(12)
            .build();

        let mut tabs = pango::TabArray::new(1, true);
        tabs.set_tab(0, pango::TabAlign::Left, 4 * 1024);
        view.set_tabs(&tabs);

        let scroller = gtk4::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .kinetic_scrolling(true)
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&view)
            .build();

        Document {
            buffer,
            view,
            scroller,
            path: None,
            dirty: false,
            file: RefCell::new(None),
            connected: Cell::new(false),
        }
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("senza-titolo")
                .to_string(),
            None => "Senza titolo".to_string(),
        }
    }

    /// Nome mostrato nella scheda: aggiunge il pallino se il documento è modificato.
    pub fn tab_title(&self) -> String {
        if self.dirty {
            format!("• {}", self.title())
        } else {
            self.title()
        }
    }
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }

    pub fn toast(&self, msg: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(msg));
    }

    /* ---------------- Documenti e schede ---------------- */

    /// Aggiunge una scheda vuota e la seleziona. Restituisce l'indice.
    pub fn new_tab(&self) -> usize {
        self.imp().push_document(None, gtk4::TextBuffer::new(None))
    }

    /// Apre `path` in una nuova scheda (riutilizza la scheda vuota corrente, se presente).
    pub fn open_path(&self, path: &Path) -> usize {
        self.imp().push_document(Some(path.to_path_buf()), gtk4::TextBuffer::new(None))
    }

    pub fn n_tabs(&self) -> i32 {
        self.imp().tab_view.n_pages()
    }

    pub fn close_current_tab(&self) {
        if let Some(page) = self.imp().tab_view.selected_page() {
            self.imp().tab_view.close_page(&page);
        }
    }

    pub fn close_other_tabs(&self) {
        if let Some(page) = self.imp().tab_view.selected_page() {
            self.imp().tab_view.close_pages_before(&page);
            self.imp().tab_view.close_pages_after(&page);
        }
    }

    /* ---------------- Salvataggio ---------------- */

    pub fn save_current(&self) {
        let Some(idx) = self.imp().selected_index() else {
            return;
        };
        match self.imp().docs.borrow()[idx].path.clone() {
            Some(p) => {
                self.write_doc(idx, &p, None);
            }
            None => self.save_current_as(),
        }
    }

    pub fn save_current_as(&self) {
        let imp = self.imp();
        let Some(idx) = imp.selected_index() else {
            return;
        };
        let suggested = imp.docs.borrow()[idx]
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "senza-titolo.txt".to_string());

        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Salva come");
        dialog.set_modal(true);
        dialog.set_initial_name(Some(&suggested));
        if let Some(parent) = imp.docs.borrow()[idx].path.as_ref().and_then(|p| p.parent().map(|x| x.to_path_buf())) {
            dialog.set_initial_folder(Some(&gio::File::for_path(parent)));
        }

        let window = self.clone();
        dialog.save(Some(self), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    window.write_doc(idx, &path, None);
                }
            }
        });
    }

    /// Scrive il documento `idx` su `path`.
    /// `on_done(true)` viene chiamato al termine, dopo l'aggiornamento dello stato.
    pub fn write_doc(
        &self,
        idx: usize,
        path: &Path,
        on_done: Option<Rc<dyn Fn(bool)>>,
    ) {
        let imp = self.imp();
        let view = match imp.docs.borrow().get(idx) {
            Some(d) => d.view.clone(),
            None => return,
        };
        let window = self.clone();
        let path = path.to_path_buf();

        // Se il documento usa il caricamento a finestre, la scrittura deve
        // sostituire solo la porzione coperta dal buffer: tutto il resto del
        // file viene ricopiato byte per byte.
        let windowed = {
            let docs = imp.docs.borrow();
            let cell = &docs[idx].file;
            let fw = cell.borrow();
            fw.as_ref().map(|f| (f.window_start(), f.path().to_path_buf()))
        };

        if let Some((win_start, cur_path)) = windowed {
            let lines = {
                let docs = imp.docs.borrow();
                document::buffer_lines(&docs[idx].buffer)
            };
            let n_lines = lines.len();
            imp.spinner.set_spinning(true);
            imp.status.set_text("Salvataggio in corso…");

            let (tx, rx) = std::sync::mpsc::channel::<Result<FileWindow, String>>();
            std::thread::spawn(move || {
                let r = match FileWindow::open(&cur_path) {
                    Err(e) => Err(format!("{e}")),
                    Ok((mut fw, _)) => match fw
                        .set_window(win_start, fw.window_count())
                        .and_then(|()| fw.save(&lines))
                    {
                        Ok(()) => Ok(fw),
                        Err(e) => Err(format!("{e}")),
                    },
                };
                let _ = tx.send(r);
            });

            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                let Ok(res) = rx.try_recv() else {
                    return glib::ControlFlow::Continue;
                };
                let imp = window.imp();
                imp.spinner.set_spinning(false);
                let ok = res.is_ok();

                if let Some(i) = imp.index_of_view(&view) {
                    match res {
                        Ok(fw) => {
                            let start = win_start.min(fw.total_lines().saturating_sub(1));
                            let mut fw = fw;
                            let _ = fw.set_window(start, n_lines);
                            {
                                let mut docs = imp.docs.borrow_mut();
                                if let Some(d) = docs.get_mut(i) {
                                    d.path = Some(path.clone());
                                    d.dirty = false;
                                    let mut cell = d.file.borrow_mut();
                                    *cell = Some(fw);
                                }
                            }
                            imp.sync_page(i);
                            imp.sync_status_windowed(i);
                            window.toast("Salvato");
                        }
                        Err(msg) => {
                            window.toast(&format!("Salvataggio fallito: {msg}"));
                        }
                    }
                }
                if let Some(cb) = &on_done {
                    cb(ok);
                }
                glib::ControlFlow::Break
            });
            return;
        }

        // Documento senza file a finestre (piccolo o nuovo): scrittura diretta
        let text = {
            let docs = imp.docs.borrow();
            let Some(doc) = docs.get(idx) else { return };
            let start = doc.buffer.start_iter();
            let end = doc.buffer.end_iter();
            doc.buffer.text(&start, &end, false).to_string()
        };

        gio::File::for_path(&path).replace_contents_async(
            text.into_bytes(),
            None::<&str>,
            false,
            gio::FileCreateFlags::REPLACE_DESTINATION,
            gio::Cancellable::NONE,
            move |res| {
                let ok = res.is_ok();
                let imp = window.imp();
                if ok {
                    if let Some(i) = imp.index_of_view(&view) {
                        if let Some(doc) = imp.docs.borrow_mut().get_mut(i) {
                            doc.path = Some(path);
                            doc.dirty = false;
                        }
                        imp.sync_page(i);
                        window.toast("Salvato");
                    }
                } else if let Err((_, e)) = res {
                    window.toast(&format!("Salvataggio fallito: {e}"));
                }
                if let Some(cb) = on_done {
                    cb(ok);
                }
            },
        );
    }

    /// Salva come con percorso scelto dall'utente; `on_done(true)` se salvato.
    pub fn save_as_for(&self, idx: usize, on_done: Rc<dyn Fn(bool)>) {
        let imp = self.imp();
        let suggested = imp
            .docs
            .borrow()
            .get(idx)
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "senza-titolo.txt".to_string());

        let parent = imp
            .docs
            .borrow()
            .get(idx)
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.parent().map(|x| x.to_path_buf()));

        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Salva come");
        dialog.set_modal(true);
        dialog.set_initial_name(Some(&suggested));
        if let Some(p) = parent {
            dialog.set_initial_folder(Some(&gio::File::for_path(p)));
        }

        let window = self.clone();
        dialog.save(Some(self), gio::Cancellable::NONE, move |res| {
            match res.ok().and_then(|f| f.path()) {
                Some(path) => window.write_doc(idx, &path, Some(on_done)),
                None => on_done(false),
            }
        });
    }

    pub fn open_file_dialog(&self) {
        let imp = self.imp();
        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Apri file");
        dialog.set_modal(true);

        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        let text = gtk4::FileFilter::new();
        text.set_name(Some("File di testo"));
        text.add_mime_type("text/plain");
        text.add_mime_type("text/markdown");
        for pat in ["*.txt", "*.md", "*.log", "*.json", "*.toml", "*.yaml", "*.yml", "*.csv"] {
            text.add_pattern(pat);
        }
        filters.append(&text);
        let all = gtk4::FileFilter::new();
        all.set_name(Some("Tutti i file"));
        all.add_pattern("*");
        filters.append(&all);
        dialog.set_filters(Some(&filters));

        if let Some(p) = imp
            .docs
            .borrow()
            .iter()
            .rev()
            .find_map(|d| d.path.as_ref())
            .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        {
            dialog.set_initial_folder(Some(&gio::File::for_path(p)));
        }

        let window = self.clone();
        dialog.open(Some(self), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    window.open_path(&path);
                }
            }
        });
    }

    /* ---------------- Chiusura finestra ---------------- */

    /// Tenta di chiudere la finestra: salva i documenti modificati, poi chiude.
    pub fn request_close(&self) {
        self.imp().try_close();
    }
}

mod imp {
    use super::*;
    use gtk4::Box as GtkBox;

    pub struct Window {
        pub tab_bar: adw::TabBar,
        pub tab_view: adw::TabView,
        pub toast_overlay: adw::ToastOverlay,
        pub status: gtk4::Label,
        pub spinner: gtk4::Spinner,
        pub window_title: adw::WindowTitle,
        pub docs: RefCell<Vec<Document>>,
        /// true mentre il testo viene impostato programmaticamente
        pub applying: Cell<bool>,
        /// true durante uno scorrimento di finestra
        pub shifting: Cell<bool>,
        /// Riga del buffer in cui si trova il cursore.
        pub cursor_line: Cell<i32>,
        /// true mentre un salvataggio automatico è in corso
        pub saving: Cell<bool>,
        /// true quando la finestra può chiudersi senza ulteriori conferme
        pub allow_close: Cell<bool>,
        pub seq: Cell<u32>,
    }

    impl Default for Window {
        fn default() -> Self {
            Window {
                tab_bar: adw::TabBar::new(),
                tab_view: adw::TabView::new(),
                toast_overlay: adw::ToastOverlay::new(),
                status: gtk4::Label::new(Some("Pronto")),
                spinner: gtk4::Spinner::new(),
                window_title: adw::WindowTitle::new("Simon-Note", ""),
                docs: RefCell::new(Vec::new()),
                applying: Cell::new(false),
                shifting: Cell::new(false),
                cursor_line: Cell::new(0),
                saving: Cell::new(false),
                allow_close: Cell::new(false),
                seq: Cell::new(0),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "SimonNoteWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(_klass: &mut Self::Class) {}
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.set_default_size(1000, 680);
            obj.set_icon_name(Some("com.simonecompany.simonnote"));

            /* ---------------- Schede ---------------- */
            self.tab_bar.set_view(Some(&self.tab_view));
            self.tab_bar.set_autohide(false);
            self.tab_view.set_shortcuts(adw::TabViewShortcuts::all());

            let scroller_stack = GtkBox::new(gtk4::Orientation::Vertical, 0);
            scroller_stack.append(&self.tab_view);

            let toolbar = adw::ToolbarView::new();
            toolbar.add_top_bar(&self.tab_bar);
            toolbar.set_content(Some(&scroller_stack));

            /* ---------------- Barra di stato ---------------- */
            self.status.set_xalign(0.0);
            self.status.add_css_class("dim-label");
            self.status.add_css_class("caption");
            self.spinner.set_valign(gtk4::Align::Center);

            let status_box = GtkBox::new(gtk4::Orientation::Horizontal, 8);
            status_box.set_margin_start(12);
            status_box.set_margin_end(12);
            status_box.set_margin_top(4);
            status_box.set_margin_bottom(4);
            status_box.append(&self.spinner);
            status_box.append(&self.status);

            let content = GtkBox::new(gtk4::Orientation::Vertical, 0);
            content.append(&toolbar);
            content.append(&status_box);
            self.toast_overlay.set_child(Some(&content));

            /* ---------------- Header bar ---------------- */
            let menu = gio::Menu::new();
            menu.append(Some("_Nuova scheda"), Some("win.new"));
            menu.append(Some("_Apri…"), Some("win.open"));
            let s0 = gio::Menu::new();
            s0.append(Some("_Annulla"), Some("win.undo"));
            s0.append(Some("_Ripeti"), Some("win.redo"));
            menu.append_section(None, &s0);
            let s1 = gio::Menu::new();
            s1.append(Some("_Salva"), Some("win.save"));
            s1.append(Some("Salva _come…"), Some("win.save-as"));
            menu.append_section(None, &s1);
            let s2 = gio::Menu::new();
            s2.append(Some("_Chiudi scheda"), Some("win.close-tab"));
            s2.append(Some("Chiudi _altre schede"), Some("win.close-others"));
            menu.append_section(None, &s2);
            let s3 = gio::Menu::new();
            s3.append(Some("_Esci"), Some("app.quit"));
            menu.append_section(None, &s3);

            let menu_button = gtk4::MenuButton::builder()
                .icon_name("open-menu-symbolic")
                .tooltip_text("Menu principale")
                .menu_model(&menu)
                .primary(true)
                .build();

            // icona dell'app, a sinistra del menu principale
            let app_image = gtk4::Image::from_icon_name("com.simonecompany.simonnote");
            app_image.set_pixel_size(24);
            let about_button = gtk4::Button::builder()
                .child(&app_image)
                .tooltip_text("Informazioni su Simon-Note")
                .css_classes(["flat"])
                .build();
            let o = obj.clone();
            about_button.connect_clicked(move |_| {
                let about = adw::AboutWindow::new();
                about.set_application_name("Simon-Note");
                about.set_application_icon("com.simonecompany.simonnote");
                about.set_version(env!("CARGO_PKG_VERSION"));
                about.set_developer_name("Simone");
                about.set_comments(
                    "Blocco note veloce a schede per GNOME, scritto in Rust \
                     con GTK4 e LibAdwaita.\nSupporta il caricamento a finestre \
                     per file di grandi dimensioni.",
                );
                about.set_website("https://github.com/simonepagliari44-cyber/Simon-Note-Linux");
                about.set_issue_url("https://github.com/simonepagliari44-cyber/Simon-Note-Linux/issues");
                about.set_license_type(gtk4::License::MitX11);
                about.set_copyright("© Simone");
                about.set_transient_for(Some(&o));
                about.set_modal(true);
                about.present();
            });

            let new_button = gtk4::Button::from_icon_name("list-add-symbolic");
            new_button.set_tooltip_text(Some("Nuova scheda (Ctrl+T)"));
            let o = obj.clone();
            new_button.connect_clicked(move |_| {
                o.new_tab();
            });

            let open_button = gtk4::Button::from_icon_name("document-open-symbolic");
            open_button.set_tooltip_text(Some("Apri (Ctrl+O)"));
            let o = obj.clone();
            open_button.connect_clicked(move |_| o.open_file_dialog());

            let save_button = gtk4::Button::from_icon_name("document-save-symbolic");
            save_button.set_tooltip_text(Some("Salva (Ctrl+S)"));
            let o = obj.clone();
            save_button.connect_clicked(move |_| o.save_current());

            let header = adw::HeaderBar::new();
            header.pack_start(&about_button);
            header.pack_start(&menu_button);
            header.pack_start(&new_button);
            header.pack_start(&open_button);
            header.pack_end(&save_button);
            header.set_title_widget(Some(&self.window_title));

            let root = GtkBox::new(gtk4::Orientation::Vertical, 0);
            root.append(&header);
            root.append(&self.toast_overlay);
            obj.set_content(Some(&root));

            /* ---------------- Azioni finestra ---------------- */
            let a_new = gio::SimpleAction::new("new", None);
            let o = obj.clone();
            a_new.connect_activate(move |_, _| {
                o.new_tab();
            });
            obj.add_action(&a_new);

            let a_open = gio::SimpleAction::new("open", None);
            let o = obj.clone();
            a_open.connect_activate(move |_, _| o.open_file_dialog());
            obj.add_action(&a_open);

            let a_save = gio::SimpleAction::new("save", None);
            let o = obj.clone();
            a_save.connect_activate(move |_, _| o.save_current());
            obj.add_action(&a_save);

            let a_save_as = gio::SimpleAction::new("save-as", None);
            let o = obj.clone();
            a_save_as.connect_activate(move |_, _| o.save_current_as());
            obj.add_action(&a_save_as);

            let a_close_tab = gio::SimpleAction::new("close-tab", None);
            let o = obj.clone();
            a_close_tab.connect_activate(move |_, _| o.close_current_tab());
            obj.add_action(&a_close_tab);

            // NB: il buffer va clonato fuori dal borrow, perché undo() e
            // redo() emettono "changed" in modo sincrono e l'handler
            // ha bisogno di docs.borrow_mut().
            let a_undo = gio::SimpleAction::new("undo", None);
            let o = obj.clone();
            a_undo.connect_activate(move |_, _| {
                let imp = o.imp();
                let buffer = imp
                    .selected_index()
                    .and_then(|i| imp.docs.borrow().get(i).map(|d| d.buffer.clone()));
                if let Some(b) = buffer {
                    b.undo();
                }
            });
            obj.add_action(&a_undo);

            let a_redo = gio::SimpleAction::new("redo", None);
            let o = obj.clone();
            a_redo.connect_activate(move |_, _| {
                let imp = o.imp();
                let buffer = imp
                    .selected_index()
                    .and_then(|i| imp.docs.borrow().get(i).map(|d| d.buffer.clone()));
                if let Some(b) = buffer {
                    b.redo();
                }
            });
            obj.add_action(&a_redo);

            let a_close_others = gio::SimpleAction::new("close-others", None);
            let o = obj.clone();
            a_close_others.connect_activate(move |_, _| o.close_other_tabs());
            obj.add_action(&a_close_others);

            /* ---------------- Prima scheda ---------------- */
            obj.new_tab();

            /* ---------------- Segnali ---------------- */

            // Quando l'utente raggiunge il bordo della finestra non arrivano
            // più eventi di scorrimento: un controllo periodico garantisce
            // che la finestra prosegua oltre.
            let o = obj.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
                let imp = o.imp();
                let sel = imp.selected_index();
                if let Some(i) = sel {
                    let windowed = {
                        let docs = imp.docs.borrow();
                        match docs.get(i) {
                            Some(d) => d
                                .file
                                .borrow()
                                .as_ref()
                                .map(|f| f.is_windowed())
                                .unwrap_or(false),
                            None => false,
                        }
                    };
                    if windowed {
                        imp.maybe_shift(i);
                    }
                    imp.refresh_dirty(i);
                    imp.sync_status_cursor(i);
                }
                glib::ControlFlow::Continue
            });


            // collega i segnali dei documenti via e via che se ne aprono
            let o = obj.clone();
            self.tab_view
                .connect_page_attached(move |_, _, _| o.imp().connect_documents());
            self.connect_documents();

            // cambio scheda selezionata
            let o = obj.clone();
            self.tab_view
                .connect_selected_page_notify(move |_| o.imp().sync_title());

            // riordino schede: riallinea l'array dei documenti
            let o = obj.clone();
            self.tab_view
                .connect_page_reordered(move |_, _, pos| o.imp().on_reordered(pos));

            // richiesta di chiusura scheda
            let o = obj.clone();
            self.tab_view.connect_close_page(move |_, page| {
                if o.imp().allow_close_page(page) {
                    glib::Propagation::Proceed
                } else {
                    glib::Propagation::Stop
                }
            });

            // rimozione scheda: allinea l'array dei documenti
            let o = obj.clone();
            self.tab_view
                .connect_page_detached(move |_, _, pos| o.imp().on_page_detached(pos));

            // senza schede la finestra si chiude
            let o = obj.clone();
            self.tab_view.connect_n_pages_notify(move |view| {
                if view.n_pages() == 0 {
                    o.imp().allow_close.set(true);
                    o.close();
                }
            });

            // chiusura finestra
            obj.connect_close_request(move |win| {
                if win.imp().allow_close.get() {
                    return glib::Propagation::Proceed;
                }
                win.request_close();
                glib::Propagation::Stop
            });
        }
    }

    impl imp::Window {
        /// Collega il segnale `changed` di ogni buffer già presente.
        pub fn connect_documents(&self) {
            let obj = self.obj();
            let count = self.docs.borrow().len();
            for idx in 0..count {
                self.connect_document(idx, obj.clone());
            }
        }

        /// Collega i segnali del documento (una sola volta).
        pub fn connect_document(&self, idx: usize, obj: super::Window) {
            let view = {
                let docs = self.docs.borrow();
                let Some(d) = docs.get(idx) else { return };
                if d.connected.get() {
                    return;
                }
                d.connected.set(true);
                d.view.clone()
            };

            // Le modifiche automatiche (caricamento, scorrimento della
            // finestra) non devono rendere il documento "modificato".
            let o = obj.clone();
            let v_changed = view.clone();
            view.buffer().connect_changed(move |_| {
                let imp = o.imp();
                if imp.applying.get() || imp.shifting.get() {
                    return;
                }
                let Some(i) = imp.index_of_view(&v_changed) else {
                    return;
                };
                {
                    let mut docs = imp.docs.borrow_mut();
                    if let Some(doc) = docs.get_mut(i) {
                        doc.dirty = true;
                        let mut fw = doc.file.borrow_mut();
                        if let Some(f) = fw.as_mut() {
                            f.mark_edited();
                        }
                    }
                }
                imp.sync_page(i);
                imp.sync_status_windowed(i);
                imp.sync_title();
            });

            // lo scrollbar in fondo o in cima deve far scorrere la finestra
            let o = obj.clone();
            let v = view.clone();
            if let Some(adj) = view.vadjustment() {
                adj.connect_value_changed(move |_| {
                    let imp = o.imp();
                    if let Some(i) = imp.index_of_view(&v) {
                        imp.maybe_shift(i);
                    }
                });
            }

            // con la tastiera il cursore si muove: lo scrollbar segue e
            // l'ancora della finestra si aggiorna dai segnali di scroll
            let o = obj.clone();
            let v = view.clone();
            view.connect_move_cursor(move |_, _, _, _| {
                let imp = o.imp();
                if let Some(i) = imp.index_of_view(&v) {
                    imp.maybe_shift(i);
                }
            });
        }

        /// Ritorna la scheda associata al documento `idx`.
        pub fn page_for(&self, idx: usize) -> Option<adw::TabPage> {
            let scroller = self.docs.borrow().get(idx).map(|d| d.scroller.clone())?;
            Some(self.tab_view.page(&scroller))
        }

        pub fn index_of_view(&self, view: &gtk4::TextView) -> Option<usize> {
            self.docs
                .borrow()
                .iter()
                .position(|d| d.view == *view)
        }

        pub fn selected_index(&self) -> Option<usize> {
            let page = self.tab_view.selected_page()?;
            let pos = self.tab_view.page_position(&page);
            if pos < 0 {
                return None;
            }
            let pos = pos as usize;
            if pos < self.docs.borrow().len() {
                Some(pos)
            } else {
                None
            }
        }

        /// Crea scheda (vuota o caricata da `path`) e la seleziona.
        pub fn push_document(&self, path: Option<PathBuf>, _unused: gtk4::TextBuffer) -> usize {
            let obj = self.obj();
            let doc = Document::new();
            let child = doc.scroller.clone().upcast::<gtk4::Widget>();
            let page = self.tab_view.append(&child);
            page.set_title(&doc.tab_title());
            if let Ok(icon) = gio::Icon::for_string("text-x-generic-symbolic") {
                page.set_icon(Some(&icon));
            }
            page.set_loading(path.is_some());

            self.docs.borrow_mut().push(doc);
            let idx = self.docs.borrow().len() - 1;
            self.tab_view.set_selected_page(&page);
            self.connect_document(idx, obj.clone());
            self.sync_title();

            if let Some(p) = path {
                self.load_into(idx, &p);
            }
            idx
        }

        /// Carica un file. L'indicizzazione (necessaria per il caricamento a
        /// finestre) gira in un thread separato per non bloccare la UI.
        fn load_into(&self, idx: usize, path: &Path) {
            let obj = self.obj().clone();
            self.spinner.set_spinning(true);
            self.status.set_text("Caricamento in corso…");

            let (tx, rx) = std::sync::mpsc::channel::<
                Result<(FileWindow, String), String>,
            >();
            let p_thread = path.to_path_buf();
            let p_doc = path.to_path_buf();

            std::thread::spawn(move || {
                let r = match FileWindow::open(&p_thread) {
                    Ok((fw, text)) => Ok((fw, text)),
                    Err(e) => Err(format!("{e}")),
                };
                let _ = tx.send(r);
            });

            // polling del risultato senza bloccare il main loop
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                let Ok(res) = rx.try_recv() else {
                    return glib::ControlFlow::Continue;
                };
                let imp = obj.imp();
                let view = {
                    let docs = imp.docs.borrow();
                    match docs.get(idx) {
                        Some(d) => d.view.clone(),
                        None => return glib::ControlFlow::Break,
                    }
                };
                if imp.index_of_view(&view).is_none() {
                    return glib::ControlFlow::Break;
                }
                imp.spinner.set_spinning(false);
                if let Some(page) = imp.page_for(idx) {
                    page.set_loading(false);
                }

                match res {
                    Ok((fw, text)) => {
                        let buffer = imp.docs.borrow()[idx].buffer.clone();
                        // impostazione del testo fuori dal borrow: il segnale
                        // "changed" viene emesso in modo sincrono
                        imp.applying.set(true);
                        buffer.set_text(&text);
                        buffer.set_modified(false);
                        imp.cursor_line.set(0);
                        {
                            let mut docs = imp.docs.borrow_mut();
                            docs[idx].path = Some(p_doc.clone());
                            docs[idx].dirty = false;
                            *docs[idx].file.borrow_mut() = Some(fw);
                        }
                        imp.applying.set(false);
                        imp.sync_page(idx);
                        imp.sync_status_windowed(idx);
                        imp.sync_title();
                    }
                    Err(msg) => {
                        let msg = format!("Impossibile aprire il file: {msg}");
                        if let Some(page) = imp.page_for(idx) {
                            imp.tab_view.close_page(&page);
                        }
                        obj.toast(&msg);
                    }
                }
                glib::ControlFlow::Break
            });
        }

        /// Fa scorrere la finestra del documento in base alla porzione
        /// visibile. Non scorre se la zona uscente contiene modifiche.
        /// Prima riga visibile nel TextView.
        ///
        /// `visible_rect()` dà coordinate che diventano negative appena si
        /// scorre, quindi non è utilizzabile: si cerca la riga con una ricerca
        /// binaria usando `buffer_to_window_coords`.
        fn first_visible_line(buffer: &gtk4::TextBuffer, view: &gtk4::TextView) -> i32 {
            let n_lines = buffer.end_iter().line() + 1;
            if n_lines <= 0 {
                return 0;
            }
            let y_of = |line: i32| -> i32 {
                match buffer.iter_at_line(line) {
                    Some(iter) => view.buffer_to_window_coords(
                        gtk4::TextWindowType::Widget,
                        iter.line_offset(),
                        0,
                    ).1,
                    None => i32::MIN,
                }
            };

            let mut lo = 0i32;
            let mut hi = n_lines - 1;
            let mut ans = 0i32;
            while lo <= hi {
                let mid = (lo + hi) / 2;
                if y_of(mid) >= 0 {
                    ans = mid;
                    hi = mid - 1;
                } else {
                    lo = mid + 1;
                }
            }
            ans
        }

        /// Fa scorrere la finestra del documento in base alla porzione
        /// visibile. Non scorre se la zona uscente contiene modifiche.
        pub fn maybe_shift(&self, idx: usize) {
            if self.applying.get() || self.shifting.get() {
                return;
            }

            let (view, buffer) = {
                let docs = self.docs.borrow();
                let Some(d) = docs.get(idx) else { return };
                if d.file.borrow().is_none() {
                    return;
                }
                (d.view.clone(), d.buffer.clone())
            };

            let (window_count, dirty) = {
                let docs = self.docs.borrow();
                let cell = &docs[idx].file;
                let fw = cell.borrow();
                let fw = fw.as_ref().unwrap();
                (fw.window_count() as i32, fw.is_dirty())
            };
            let margin = document::MARGIN as i32;
            if window_count <= margin * 2 {
                return;
            }

            let mut anchor = Self::first_visible_line(&buffer, &view);
            // scrollbar completamente in fondo: prosegui oltre la finestra
            if let Some(adj) = view.vadjustment() {
                let upper = adj.upper();
                let page = adj.page_size();
                if upper > page + 1.0 && adj.value() >= upper - page - 1.0 {
                    anchor = window_count - 1;
                }
            }
            anchor = anchor.clamp(0, window_count - 1);
            self.cursor_line.set(anchor);

            if anchor >= margin && anchor <= window_count - margin {
                return;
            }

            let lines = document::buffer_lines(&buffer);
            let result = {
                let docs = self.docs.borrow();
                let mut fw = docs[idx].file.borrow_mut();
                let fw = fw.as_mut().unwrap();
                fw.try_shift_visible(anchor as i64, &lines)
            };
            let Ok(shifted) = result else { return };

            let Some((text, new_start)) = shifted else {
                // scorrimento rifiutato: la zona uscente è modificata
                if dirty {
                    self.status
                        .set_text("Salva (Ctrl+S) per scorrere oltre la finestra");
                }
                return;
            };

            let abs = new_start as i64 + anchor as i64;
            self.shifting.set(true);
            buffer.set_text(&text);
            // il testo nuovo arriva dal disco: non è una modifica
            buffer.set_modified(false);
            self.shifting.set(false);

            let rel = (abs - new_start as i64).clamp(0, i32::MAX as i64) as i32;
            if let Some(mut iter) = buffer.iter_at_line(rel) {
                buffer.place_cursor(&iter);
                view.scroll_to_iter(&mut iter, 0.0, false, 0.0, 0.0);
            }
            self.sync_page(idx);
            self.sync_status_windowed(idx);
        }

        /// Aggiorna titolo e pallino "modificato" della scheda `idx`.
        pub fn sync_page(&self, idx: usize) {
            let (title, dirty) = {
                let docs = self.docs.borrow();
                let Some(doc) = docs.get(idx) else { return };
                (doc.tab_title(), doc.dirty)
            };
            if let Some(page) = self.page_for(idx) {
                page.set_title(&title);
                page.set_needs_attention(dirty);
            }
        }

        /// Barra di stato, con informazioni sulla finestra se il file è grande.
        pub fn sync_status_windowed(&self, idx: usize) {
            // posizione del cursore: il mark "insert" segue il cursore
            let (col, col_off) = {
                let docs = self.docs.borrow();
                let Some(d) = docs.get(idx) else { return };
                let it = d.buffer.iter_at_mark(&d.buffer.get_insert());
                (it.line() + 1, it.line_offset() + 1)
            };

            let (lines, bytes, windowed, start, count) = {
                let docs = self.docs.borrow();
                let Some(d) = docs.get(idx) else { return };
                let size = d
                    .path
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let info = d
                    .file
                    .borrow()
                    .as_ref()
                    .map(|f| {
                        let (a, b) = f.abs_range();
                        (f.total_lines(), f.is_windowed(), a, b - a)
                    })
                    .unwrap_or((0, false, 0, 0));
                (info.0, size, info.1, info.2, info.3)
            };

            self.status.set_text(&if windowed {
                format!(
                    "Ln {col}, Col {col_off} · finestra {}-{} di {lines} · {}",
                    start + 1,
                    start + count,
                    human_size(bytes),
                )
            } else {
                format!(
                    "Ln {col}, Col {col_off} · {lines} righe · {}",
                    human_size(bytes)
                )
            });
        }

        /// Aggiorna solo la posizione del cursore nella barra di stato.
        pub fn sync_status_cursor(&self, idx: usize) {
            self.sync_status_windowed(idx);
        }

        /// Ricalcola se il documento è davvero diverso da disco.
        ///
        /// Serve perché con l'undo si può tornare esattamente al testo
        /// originale: in quel caso il pallino deve sparire. Si fa un
        /// confronto con il contenuto su disco, ma solo qualche volta al
        /// secondo e non a ogni tasto.
        pub fn refresh_dirty(&self, idx: usize) {
            if self.applying.get() || self.shifting.get() {
                return;
            }
            let dirty_now = self.docs.borrow().get(idx).map(|d| d.dirty).unwrap_or(false);
            if !dirty_now {
                return;
            }

            let equals = {
                let docs = self.docs.borrow();
                let Some(d) = docs.get(idx) else { return };
                let cell = &d.file;
                let fw = cell.borrow();
                let Some(fw) = fw.as_ref() else {
                    return;
                };
                let lines = document::buffer_lines(&d.buffer);
                fw.diff_indices(&lines).is_empty()
            };

            if equals {
                let mut docs = self.docs.borrow_mut();
                if let Some(doc) = docs.get_mut(idx) {
                    doc.dirty = false;
                    let mut fw = doc.file.borrow_mut();
                    if let Some(f) = fw.as_mut() {
                        f.mark_saved();
                    }
                }
                drop(docs);
                self.sync_page(idx);
                self.sync_title();
            }
        }

        pub fn sync_status(&self, idx: usize) {
            let (bytes, text) = {
                let docs = self.docs.borrow();
                let Some(doc) = docs.get(idx) else { return };
                let start = doc.buffer.start_iter();
                let end = doc.buffer.end_iter();
                (doc.buffer.char_count(), doc.buffer.text(&start, &end, false).len())
            };
            let _ = text;
            self.status.set_text(&format!(
                "{} caratteri · ~{:.2} MB",
                bytes,
                text as f64 / 1_048_576.0
            ));
        }

        pub fn sync_title(&self) {
            let Some(idx) = self.selected_index() else {
                self.window_title.set_subtitle("");
                return;
            };
            let docs = self.docs.borrow();
            let Some(doc) = docs.get(idx) else { return };
            self.window_title.set_subtitle(&doc.title());
        }

        /// Una scheda è stata rimossa: elimina il documento corrispondente.
        pub fn on_page_detached(&self, pos: i32) {
            let pos = pos as usize;
            let mut docs = self.docs.borrow_mut();
            if pos < docs.len() {
                docs.remove(pos);
            }
        }

        fn on_reordered(&self, new_pos: i32) {
            let new_pos = new_pos as usize;
            let mut docs = self.docs.borrow_mut();
            if new_pos < docs.len() {
                let doc = docs.remove(new_pos);
                docs.insert(new_pos, doc);
            }
        }

        /// Restituisce true se la scheda può chiudersi subito.
        /// Altrimenti mostra il dialog e, al termine, chiama `close_page_finish`.
        pub fn allow_close_page(&self, page: &adw::TabPage) -> bool {
            let obj = self.obj().clone();

            let view = match page
                .child()
                .downcast::<gtk4::ScrolledWindow>()
                .ok()
                .and_then(|s| s.child())
                .and_then(|c| c.downcast::<gtk4::TextView>().ok())
            {
                Some(v) => v,
                None => return true,
            };
            let Some(idx) = self.index_of_view(&view) else {
                return true;
            };
            if !self.docs.borrow()[idx].dirty {
                return true;
            }

            let title = self.docs.borrow()[idx].title();
            let dlg = adw::AlertDialog::builder()
                .heading("Modifiche non salvate")
                .body(format!("\u{ab}\"{title}\" ha modifiche non salvate. Vuoi salvare?"))
                .build();
            dlg.add_responses(&[
                ("cancel", "_Annulla"),
                ("save", "_Salva"),
                ("discard", "_Non salvare"),
            ]);
            dlg.set_default_response(Some("cancel"));
            dlg.set_close_response("cancel");

            let parent = obj.clone();
            let page = page.clone();
            dlg.choose(Some(&parent), gio::Cancellable::NONE, move |resp| {
                    match resp.as_str() {
                    "save" => {
                        let path = obj.imp().docs.borrow()[idx].path.clone();
                        let page2 = page.clone();
                        let owner = obj.clone();
                        let cb: Rc<dyn Fn(bool)> =
                            Rc::new(move |ok| owner.imp().tab_view.close_page_finish(&page2, ok));
                        match path {
                            Some(p) => obj.write_doc(idx, &p, Some(cb)),
                            None => obj.save_as_for(idx, cb),
                        }
                    }
                    "discard" => obj.imp().tab_view.close_page_finish(&page, true),
                    _ => obj.imp().tab_view.close_page_finish(&page, false),
                }
            });

            false
        }

        /// Chiede conferma, salva i documenti modificati, poi chiude la finestra.
        pub fn try_close(&self) {
            if self.saving.get() {
                return;
            }
            let dirty: Vec<usize> = self
                .docs
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, d)| d.dirty)
                .map(|(i, _)| i)
                .collect();

            if dirty.is_empty() {
                self.finish_close();
                return;
            }

            let obj = self.obj().clone();
            let elenco = dirty
                .iter()
                .map(|&i| self.docs.borrow()[i].title())
                .collect::<Vec<_>>()
                .join("\n\u{2022} ");

            let dlg = adw::AlertDialog::builder()
                .heading("Vuoi salvare le modifiche?")
                .body(format!("Modifiche non salvate in:\n\u{2022} {elenco}"))
                .build();
            dlg.add_responses(&[
                ("cancel", "_Annulla"),
                ("discard", "_Non salvare"),
                ("save", "_Salva tutto"),
            ]);
            dlg.set_default_response(Some("cancel"));
            dlg.set_close_response("cancel");

            let parent = obj.clone();
            dlg.choose(Some(&parent), gio::Cancellable::NONE, move |resp| {
                    match resp.as_str() {
                    "save" => {
                        obj.imp().saving.set(true);
                        obj.imp().confirm_close(dirty);
                    }
                    "discard" => obj.imp().finish_close(),
                    _ => {}
                }
            });
        }

        fn confirm_close(&self, dirty: Vec<usize>) {
            let obj = self.obj().clone();

            // documenti senza percorso: prima chiedi "Salva come"
            for &idx in &dirty {
                if self.docs.borrow()[idx].path.is_none() {
                    let o = obj.clone();
                    let cb: Rc<dyn Fn(bool)> = Rc::new(move |ok| {
                        if ok {
                            o.imp().continue_close();
                        } else {
                            o.imp().saving.set(false);
                        }
                    });
                    obj.save_as_for(idx, cb);
                    return;
                }
            }

            let paths: Vec<PathBuf> = dirty
                .iter()
                .filter_map(|&i| self.docs.borrow()[i].path.clone())
                .collect();

            let remaining = Rc::new(Cell::new(paths.len()));
            let failed = Rc::new(Cell::new(false));

            for (i, path) in dirty.iter().zip(paths.iter()) {
                let remaining = remaining.clone();
                let failed = failed.clone();
                let o = obj.clone();
                let cb: Rc<dyn Fn(bool)> = Rc::new(move |ok| {
                    if !ok {
                        failed.set(true);
                    }
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        if failed.get() {
                            o.imp().saving.set(false);
                        } else {
                            o.imp().continue_close();
                        }
                    }
                });
                obj.write_doc(*i, path, Some(cb));
            }
        }

        fn continue_close(&self) {
            self.saving.set(false);
            self.finish_close();
        }

        fn finish_close(&self) {
            self.allow_close.set(true);
            self.obj().close();
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl gtk4::subclass::prelude::ApplicationWindowImpl for Window {}
    impl adw::subclass::prelude::AdwApplicationWindowImpl for Window {}
}
