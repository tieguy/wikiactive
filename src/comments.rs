//! The comment queue — the single reviewer↔loop interface (plan-004 P.1).
//!
//! Replaces the lavish annotation/poll leg: the operator leaves block-anchored
//! comments on the session page (`wa serve`) or via `wa comments add`, the
//! driver's resolve step consumes the OPEN entries, and every resolution is
//! persisted back here (the queue is the record — no in-memory state).
//!
//! The queue file is `sessions/<slug>/comments.jsonl`, append-only, one
//! JSON object per line (the ledger pattern). Serde defaults keep old
//! sessions loadable as the schema grows.
//!
//! `target` semantics: the artifact's `wikitext_anchor` **verbatim** from
//! the embedded `#wa-anchor-table` (a plain `L..:C..-L..:C..` range for
//! new-side blocks, `base:`-prefixed for removed wording, `ledger:Q<n>`
//! for evidence cards) — the in-app forms write it directly, so comment
//! anchoring is exact by construction. An element id (`wa-2`, `ev-1`) is
//! also accepted (`wa comments add --target wa-2`); the driver-resolve
//! step maps it through the anchor table and leaves the comment open with
//! an error note when the id is unknown.

use std::io::Write as _;

use serde::Deserialize;
use serde::Serialize;

/// Comment lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommentStatus {
    /// Awaiting resolution (driver apply or manual).
    #[default]
    Open,
    /// Resolved; `resolution` carries the note.
    Resolved,
}

/// One reviewer comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// Queue id (`K1`, `K2`, …), allocated at append.
    pub id: String,
    /// The anchor this comment targets (see the module doc): a
    /// `wikitext_anchor` verbatim, or an artifact element id.
    pub target: String,
    /// The comment text.
    pub text: String,
    /// Operator-highlighted words, free text (the poor reviewer's
    /// text-selection; the resolution quotes the acted-on span back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted: Option<String>,
    /// ISO-8601 timestamp of submission.
    pub timestamp: String,
    /// Open or resolved.
    #[serde(default)]
    pub status: CommentStatus,
    /// What was done: the combined applied/rejected lines + the model's
    /// reply (driver resolution), or the operator's manual note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
}

impl Comment {
    /// A new open comment (id allocated by the queue at append).
    #[must_use]
    pub fn new(target: &str, text: &str, quoted: Option<&str>, timestamp: &str) -> Self {
        Self {
            id: String::new(),
            target: target.to_string(),
            text: text.to_string(),
            quoted: quoted.map(str::to_string),
            timestamp: timestamp.to_string(),
            status: CommentStatus::Open,
            resolution: None,
        }
    }

    /// Is this a comment on an evidence card (about a source/quote, not
    /// wikitext)? Targets can be the `ledger:Q<n>` anchor directly or the
    /// `ev-N` element id.
    #[must_use]
    pub fn is_evidence(&self) -> bool {
        self.target.starts_with("ledger:") || self.target.starts_with("ev-")
    }
}

/// The loaded queue (`comments.jsonl`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentQueue {
    pub comments: Vec<Comment>,
}

impl CommentQueue {
    /// Load from disk. A missing file is an empty queue (no comments yet —
    /// the normal state before the first review); a malformed line is a
    /// loud error, never a silent skip (the anchor-table lesson).
    ///
    /// # Errors
    /// IO failure other than "not found", or a line that does not parse.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("comments queue {}: {e}", path.display())),
        };
        let mut comments = Vec::new();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: Comment = serde_json::from_str(line)
                .map_err(|e| format!("comments queue {}: line {}: {e}", path.display(), n + 1))?;
            comments.push(parsed);
        }
        Ok(Self { comments })
    }

    /// Next queue id (`K1`, `K2`, …) given the loaded entries.
    #[must_use]
    pub fn next_id(&self) -> String {
        let max = self
            .comments
            .iter()
            .filter_map(|c| c.id.strip_prefix('K').and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("K{}", max + 1)
    }

    /// Append one comment (allocating its id) and persist the single JSONL
    /// line — nothing is rewritten. The id is allocated from the file as
    /// it is NOW, not from this snapshot, so a comment another writer
    /// added since the load never shares an id with this one.
    ///
    /// # Errors
    /// A malformed queue file or IO failure; the comment is then neither
    /// on disk nor in memory.
    pub fn append(
        &mut self,
        path: &std::path::Path,
        mut comment: Comment,
    ) -> Result<String, String> {
        *self = Self::load(path)?;
        comment.id = self.next_id();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("comments queue {}: {e}", path.display()))?;
        let line = serde_json::to_string(&comment).map_err(|e| e.to_string())?;
        writeln!(file, "{line}").map_err(|e| e.to_string())?;
        let id = comment.id.clone();
        self.comments.push(comment);
        Ok(id)
    }

    /// Resolve one comment (status flip + note). `resolve_note` is what was
    /// done; it is persisted in the queue, not held in memory. The rewrite
    /// starts from the file as it is NOW: a comment appended since this
    /// queue was loaded (the operator keeps commenting while the driver
    /// waits on the model) must survive it.
    ///
    /// # Errors
    /// Unknown id, a malformed queue file, or the save failure (the
    /// resolution is rewritten to disk — the queue is the record).
    pub fn resolve(
        &mut self,
        path: &std::path::Path,
        id: &str,
        resolve_note: &str,
    ) -> Result<(), String> {
        let mut current = Self::load(path)?;
        let comment = current
            .comments
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("no comment {id}"))?;
        comment.status = CommentStatus::Resolved;
        comment.resolution = Some(resolve_note.to_string());
        current.save(path)?;
        *self = current;
        Ok(())
    }

    /// Rewrite the whole queue (resolve is the only mutator; appends go
    /// through [`Self::append`]).
    ///
    /// # Errors
    /// Serialization or IO failure.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let mut out = String::new();
        for c in &self.comments {
            out.push_str(&serde_json::to_string(c).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        // Temp file + rename: a crash mid-write must not truncate the queue.
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, out)
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("comments queue {}: {e}", path.display()))
    }

    /// The open comments (driver-resolve input).
    #[must_use]
    pub fn open(&self) -> Vec<&Comment> {
        self.comments
            .iter()
            .filter(|c| c.status == CommentStatus::Open)
            .collect()
    }
}

