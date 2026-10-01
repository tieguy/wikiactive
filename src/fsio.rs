//! The tool's file-IO seam (`SonarCloud` rule `S7493`): blocking
//! operations on the tiny local session files live in these named
//! synchronous functions instead of inline in `async fn` bodies.
//! Signatures match `std::fs` exactly, so call sites read identically —
//! the seam exists so the async/sync boundary is explicit and auditable
//! (single-user tool, small files: measured blocking, deliberately
//! sync).

/// `std::fs::read_to_string` behind the seam.
///
/// # Errors
/// Whatever `std::fs::read_to_string` reports.
pub fn read_to_string<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// `std::fs::write` behind the seam.
///
/// # Errors
/// Whatever `std::fs::write` reports.
pub fn write<P: AsRef<std::path::Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

/// `std::fs::create_dir_all` behind the seam.
///
/// # Errors
/// Whatever `std::fs::create_dir_all` reports.
pub fn create_dir_all<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `std::fs::remove_file` behind the seam.
///
/// # Errors
/// Whatever `std::fs::remove_file` reports.
pub fn remove_file<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// Append one line (created if absent) to an append-only log — the
/// `rounds.jsonl` writes.
///
/// # Errors
/// Opening or writing the log file.
pub fn append_line<P: AsRef<std::path::Path>>(path: P, line: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}
