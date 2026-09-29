use adw::prelude::*;

pub mod document;
mod window;
use window::Window;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "Simon-Note {VERSION} — blocco note a schede per GNOME\n\n\
         USO:\n    simonnote [FILE]...\n\n\
         OPZIONI:\n\
         \x20   -h, --help       Mostra questo messaggio\n\
         \x20   -V, --version    Mostra la versione\n\n\
         SCORCIATOIE:\n\
         \x20   Ctrl+Z           Annulla\n\
         \x20   Ctrl+Shift+Z     Ripeti\n\
         \x20   Ctrl+T           Nuova scheda\n\
         \x20   Ctrl+N           Nuova scheda\n\
         \x20   Ctrl+O           Apri file\n\
         \x20   Ctrl+S           Salva\n\
         \x20   Ctrl+Shift+S     Salva come\n\
         \x20   Ctrl+W           Chiudi scheda\n\
         \x20   Ctrl+Page        Scheda successiva/precedente\n\
         \x20   Ctrl+Q           Esci"
    );
}

/// Restituisce la prima finestra dell'app, creandola se non esiste.
fn main_window(app: &adw::Application) -> Window {
    match app.windows().first() {
        Some(w) => w
            .downcast_ref::<Window>()
            .expect("finestra non valida")
            .clone(),
        None => Window::new(app),
    }
}

fn main() {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("simonnote {VERSION}");
                return;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    let application = adw::Application::builder()
        .application_id("com.simonecompany.simonnote")
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    application.connect_startup(|app| {
        let display = gdk4::Display::default().expect("Nessun display disponibile");
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(include_str!("style.css"));
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let quit = gio::SimpleAction::new("quit", None);
        let a = app.clone();
        quit.connect_activate(move |_, _| a.quit());
        app.add_action(&quit);

        app.set_accels_for_action("app.quit", &["<primary>q"]);
        app.set_accels_for_action("win.new", &["<primary>t", "<primary>n"]);
        app.set_accels_for_action("win.open", &["<primary>o"]);
        app.set_accels_for_action("win.save", &["<primary>s"]);
        app.set_accels_for_action("win.save-as", &["<primary><shift>s"]);
        app.set_accels_for_action("win.undo", &["<primary>z", "<primary>Z"]);
        app.set_accels_for_action("win.redo", &["<primary>y", "<primary><shift>z"]);
        app.set_accels_for_action("win.close-tab", &["<primary>w"]);
        app.set_accels_for_action("win.close-others", &["<primary><shift>w"]);
    });

    application.connect_activate(|app| {
        let window = main_window(app);
        window.present();
    });

    application.connect_open(|app, files, _| {
        let window = main_window(app);
        for file in files {
            if let Some(path) = file.path() {
                window.open_path(&path);
            }
        }
        window.present();
    });

    application.run();
}