/// ISO-8601 now, to the second, `Z`-suffixed (the session-log format).
#[must_use]
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::{Comment, CommentQueue, CommentStatus};

    fn tmp(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("wa-comments-{name}-{n}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("comments.jsonl")
    }

    #[test]
    fn missing_file_is_an_empty_queue() {
        let q = CommentQueue::load(std::path::Path::new("/nonexistent/comments.jsonl")).unwrap();
        assert!(q.comments.is_empty());
        assert_eq!(q.next_id(), "K1");
    }

    #[test]
    fn append_allocates_ids_and_round_trips() {
        let path = tmp("roundtrip");
        let mut q = CommentQueue::default();
        let k1 = q
            .append(
                &path,
                Comment::new(
                    "L3:C0-L3:C120",
                    "tighten this",
                    Some("several hundred"),
                    "t1",
                ),
            )
            .unwrap();
        let k2 = q
            .append(
                &path,
                Comment::new("ledger:Q1", "quote looks trimmed", None, "t2"),
            )
            .unwrap();
        assert_eq!((k1.as_str(), k2.as_str()), ("K1", "K2"));

        let loaded = CommentQueue::load(&path).unwrap();
        assert_eq!(loaded.comments.len(), 2);
        assert_eq!(loaded.comments[0].id, "K1");
        assert_eq!(loaded.comments[0].target, "L3:C0-L3:C120");
        assert_eq!(
            loaded.comments[0].quoted.as_deref(),
            Some("several hundred")
        );
        assert_eq!(loaded.comments[0].status, CommentStatus::Open);
        assert_eq!(loaded.comments[1].target, "ledger:Q1");
        assert!(loaded.comments[1].quoted.is_none());
        assert_eq!(loaded.next_id(), "K3");

        // A fresh queue over the same file continues the id sequence.
        let mut again = CommentQueue::load(&path).unwrap();
        let k3 = again
            .append(
                &path,
                Comment::new("base:L5:C0-L5:C9", "keep this?", None, "t3"),
            )
            .unwrap();
        assert_eq!(k3, "K3");
    }

    #[test]
    fn resolve_flips_status_and_persists_the_note() {
        let path = tmp("resolve");
        let mut q = CommentQueue::default();
        let k1 = q
            .append(&path, Comment::new("wa-2", "fix", None, "t1"))
            .unwrap();
        q.resolve(&path, &k1, "applied; revised block spliced at L3")
            .unwrap();

        let loaded = CommentQueue::load(&path).unwrap();
        assert_eq!(loaded.comments[0].status, CommentStatus::Resolved);
        assert_eq!(
            loaded.comments[0].resolution.as_deref(),
            Some("applied; revised block spliced at L3")
        );
        assert!(loaded.open().is_empty());
    }

    #[test]
    fn resolve_of_unknown_id_fails_loudly() {
        let path = tmp("unknown");
        let mut q = CommentQueue::default();
        q.append(&path, Comment::new("wa-2", "fix", None, "t1"))
            .unwrap();
        assert!(q.resolve(&path, "K9", "note").is_err());
    }

    #[test]
    fn malformed_line_is_a_loud_error() {
        let path = tmp("malformed");
        std::fs::write(&path, "{\"id\":\"K1\" garbage\n").unwrap();
        assert!(CommentQueue::load(&path).is_err());
    }

    #[test]
    fn evidence_targets_are_recognized_in_both_spellings() {
        let by_anchor = Comment::new("ledger:Q2", "source?", None, "t");
        let by_id = Comment::new("ev-1", "source?", None, "t");
        let wikitext = Comment::new("base:L1:C0-L1:C9", "words", None, "t");
        assert!(by_anchor.is_evidence());
        assert!(by_id.is_evidence());
        assert!(!wikitext.is_evidence());
    }
}
