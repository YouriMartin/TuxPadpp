// editor.rs – EditorView module
//
// Wraps a GtkSourceView5 widget with syntax highlighting and line numbers,
// backed by a `ropey::Rope` for efficient large-file buffer management.
// Also provides regex-based in-editor search and a placeholder structure
// for grep-searcher-based file-search integration.

// Public API surface – some methods are not yet called from main.rs but are
// intentional scaffolding for future features.
#![allow(dead_code)]

use gtk4::prelude::*;
use sourceview5::prelude::*;

// ─── EditorView ─────────────────────────────────────────────────────────────

/// A single editor pane containing a GtkSourceView and a rope text buffer.
pub struct EditorView {
    /// Scrolled container – the root widget to embed in a Notebook tab.
    scrolled_window: gtk4::ScrolledWindow,
    /// The underlying source-code editor widget.
    view: sourceview5::View,
    /// Direct reference to the sourceview buffer (avoids repeated downcasts).
    buffer: sourceview5::Buffer,
    /// Rope buffer for O(log n) insertion/deletion on very large files.
    rope: ropey::Rope,
    /// Path of the file currently open in this tab (None for unsaved buffers).
    pub file_path: Option<std::path::PathBuf>,
}

impl EditorView {
    /// Create a new, empty editor view with default settings.
    pub fn new() -> Self {
        // Buffer ----------------------------------------------------------------
        let buffer = sourceview5::Buffer::new(None::<&gtk4::TextTagTable>);
        buffer.set_highlight_syntax(true);
        buffer.set_highlight_matching_brackets(true);

        // Apply Adwaita-dark style scheme when available
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        for scheme_id in &["Adwaita-dark", "oblivion", "classic"] {
            if let Some(scheme) = scheme_manager.scheme(scheme_id) {
                buffer.set_style_scheme(Some(&scheme));
                break;
            }
        }

        // View ------------------------------------------------------------------
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(true);
        view.set_auto_indent(true);
        view.set_tab_width(4);
        view.set_insert_spaces_instead_of_tabs(true);
        view.set_smart_backspace(true);
        view.set_monospace(true);

        // Wrap in a ScrolledWindow so the tab can scroll
        let scrolled_window = gtk4::ScrolledWindow::new();
        scrolled_window.set_child(Some(&view));
        scrolled_window.set_hexpand(true);
        scrolled_window.set_vexpand(true);

        Self {
            scrolled_window,
            view,
            buffer,
            rope: ropey::Rope::new(),
            file_path: None,
        }
    }

    // ── Widget accessors ──────────────────────────────────────────────────────

    /// Returns the root widget (a `ScrolledWindow`) for embedding in a tab.
    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scrolled_window
    }

    /// Returns a reference to the underlying GtkSourceView.
    pub fn view(&self) -> &sourceview5::View {
        &self.view
    }

    /// Returns a reference to the underlying GtkSourceBuffer.
    pub fn buffer(&self) -> &sourceview5::Buffer {
        &self.buffer
    }

    // ── Text content ──────────────────────────────────────────────────────────

    /// Replace the entire editor content and re-sync the rope buffer.
    pub fn set_text(&mut self, text: &str) {
        self.rope = ropey::Rope::from_str(text);
        self.buffer.set_text(text);
    }

    /// Return the current full text from the GTK buffer.
    pub fn get_text(&self) -> String {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        self.buffer.text(&start, &end, true).to_string()
    }

    /// Insert `text` at the current cursor position (updates the rope as well).
    pub fn insert_at_cursor(&mut self, text: &str) {
        let mut cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let offset = cursor.offset() as usize;
        self.rope.insert(offset, text);
        self.buffer.insert(&mut cursor, text);
    }

    // ── Syntax highlighting ───────────────────────────────────────────────────

    /// Explicitly set the syntax-highlighting language by its GtkSourceView ID
    /// (e.g. `"rust"`, `"python"`, `"javascript"`).
    pub fn set_language(&self, language_id: &str) {
        let lm = sourceview5::LanguageManager::default();
        if let Some(lang) = lm.language(language_id) {
            self.buffer.set_language(Some(&lang));
        }
    }

    /// Guess and apply the language based on the file name.
    pub fn detect_language_from_file(&self, file_name: &str) {
        let lm = sourceview5::LanguageManager::default();
        if let Some(lang) = lm.guess_language(Some(file_name), None) {
            self.buffer.set_language(Some(&lang));
        }
    }

    // ── File I/O ──────────────────────────────────────────────────────────────

    /// Open `path`, load its contents into the editor, and detect language.
    pub fn open_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;

        self.file_path = Some(path.to_path_buf());

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            self.detect_language_from_file(name);
        }

        self.set_text(&content);
        Ok(())
    }

    /// Save the editor content to the file it was opened from.
    /// Returns `Err` if no file path is associated; use `save_to_path` instead.
    pub fn save_file(&mut self) -> Result<(), String> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| "No file path – use Save As.".to_string())?;
        self.save_to_path(&path)
    }

    /// Save the editor content to `path` and update the associated file path.
    pub fn save_to_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = self.get_text();
        // Keep the rope in sync with the saved state
        self.rope = ropey::Rope::from_str(&text);
        std::fs::write(path, text.as_bytes())
            .map_err(|e| format!("Failed to write '{}': {}", path.display(), e))?;
        self.file_path = Some(path.to_path_buf());
        Ok(())
    }

    // ── Search (regex) ────────────────────────────────────────────────────────

    /// Search for `pattern` (a regex) in the editor content.
    ///
    /// Returns a `Vec` of `(byte_start, byte_end)` positions of each match.
    /// Uses `regex::Regex` for in-buffer search.
    pub fn search(&self, pattern: &str) -> Result<Vec<(usize, usize)>, String> {
        let re = regex::Regex::new(pattern)
            .map_err(|e| format!("Invalid regex '{}': {}", pattern, e))?;
        let text = self.get_text();
        let matches = re.find_iter(&text).map(|m| (m.start(), m.end())).collect();
        Ok(matches)
    }

    /// Highlight all occurrences of `pattern` in the editor using a GTK text tag.
    /// The tag name is `"search-highlight"` and is created (or reused) on demand.
    pub fn highlight_search(&self, pattern: &str) -> Result<usize, String> {
        // Remove any previous highlights
        self.clear_search_highlights();

        let matches = self.search(pattern)?;
        let count = matches.len();

        if count == 0 {
            return Ok(0);
        }

        // Ensure the highlight tag exists
        let tag_table = self.buffer.tag_table();
        if tag_table.lookup("search-highlight").is_none() {
            let tag = gtk4::TextTag::new(Some("search-highlight"));
            tag.set_background(Some("#f39c12"));
            tag.set_foreground(Some("#000000"));
            tag_table.add(&tag);
        }

        let text = self.get_text();
        for (start_byte, end_byte) in &matches {
            // Convert byte offsets to character offsets for GTK iterators
            let start_char = text[..*start_byte].chars().count() as i32;
            let end_char = text[..*end_byte].chars().count() as i32;
            let start_iter = self.buffer.iter_at_offset(start_char);
            let end_iter = self.buffer.iter_at_offset(end_char);
            self.buffer
                .apply_tag_by_name("search-highlight", &start_iter, &end_iter);
        }

        Ok(count)
    }

    /// Remove all search-highlight tags from the buffer.
    pub fn clear_search_highlights(&self) {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        self.buffer
            .remove_tag_by_name("search-highlight", &start, &end);
    }
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

