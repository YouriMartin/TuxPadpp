// main.rs – TuxPad++ entry point
//
// Creates the Adwaita application, builds the main window with:
//   • an Adwaita HeaderBar with a hamburger menu,
//   • a quick-search bar (activated with Ctrl+F),
//   • a GtkNotebook for multiple editor tabs,
//   • a status bar showing cursor position and character count.
//
// File, Edit and Tools actions wire together the editor, formatter, and diff
// modules defined in the companion source files.

mod diff;
mod editor;
mod formatter;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use sourceview5::prelude::*;

const APP_ID: &str = "com.tuxpad.TuxPad";

// ─── Application entry point ─────────────────────────────────────────────────

fn main() -> glib::ExitCode {
    // Initialise GtkSourceView type system before any GTK objects are created
    sourceview5::init();

    let app = adw::Application::new(Some(APP_ID), gio::ApplicationFlags::empty());
    app.connect_activate(build_ui);
    app.run()
}

// ─── UI builder ──────────────────────────────────────────────────────────────

fn build_ui(app: &adw::Application) {
    let window = build_main_window(app);
    window.present();
}

fn build_main_window(app: &adw::Application) -> adw::ApplicationWindow {
    // ── Window ----------------------------------------------------------------
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("TuxPad++")
        .default_width(1100)
        .default_height(780)
        .build();

    // ── Root layout -----------------------------------------------------------
    let root_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // ── Header bar ------------------------------------------------------------
    let header_bar = adw::HeaderBar::new();

    // "New tab" button in the header
    let new_tab_button = gtk4::Button::new();
    new_tab_button.set_icon_name("tab-new-symbolic");
    new_tab_button.set_tooltip_text(Some("New Tab (Ctrl+T)"));
    header_bar.pack_start(&new_tab_button);

    // Hamburger menu
    let menu_button = gtk4::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    let menu_model = build_app_menu();
    menu_button.set_menu_model(Some(&menu_model));
    header_bar.pack_end(&menu_button);

    root_box.append(&header_bar);

    // ── Search bar (shown with Ctrl+F) ----------------------------------------
    let search_bar = gtk4::SearchBar::new();
    search_bar.set_show_close_button(true);

    let search_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    search_row.set_margin_start(8);
    search_row.set_margin_end(8);

    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some("Search… (regex supported)"));

    let prev_button = gtk4::Button::new();
    prev_button.set_icon_name("go-up-symbolic");
    prev_button.set_tooltip_text(Some("Previous match"));

    let next_button = gtk4::Button::new();
    next_button.set_icon_name("go-down-symbolic");
    next_button.set_tooltip_text(Some("Next match"));

    let match_label = gtk4::Label::new(Some(""));

    search_row.append(&search_entry);
    search_row.append(&prev_button);
    search_row.append(&next_button);
    search_row.append(&match_label);
    search_bar.set_child(Some(&search_row));
    // Forward key strokes to the search bar so typing opens it automatically
    search_bar.connect_entry(&search_entry);

    root_box.append(&search_bar);

    // ── Notebook (tabs) -------------------------------------------------------
    let notebook = gtk4::Notebook::new();
    notebook.set_vexpand(true);
    notebook.set_tab_pos(gtk4::PositionType::Top);
    notebook.set_scrollable(true);
    notebook.set_show_border(false);

    // Open with one blank tab
    add_new_tab(&notebook, None);

    root_box.append(&notebook);

    // ── Status bar ------------------------------------------------------------
    let status_bar = gtk4::Label::new(Some("Ln 1, Col 1"));
    status_bar.set_xalign(0.0);
    status_bar.set_margin_start(8);
    status_bar.set_margin_end(8);
    status_bar.set_margin_top(3);
    status_bar.set_margin_bottom(3);

    let status_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    status_box.add_css_class("statusbar");
    status_box.append(&status_bar);
    root_box.append(&status_box);

    // ── Assemble window -------------------------------------------------------
    window.set_content(Some(&root_box));

    // ── Keyboard shortcuts ----------------------------------------------------
    setup_shortcuts(&window, &notebook, &search_bar);

    // ── Actions ---------------------------------------------------------------
    setup_actions(&window, &notebook, &search_entry, &match_label);

    // Connect new-tab button to win.new_tab action
    new_tab_button.connect_clicked({
        let window = window.clone();
        move |_| {
            if let Some(action) = window.lookup_action("new_tab") {
                action.activate(None);
            }
        }
    });

    // Update status bar when cursor moves in the active editor
    {
        let status_bar_clone = status_bar.clone();
        let notebook_clone = notebook.clone();
        notebook.connect_switch_page(move |nb, _, page_num| {
            connect_cursor_moved(nb, page_num, &status_bar_clone);
            let _ = (notebook_clone.clone(), page_num); // suppress unused warning
        });
    }

    window
}

