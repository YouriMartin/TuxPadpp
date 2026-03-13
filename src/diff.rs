// diff.rs – Diff view module
//
// Computes a line-by-line diff between two text versions using the `similar`
// crate and presents the result in a scrollable GTK4 dialog window.

// Public API scaffold – `to_unified_string` and helpers are provided for
// future CLI / export use even though they are not yet called from main.rs.
#![allow(dead_code)]

use similar::{ChangeTag, TextDiff};

// ─── Data types ───────────────────────────────────────────────────────────────

/// The kind of change applied to a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Line is present in both versions (no change).
    Equal,
    /// Line was added in the new version.
    Added,
    /// Line was removed from the old version.
    Removed,
}

/// A single line entry in the diff output.
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// Type of change for this line.
    pub kind: ChangeKind,
    /// Text content of the line (including the trailing newline when present).
    pub content: String,
}

/// A complete diff result between two texts.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Ordered list of line-level changes.
    pub lines: Vec<DiffLine>,
    /// Total number of lines added.
    pub added_count: usize,
    /// Total number of lines removed.
    pub removed_count: usize,
}

impl DiffResult {
    /// Compute a line-by-line diff between `old_text` and `new_text`.
    pub fn compute(old_text: &str, new_text: &str) -> Self {
        let diff = TextDiff::from_lines(old_text, new_text);

        let mut lines = Vec::new();
        let mut added_count = 0usize;
        let mut removed_count = 0usize;

        for change in diff.iter_all_changes() {
            let kind = match change.tag() {
                ChangeTag::Delete => {
                    removed_count += 1;
                    ChangeKind::Removed
                }
                ChangeTag::Insert => {
                    added_count += 1;
                    ChangeKind::Added
                }
                ChangeTag::Equal => ChangeKind::Equal,
            };
            lines.push(DiffLine {
                kind,
                content: change.value().to_string(),
            });
        }

        DiffResult {
            lines,
            added_count,
            removed_count,
        }
    }

    /// Render the diff in unified-diff format (similar to `diff -u`).
    pub fn to_unified_string(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let prefix = match line.kind {
                ChangeKind::Equal => ' ',
                ChangeKind::Added => '+',
                ChangeKind::Removed => '-',
            };
            out.push(prefix);
            out.push_str(&line.content);
            // Ensure the line ends with a newline
            if !line.content.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Return a brief summary string (e.g. `"+3 lines, -1 line"`).
    pub fn summary(&self) -> String {
        format!(
            "+{} line{}, -{} line{}",
            self.added_count,
            if self.added_count == 1 { "" } else { "s" },
            self.removed_count,
            if self.removed_count == 1 { "" } else { "s" },
        )
    }
}

// ─── GTK4 diff dialog ────────────────────────────────────────────────────────

/// Show a modal diff dialog comparing `old_text` and `new_text`.
///
/// Added lines are rendered in green, removed lines in red, unchanged lines
/// in the default foreground colour.
///
/// # Parameters
/// * `parent`   – the parent window (the dialog will be transient for it).
/// * `old_text` – original text (left / "before" side).
/// * `new_text` – modified text (right / "after" side).
pub fn show_diff_dialog(
    parent: &impl gtk4::prelude::IsA<gtk4::Window>,
    old_text: &str,
    new_text: &str,
) {
    use gtk4::prelude::*;

    let result = DiffResult::compute(old_text, new_text);

    // Window -------------------------------------------------------------------
    let dialog = gtk4::Window::builder()
        .title(format!("Diff – {}", result.summary()))
        .default_width(750)
        .default_height(540)
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .build();

    // Layout -------------------------------------------------------------------
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // Summary label
    let summary_label = gtk4::Label::new(Some(&result.summary()));
    summary_label.set_margin_top(8);
    summary_label.set_margin_bottom(8);
    vbox.append(&summary_label);

    // Diff text view
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);

    let text_view = gtk4::TextView::new();
    text_view.set_editable(false);
    text_view.set_monospace(true);
    text_view.set_left_margin(8);
    text_view.set_top_margin(4);

    // Create colour tags -------------------------------------------------------
    let buffer = text_view.buffer();
    let tag_table = buffer.tag_table();

    let added_tag = gtk4::TextTag::new(Some("added"));
    added_tag.set_foreground(Some("#2ecc71")); // green
    tag_table.add(&added_tag);

    let removed_tag = gtk4::TextTag::new(Some("removed"));
    removed_tag.set_foreground(Some("#e74c3c")); // red
    tag_table.add(&removed_tag);

    // Fill the buffer ----------------------------------------------------------
    for line in &result.lines {
        let prefix = match line.kind {
            ChangeKind::Equal => ' ',
            ChangeKind::Added => '+',
            ChangeKind::Removed => '-',
        };
        let text = format!("{}{}", prefix, line.content);
        // Ensure trailing newline
        let text = if text.ends_with('\n') {
            text
        } else {
            format!("{}\n", text)
        };

        // Track start position before insertion
        let start_offset = buffer.char_count();
        let mut end_iter = buffer.end_iter();
        buffer.insert(&mut end_iter, &text);

        // Apply colour tag if needed
        let tag_name = match line.kind {
            ChangeKind::Added => Some("added"),
            ChangeKind::Removed => Some("removed"),
            ChangeKind::Equal => None,
        };
        if let Some(tag_name) = tag_name {
            let start_iter = buffer.iter_at_offset(start_offset);
            let end_iter = buffer.end_iter();
            buffer.apply_tag_by_name(tag_name, &start_iter, &end_iter);
        }
    }

    scrolled.set_child(Some(&text_view));
    vbox.append(&scrolled);

    // Close button -------------------------------------------------------------
    let close_button = gtk4::Button::with_label("Close");
    close_button.set_margin_top(8);
    close_button.set_margin_bottom(8);
    close_button.set_margin_start(8);
    close_button.set_margin_end(8);
    let dialog_clone = dialog.clone();
    close_button.connect_clicked(move |_| dialog_clone.close());
    vbox.append(&close_button);

    dialog.set_child(Some(&vbox));
    dialog.present();
}