// ─── GrepSearchProvider ──────────────────────────────────────────────────────
//
// Placeholder structure for integrating `grep-searcher` (from the ripgrep
// project) for high-performance multi-file search.  A full implementation
// would also require the `grep-regex` crate as a `grep-matcher` backend.

/// A search result produced by the grep-based file searcher.
pub struct GrepMatch {
    /// 1-based line number of the match.
    pub line_number: u64,
    /// The full text of the matching line.
    pub line: String,
    /// Byte range of the match within the line.
    pub match_range: std::ops::Range<usize>,
}

/// Provides file-system-level search backed by `grep-searcher`.
///
/// # Planned usage
/// ```no_run
/// let provider = GrepSearchProvider::new();
/// let hits = provider.search_file("fn main", std::path::Path::new("src/main.rs")).unwrap();
/// for hit in hits { println!("{}:{}", hit.line_number, hit.line); }
/// ```
pub struct GrepSearchProvider {
    /// When `true` the pattern is matched case-sensitively.
    pub case_sensitive: bool,
    /// When `true` the pattern is interpreted as a regular expression.
    pub use_regex: bool,
}

impl GrepSearchProvider {
    /// Create a new provider with defaults (case-insensitive regex search).
    pub fn new() -> Self {
        Self {
            case_sensitive: false,
            use_regex: true,
        }
    }

    /// Search `pattern` in a single file.
    ///
    /// Returns `Ok(Vec<GrepMatch>)` on success or an error message.
    ///
    /// NOTE: This is a structural placeholder.  A production implementation
    /// would use `grep_searcher::SearcherBuilder` together with a
    /// `grep_regex::RegexMatcher` and a custom `grep_searcher::Sink`.
    pub fn search_file(
        &self,
        pattern: &str,
        file_path: &std::path::Path,
    ) -> Result<Vec<GrepMatch>, String> {
        // Use the `regex` crate directly while the grep-searcher integration
        // is being finalized.
        let re = {
            let pat = if self.case_sensitive {
                pattern.to_owned()
            } else {
                format!("(?i){}", pattern)
            };
            regex::Regex::new(&pat)
                .map_err(|e| format!("Invalid pattern '{}': {}", pattern, e))?
        };

        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Cannot read '{}': {}", file_path.display(), e))?;

        let mut results = Vec::new();
        for (line_number, line) in content.lines().enumerate() {
            if let Some(m) = re.find(line) {
                results.push(GrepMatch {
                    line_number: (line_number + 1) as u64,
                    line: line.to_owned(),
                    match_range: m.start()..m.end(),
                });
            }
        }
        Ok(results)
    }

    /// Search `pattern` across multiple files (e.g., an entire project directory).
    pub fn search_files(
        &self,
        pattern: &str,
        paths: &[&std::path::Path],
    ) -> Vec<(std::path::PathBuf, Vec<GrepMatch>)> {
        paths
            .iter()
            .filter_map(|p| {
                self.search_file(pattern, p)
                    .ok()
                    .map(|hits| (p.to_path_buf(), hits))
            })
            .filter(|(_, hits)| !hits.is_empty())
            .collect()
    }
}

impl Default for GrepSearchProvider {
    fn default() -> Self {
        Self::new()
    }
}