// ─── Menu model ──────────────────────────────────────────────────────────────

fn build_app_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(Some("New Tab"), Some("win.new_tab"));
    file_section.append(Some("Open File…"), Some("win.open_file"));
    file_section.append(Some("Save"), Some("win.save_file"));
    file_section.append(Some("Save As…"), Some("win.save_file_as"));
    menu.append_section(Some("File"), &file_section);

    let tools_section = gio::Menu::new();
    tools_section.append(Some("Format Code (Beautify)"), Some("win.format_code"));
    tools_section.append(Some("Show Diff…"), Some("win.show_diff"));
    menu.append_section(Some("Tools"), &tools_section);

    let app_section = gio::Menu::new();
    app_section.append(Some("About TuxPad++"), Some("app.about"));
    menu.append_section(None, &app_section);

    menu
}

// ─── Tab helpers ─────────────────────────────────────────────────────────────

/// Add a new editor tab to `notebook`.
///
/// * `file_path` – if `Some`, the file is loaded and the tab is labelled with
///   the file name; otherwise the tab is labelled `"Untitled"`.
///
/// Returns the `EditorView` so the caller can perform further operations.
fn add_new_tab(
    notebook: &gtk4::Notebook,
    file_path: Option<&std::path::Path>,
) -> editor::EditorView {
    let mut editor_view = editor::EditorView::new();

    let label_text = if let Some(path) = file_path {
        match editor_view.open_file(path) {
            Ok(()) => {
                // Persist the path as GObject data so actions (Save, Format…) can
                // retrieve it without needing to carry the EditorView separately.
                let stored_path: std::path::PathBuf = path.to_path_buf();
                unsafe {
                    editor_view.view().set_data("file_path", stored_path);
                }
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_owned()
            }
            Err(err) => {
                eprintln!("Error opening file: {}", err);
                "Untitled".to_owned()
            }
        }
    } else {
        "Untitled".to_owned()
    };

    // Tab label with a close button
    let tab_label = build_tab_label(&label_text, notebook, editor_view.widget());

    let page_index = notebook.append_page(editor_view.widget(), Some(&tab_label));
    notebook.set_tab_reorderable(editor_view.widget(), true);
    notebook.set_current_page(Some(page_index));

    editor_view
}

/// Build a tab label widget: `[filename] [✕]`
fn build_tab_label(
    text: &str,
    notebook: &gtk4::Notebook,
    page_widget: &gtk4::ScrolledWindow,
) -> gtk4::Box {
    let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);

    let label = gtk4::Label::new(Some(text));
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(20);

    let close_btn = gtk4::Button::new();
    close_btn.set_icon_name("window-close-symbolic");
    close_btn.set_has_frame(false);
    close_btn.add_css_class("flat");
    close_btn.set_tooltip_text(Some("Close tab"));

    {
        let nb = notebook.clone();
        let pw = page_widget.clone();
        close_btn.connect_clicked(move |_| {
            if let Some(index) = nb.page_num(&pw) {
                nb.remove_page(Some(index));
            }
        });
    }

    tab_box.append(&label);
    tab_box.append(&close_btn);
    tab_box
}

