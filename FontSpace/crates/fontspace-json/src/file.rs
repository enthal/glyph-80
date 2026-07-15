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

use fontspace_model::{FontSpace, FontSpaceFragment};

use crate::{JsonError, LoadOutcome, load, load_fragment, save, save_fragment};

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

/// Writes `doc` to `path` as canonical JSON using atomic replacement (spec/16 §16.2).
/// A failed or partial write leaves any existing file untouched — a crashed save
/// never corrupts the user's document.
pub fn write_document(path: &Path, doc: &FontSpace) -> io::Result<()> {
    write_atomic(path, save(doc).as_bytes())
}

/// Reads `path` and parses it as a canonical fragment (spec/08 §8.5) — the on-disk
/// form the CLI produces with [`write_fragment`]. A missing file or a malformed
/// fragment is a [`ReadError`] naming the path.
pub fn read_fragment(path: &Path) -> Result<FontSpaceFragment, ReadError> {
    let text = fs::read_to_string(path).map_err(|source| ReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_fragment(&text).map_err(|source| ReadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes `fragment` to `path` as canonical fragment JSON (spec/08 §8.5) using the
/// same atomic replacement as [`write_document`].
pub fn write_fragment(path: &Path, fragment: &FontSpaceFragment) -> io::Result<()> {
    write_atomic(path, save_fragment(fragment).as_bytes())
}

/// Atomic replacement (spec/16 §16.2): write a sibling `.tmp`, flush it to disk, then
/// rename it over the destination. Shared by every canonical writer so the crash-safe
/// guarantee lives in exactly one place.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);

    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    // Flush to disk *before* the rename so the replaced file is never a half-written
    // temp promoted into place by a crash between write and fsync (spec/16 §16.2).
    file.sync_all()?;
    drop(file);

    // On a failed rename the original file is already safe (untouched); clean up the
    // orphaned temp so a botched save doesn't litter a `.tmp` beside the destination.
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
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
    fn a_failed_rename_cleans_up_the_temp_and_reports_the_error() {
        // Force the rename to fail by making the destination an existing directory.
        // The original (the directory) is untouched and no `.tmp` is left behind.
        let dir = temp_path("rename-fail-dir");
        let _ = fs::remove_file(&dir);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let tmp = {
            let mut s = dir.clone().into_os_string();
            s.push(".tmp");
            PathBuf::from(s)
        };

        write_document(&dir, &sample()).expect_err("rename onto a dir fails");
        assert!(
            !tmp.exists(),
            "orphaned temp cleaned up after a failed rename"
        );
        assert!(dir.is_dir(), "destination directory left untouched");

        let _ = fs::remove_dir_all(&dir);
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

    #[test]
    fn fragment_write_then_read_round_trips() {
        use fontspace_model::{Bitmap, FragmentGlyph, GlyphFragment, GlyphSize};
        let path = std::env::temp_dir().join("fontspace-json-fragment-round-trip.json");
        let _ = fs::remove_file(&path);
        let size = GlyphSize::new(5, 3);
        let mut bitmap = Bitmap::new_blank(size);
        bitmap.set(0, 0, true).unwrap();
        let fragment = FontSpaceFragment::Glyphs(GlyphFragment {
            source_glyph_size: size,
            glyphs: vec![FragmentGlyph {
                code: 0x41,
                label: "A".into(),
                bitmap,
            }],
        });

        write_fragment(&path, &fragment).expect("atomic fragment write succeeds");
        let back = read_fragment(&path).expect("read back succeeds");
        assert_eq!(back, fragment);
        // The bytes on disk are exactly canonical fragment JSON.
        assert_eq!(fs::read_to_string(&path).unwrap(), save_fragment(&fragment));

        let _ = fs::remove_file(&path);
    }
}
