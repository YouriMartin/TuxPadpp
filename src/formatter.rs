// formatter.rs – Code formatter module
//
// Calls external formatter tools (rustfmt, prettier, black, …) on a file
// path so that TuxPad++ can offer a "Beautify" command for any language.

// Public API scaffold – not all methods are wired up in main.rs yet.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

// ─── Formatter ───────────────────────────────────────────────────────────────

/// Represents an external code-formatting tool that is invoked as a subprocess.
///
/// # Example
/// ```no_run
/// use std::path::Path;
///
/// let fmt = tuxpad::formatter::Formatter::rustfmt();
/// fmt.format_file(Path::new("src/main.rs")).expect("rustfmt failed");
/// ```
pub struct Formatter {
    /// The executable name (or absolute path) of the formatter.
    pub command: String,
    /// Additional arguments passed *before* the file path.
    pub args: Vec<String>,
}

impl Formatter {
    // ── Built-in presets ──────────────────────────────────────────────────────

    /// Formatter preset for Rust (`rustfmt`).
    pub fn rustfmt() -> Self {
        Self {
            command: "rustfmt".to_string(),
            args: vec![],
        }
    }

    /// Formatter preset for JavaScript / TypeScript / JSON / CSS / HTML
    /// (`prettier --write`).
    pub fn prettier() -> Self {
        Self {
            command: "prettier".to_string(),
            args: vec!["--write".to_string()],
        }
    }

    /// Formatter preset for Python (`black`).
    pub fn black() -> Self {
        Self {
            command: "black".to_string(),
            args: vec![],
        }
    }

    /// Formatter preset for C / C++ (`clang-format -i`).
    pub fn clang_format() -> Self {
        Self {
            command: "clang-format".to_string(),
            args: vec!["-i".to_string()],
        }
    }

    /// Create a formatter from a fully custom command and argument list.
    ///
    /// ```no_run
    /// let fmt = tuxpad::formatter::Formatter::custom("gofmt", &["-w"]);
    /// ```
    pub fn custom(command: &str, args: &[&str]) -> Self {
        Self {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── Language detection ────────────────────────────────────────────────────

    /// Return the appropriate formatter for a given GtkSourceView language ID,
    /// or `None` when no preset is available.
    pub fn for_language(language_id: &str) -> Option<Self> {
        match language_id {
            "rust" => Some(Self::rustfmt()),
            "js" | "javascript" | "typescript" | "css" | "html" | "json" => {
                Some(Self::prettier())
            }
            "python" => Some(Self::black()),
            "c" | "cpp" => Some(Self::clang_format()),
            _ => None,
        }
    }

    // ── Execution ─────────────────────────────────────────────────────────────

    /// Run the formatter on `file_path`.
    ///
    /// Most formatters operate *in-place* (they rewrite the file on disk);
    /// callers should reload the buffer after a successful call.
    ///
    /// Returns `Ok(())` on success, or an `Err` containing the formatter's
    /// stderr output together with a human-readable message.
    pub fn format_file(&self, file_path: &Path) -> Result<(), String> {
        let path_str = file_path
            .to_str()
            .ok_or_else(|| "File path contains non-UTF-8 characters".to_string())?;

        let output = Command::new(&self.command)
            .args(&self.args)
            .arg(path_str)
            .output()
            .map_err(|e| format!("Could not launch '{}': {}", self.command, e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "'{}' exited with {}: {}",
                self.command,
                output.status,
                stderr.trim()
            ))
        }
    }

    /// Run the formatter on a string in memory and return the formatted result.
    ///
    /// The content is piped to stdin and the formatted output is read from
    /// stdout, which avoids touching the file system.
    ///
    /// Not every formatter supports stdin mode; this method adds `--stdin`
    /// (or equivalent) flags only for known formatters.  Unknown formatters
    /// fall back to a temporary file.
    pub fn format_string(&self, content: &str, file_extension: &str) -> Result<String, String> {
        use std::io::Write;

        // Build stdin-compatible arguments for known formatters
        let stdin_args: Vec<String> = match self.command.as_str() {
            "rustfmt" => vec![],           // rustfmt reads stdin by default
            "prettier" => vec![
                "--stdin-filepath".to_string(),
                format!("input.{}", file_extension),
            ],
            "black" => vec!["-".to_string()], // black reads stdin with "-"
            _ => {
                // Fallback: write to a temp file, format, read back
                return self.format_via_tempfile(content, file_extension);
            }
        };

        let mut child = Command::new(&self.command)
            .args(&self.args)
            .args(&stdin_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not launch '{}': {}", self.command, e))?;

        // Write content to the formatter's stdin
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| format!("Failed to write to formatter stdin: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Formatter '{}' failed: {}", self.command, e))?;

        if output.status.success() {
            String::from_utf8(output.stdout)
                .map_err(|e| format!("Formatter output is not valid UTF-8: {}", e))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "'{}' exited with {}: {}",
                self.command,
                output.status,
                stderr.trim()
            ))
        }
    }

    /// Internal helper: write content to a temp file, format it in-place,
    /// then read and return the result.
    fn format_via_tempfile(
        &self,
        content: &str,
        file_extension: &str,
    ) -> Result<String, String> {
        use std::io::Write;

        let tmp_path = std::env::temp_dir()
            .join(format!("tuxpad_fmt_{}.{}", std::process::id(), file_extension));

        // Write content
        {
            let mut f = std::fs::File::create(&tmp_path)
                .map_err(|e| format!("Cannot create temp file: {}", e))?;
            f.write_all(content.as_bytes())
                .map_err(|e| format!("Cannot write temp file: {}", e))?;
        }

        // Format in-place
        self.format_file(&tmp_path)?;

        // Read back
        let formatted = std::fs::read_to_string(&tmp_path)
            .map_err(|e| format!("Cannot read formatted temp file: {}", e))?;

        // Clean up
        let _ = std::fs::remove_file(&tmp_path);
        Ok(formatted)
    }
}