/// Returns the `sourceview5::View` of the currently active tab, if any.
fn current_source_view(notebook: &gtk4::Notebook) -> Option<sourceview5::View> {
    use sourceview5::prelude::*;

    let page_index = notebook.current_page()?;
    let scrolled = notebook
        .nth_page(Some(page_index))?
        .downcast::<gtk4::ScrolledWindow>()
        .ok()?;
    scrolled
        .child()?
        .downcast::<sourceview5::View>()
        .ok()
}

// ─── Status bar helper ────────────────────────────────────────────────────────

/// Connect a cursor-moved signal on the active tab's buffer to `status_bar`.
fn connect_cursor_moved(
    notebook: &gtk4::Notebook,
    page_num: u32,
    status_bar: &gtk4::Label,
) {
    if let Some(view) = {
        use sourceview5::prelude::*;
        notebook
            .nth_page(Some(page_num))
            .and_then(|w| w.downcast::<gtk4::ScrolledWindow>().ok())
            .and_then(|sw| sw.child())
            .and_then(|c| c.downcast::<sourceview5::View>().ok())
    } {
        let status_clone = status_bar.clone();
        view.buffer().connect_mark_set(move |buf, iter, mark| {
            if mark.name().as_deref() == Some("insert") {
                let line = iter.line() + 1;
                let col = buf
                    .iter_at_line_offset(iter.line(), 0)
                    .map(|start| iter.offset() - start.offset() + 1)
                    .unwrap_or(1);
                status_clone.set_text(&format!("Ln {}, Col {}", line, col));
            }
        });
    }
}

// ─── Keyboard shortcuts ───────────────────────────────────────────────────────

fn setup_shortcuts(
    window: &adw::ApplicationWindow,
    notebook: &gtk4::Notebook,
    search_bar: &gtk4::SearchBar,
) {
    // Ctrl+F – toggle search bar
    {
        let sb = search_bar.clone();
        let ctrl_f = gtk4::ShortcutController::new();
        let trigger = gtk4::KeyvalTrigger::new(
            gtk4::gdk::Key::f,
            gtk4::gdk::ModifierType::CONTROL_MASK,
        );
        let action = gtk4::CallbackAction::new(move |_, _| {
            let active = !sb.is_search_mode();
            sb.set_search_mode(active);
            glib::Propagation::Stop
        });
        ctrl_f.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
        window.add_controller(ctrl_f);
    }

    // Ctrl+T – new tab
    {
        let nb = notebook.clone();
        let ctrl_t = gtk4::ShortcutController::new();
        let trigger = gtk4::KeyvalTrigger::new(
            gtk4::gdk::Key::t,
            gtk4::gdk::ModifierType::CONTROL_MASK,
        );
        let action = gtk4::CallbackAction::new(move |_, _| {
            add_new_tab(&nb, None);
            glib::Propagation::Stop
        });
        ctrl_t.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
        window.add_controller(ctrl_t);
    }

    // Ctrl+S – save current file
    {
        let nb = notebook.clone();
        let ctrl_s = gtk4::ShortcutController::new();
        let trigger = gtk4::KeyvalTrigger::new(
            gtk4::gdk::Key::s,
            gtk4::gdk::ModifierType::CONTROL_MASK,
        );
        let action = gtk4::CallbackAction::new(move |_, _| {
            if let Some(view) = current_source_view(&nb) {
                // Retrieve file path stored as object data
                if let Some(path_ptr) =
                    unsafe { view.data::<std::path::PathBuf>("file_path") }
                {
                    let path = unsafe { path_ptr.as_ref().clone() };
                    let buf = view.buffer();
                    let start = buf.start_iter();
                    let end = buf.end_iter();
                    let text = buf.text(&start, &end, true);
                    if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                        eprintln!("Save error: {}", e);
                    }
                }
            }
            glib::Propagation::Stop
        });
        ctrl_s.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
        window.add_controller(ctrl_s);
    }
}

