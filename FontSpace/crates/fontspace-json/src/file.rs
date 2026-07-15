//! Path-based document persistence: read a `.fontspace.json` file into a document,
//! and write one back using **atomic replacement** (spec/16 §16.2).
//!
//! This is the single home for reading/writing document files. The pure
//! string<->document conversion lives in [`crate::save`]/[`crate::load`]; this module
//! only adds the filesystem edge (and the atomic-write guarantee) so the CLI and GUI
//! never re-implement it. It stays free of any UI/session concern (spec/02).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::{fs, io};

use fontspace_model::FontSpace;

use crate::{JsonError, LoadOutcome, load, save};

/// A failure reading a document file: either the filesystem read or the parse, each
/// carrying the path so the caller can report which file was at fault.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("reading {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("parsing {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: JsonError,
    },
}

/// Reads `path` and parses it as a canonical FontSpace document (spec/06). Dangling
/// glyphs are tolerated and surfaced as [`LoadOutcome::warnings`]; a missing file or
/// hard structural error is a [`ReadError`].
pub fn read_document(path: &Path) -> Result<LoadOutcome, ReadError> {
    let text = fs::read_to_string(path).map_err(|source| ReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load(&text).map_err(|source| ReadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes `doc` to `path` as canonical JSON using atomic replacement (spec/16 §16.2):
/// write a sibling `.tmp`, flush it to disk, then rename it over the destination. A
/// failed or partial write leaves any existing file untouched — a crashed save never
/// corrupts the user's document.
pub fn write_document(path: &Path, doc: &FontSpace) -> io::Result<()> {
    let json = save(doc);

    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);

    let mut file = fs::File::create(&tmp)?;
    file.write_all(json.as_bytes())?;
    // Flush to disk *before* the rename so the replaced file is never a half-written
    // temp promoted into place by a crash between write and fsync (spec/16 §16.2).
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::{FontSpace, SequentialIdGen};

    /// A distinct temp path per test name — deterministic (no time/random) and
    /// collision-free across the parallel test threads in this binary.
    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fontspace-json-{tag}.fontspace.json"))
    }

    fn sample() -> FontSpace {
        let mut ids = SequentialIdGen::new();
        FontSpace::new(&mut ids, "Round Trip", "a doc")
    }

    #[test]
    fn write_then_read_round_trips_canonical_bytes() {
        let path = temp_path("round-trip");
        let _ = fs::remove_file(&path);
        let doc = sample();

        write_document(&path, &doc).expect("atomic write succeeds");
        let outcome = read_document(&path).expect("read back succeeds");

        // The bytes on disk are exactly canonical JSON, and re-reading reproduces the
        // same document (save is the canonical comparison — spec/06).
        assert_eq!(save(&outcome.document), save(&doc));
        assert_eq!(fs::read_to_string(&path).unwrap(), save(&doc));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_leaves_no_dangling_tmp() {
        let path = temp_path("no-tmp");
        let tmp = temp_path("no-tmp");
        let tmp = {
            let mut s = tmp.into_os_string();
            s.push(".tmp");
            PathBuf::from(s)
        };
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&tmp);

        write_document(&path, &sample()).expect("write succeeds");
        assert!(path.exists(), "destination written");
        assert!(!tmp.exists(), "temp renamed away, not left behind");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn read_missing_file_is_an_io_error_naming_the_path() {
        let path = temp_path("does-not-exist");
        let _ = fs::remove_file(&path);
        let err = read_document(&path).expect_err("missing file fails");
        assert!(matches!(err, ReadError::Io { .. }));
        assert!(err.to_string().contains("does-not-exist"));
    }

    #[test]
    fn read_malformed_json_is_a_parse_error() {
        let path = temp_path("malformed");
        fs::write(&path, b"{ not valid json").unwrap();
        let err = read_document(&path).expect_err("bad json fails");
        assert!(matches!(err, ReadError::Parse { .. }));
        let _ = fs::remove_file(&path);
    }
}