// ─── Actions ─────────────────────────────────────────────────────────────────

fn setup_actions(
    window: &adw::ApplicationWindow,
    notebook: &gtk4::Notebook,
    search_entry: &gtk4::SearchEntry,
    match_label: &gtk4::Label,
) {
    // win.new_tab
    {
        let nb = notebook.clone();
        let action = gio::SimpleAction::new("new_tab", None);
        action.connect_activate(move |_, _| {
            add_new_tab(&nb, None);
        });
        window.add_action(&action);
    }

    // win.open_file
    {
        let nb = notebook.clone();
        let win = window.clone();
        let action = gio::SimpleAction::new("open_file", None);
        action.connect_activate(move |_, _| {
            let dialog = gtk4::FileDialog::builder()
                .title("Open File")
                .modal(true)
                .build();
            let nb_clone = nb.clone();
            dialog.open(
                Some(&win),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            add_new_tab(&nb_clone, Some(&path));
                        }
                    }
                },
            );
        });
        window.add_action(&action);
    }

    // win.save_file
    {
        let nb = notebook.clone();
        let action = gio::SimpleAction::new("save_file", None);
        action.connect_activate(move |_, _| {
            if let Some(view) = current_source_view(&nb) {
                if let Some(path_ptr) =
                    unsafe { view.data::<std::path::PathBuf>("file_path") }
                {
                    let path = unsafe { path_ptr.as_ref().clone() };
                    let buf = view.buffer();
                    let start = buf.start_iter();
                    let end = buf.end_iter();
                    let text = buf.text(&start, &end, true);
                    if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                        eprintln!("Save error: {}", e);
                    }
                }
            }
        });
        window.add_action(&action);
    }

    // win.save_file_as
    {
        let nb = notebook.clone();
        let win = window.clone();
        let action = gio::SimpleAction::new("save_file_as", None);
        action.connect_activate(move |_, _| {
            if let Some(view) = current_source_view(&nb) {
                let dialog = gtk4::FileDialog::builder()
                    .title("Save As")
                    .modal(true)
                    .build();
                let view_clone = view.clone();
                dialog.save(
                    Some(&win),
                    None::<&gio::Cancellable>,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let buf = view_clone.buffer();
                                let start = buf.start_iter();
                                let end = buf.end_iter();
                                let text = buf.text(&start, &end, true);
                                if std::fs::write(&path, text.as_bytes()).is_ok() {
                                    // Update the stored path so future Saves work
                                    unsafe {
                                        view_clone.set_data("file_path", path.clone());
                                    }
                                } else {
                                    eprintln!("Save As error: could not write to {}", path.display());
                                }
                            }
                        }
                    },
                );
            }
        });
        window.add_action(&action);
    }

    // win.format_code – run the appropriate external formatter
    {
        let nb = notebook.clone();
        let action = gio::SimpleAction::new("format_code", None);
        action.connect_activate(move |_, _| {
            let Some(view) = current_source_view(&nb) else {
                return;
            };

            // Determine language from the sourceview buffer
            let language_id: Option<String> = view
                .buffer()
                .downcast::<sourceview5::Buffer>()
                .ok()
                .and_then(|b| b.language())
                .map(|l: sourceview5::Language| l.id().to_string());

            let Some(lang_id) = language_id else {
                eprintln!("Format: cannot determine language for current tab");
                return;
            };

            let Some(fmt) = formatter::Formatter::for_language(&lang_id) else {
                eprintln!("Format: no formatter available for '{}'", lang_id);
                return;
            };

            // Retrieve the file path stored as object data on the view
            if let Some(path_ptr) =
                unsafe { view.data::<std::path::PathBuf>("file_path") }
            {
                let path = unsafe { path_ptr.as_ref().clone() };
                // Save first so the formatter can read the latest content
                let buf = view.buffer();
                let start = buf.start_iter();
                let end = buf.end_iter();
                let text = buf.text(&start, &end, true);
                if std::fs::write(&path, text.as_bytes()).is_ok() {
                    match fmt.format_file(&path) {
                        Ok(()) => {
                            // Reload formatted content into the buffer
                            if let Ok(formatted) = std::fs::read_to_string(&path) {
                                buf.set_text(&formatted);
                            }
                        }
                        Err(e) => eprintln!("Format error: {}", e),
                    }
                }
            }
        });
        window.add_action(&action);
    }

    // win.show_diff – compare saved version with current editor content
    {
        let nb = notebook.clone();
        let win = window.clone();
        let action = gio::SimpleAction::new("show_diff", None);
        action.connect_activate(move |_, _| {
            let Some(view) = current_source_view(&nb) else {
                return;
            };

            let buf = view.buffer();
            let start = buf.start_iter();
            let end = buf.end_iter();
            let current_text = buf.text(&start, &end, true).to_string();

            // Compare against the on-disk version when a file is open
            let saved_text = unsafe { view.data::<std::path::PathBuf>("file_path") }
                .map(|ptr| unsafe { ptr.as_ref().clone() })
                .and_then(|path| std::fs::read_to_string(path).ok())
                .unwrap_or_default();

            diff::show_diff_dialog(&win, &saved_text, &current_text);
        });
        window.add_action(&action);
    }

    // Search entry: activate on Enter
    {
        let nb = notebook.clone();
        let ml = match_label.clone();
        let se = search_entry.clone();
        search_entry.connect_activate(move |_| {
            let pattern = se.text().to_string();
            if pattern.is_empty() {
                return;
            }
            if let Some(view) = current_source_view(&nb) {
                let buf = view.buffer();
                let start = buf.start_iter();
                let end = buf.end_iter();
                let text = buf.text(&start, &end, true).to_string();

                match regex::Regex::new(&pattern) {
                    Ok(re) => {
                        let count = re.find_iter(&text).count();
                        ml.set_text(&format!("{} match{}", count, if count == 1 { "" } else { "es" }));
                    }
                    Err(_) => ml.set_text("invalid regex"),
                }
            }
        });
    }

    // Search entry: live highlight as user types
    {
        let nb = notebook.clone();
        search_entry.connect_search_changed(move |entry| {
            let pattern = entry.text().to_string();
            if let Some(view) = current_source_view(&nb) {
                let buf = view
                    .buffer()
                    .downcast::<sourceview5::Buffer>()
                    .ok();
                if let Some(buf) = buf {
                    // Remove previous tag
                    let start = buf.start_iter();
                    let end = buf.end_iter();
                    buf.remove_tag_by_name("search-highlight", &start, &end);

                    if pattern.is_empty() {
                        return;
                    }

                    // Ensure tag exists
                    let tag_table = buf.tag_table();
                    if tag_table.lookup("search-highlight").is_none() {
                        let tag = gtk4::TextTag::new(Some("search-highlight"));
                        tag.set_background(Some("#f39c12"));
                        tag.set_foreground(Some("#000000"));
                        tag_table.add(&tag);
                    }

                    if let Ok(re) = regex::Regex::new(&pattern) {
                        let text = buf
                            .text(&buf.start_iter(), &buf.end_iter(), true)
                            .to_string();
                        for m in re.find_iter(&text) {
                            let sc = text[..m.start()].chars().count() as i32;
                            let ec = text[..m.end()].chars().count() as i32;
                            let si = buf.iter_at_offset(sc);
                            let ei = buf.iter_at_offset(ec);
                            buf.apply_tag_by_name("search-highlight", &si, &ei);
                        }
                    }
                }
            }
        });
    }
}
