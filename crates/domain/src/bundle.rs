//! Reading a Polaris estate bundle (`contracts/bundle.schema.json`) — W5 task 9.
//!
//! A bundle is a zip carrying `bundle.json`, `content.md` and any attachments, with a sha256 per file in the manifest. The
//! reader's job is to refuse: to hand the pipeline bytes it can prove are the bytes that were exported, and to say which
//! file failed when it cannot.
//!
//! # What it deliberately does not do
//!
//! It does not decide who the work belongs to (the import request takes an explicit owner), it does not write anything,
//! and it does not convert the Markdown — the product's own writer stores the body, so a converter here would be a second
//! renderer.
//!
//! # The measured shape
//!
//! A real export of one work (`tools/export_bundle.py --kind work --id … --zip`, 2026-11-06):
//!
//! ```text
//! 129      attachments.json
//! 630      bundle.json
//! 8994994  content.md
//! ```
//!
//! `content.md` is the whole body with the 784 units concatenated, so this reader holds one copy of it in memory and
//! another in the checksum digest. If a caller ever needs to stream an 8.99 MB body, that is a change to this API and its
//! caller together — designing the streaming shape without the caller would be a guess.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The only bundle version this reader understands. Checked, not hoped for: a v2 bundle imported by a v1 reader would
/// produce a work that looks restored and is not.
pub const SUPPORTED_VERSION: u32 = 1;

/// The manifest, as the exporter writes it.
///
/// `deny_unknown_fields` is deliberately **not** used: a future exporter adding a field must not make older readers refuse
/// a bundle that is otherwise intact.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Manifest {
    pub estate_bundle: u32,
    pub kind: String,
    pub id: String,
    pub site: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub canonical_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub exported_at: Option<String>,
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub checksums: BTreeMap<String, String>,
}

/// One work, verified, ready for the pipeline to store.
#[derive(Debug, Clone)]
pub struct BundleItem {
    pub manifest: Manifest,
    /// `content.md`, byte for byte.
    pub body: String,
    /// Every other file the manifest checksums, by name. `attachments.json` is included: the pipeline decides what to do
    /// with it, and a reader that dropped it would lose the only description of the originals.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Every way a bundle can fail to be one.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("the archive could not be opened: {0}")]
    Archive(String),
    #[error("the archive has no bundle.json — this is not a Polaris bundle")]
    NoManifest,
    #[error("bundle.json is not valid JSON: {0}")]
    Manifest(String),
    #[error("bundle version {0} is not supported by this reader (it understands {SUPPORTED_VERSION})")]
    UnsupportedVersion(u32),
    #[error("bundle item kind {0:?} is not a work")]
    UnsupportedKind(String),
    #[error("the archive has no {0}")]
    MissingEntry(String),
    #[error("{path} does not match its checksum (expected {expected}, computed {actual})")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("{0} is listed in the manifest but is not in the archive")]
    UnlistedEntry(String),
    #[error("content.md is not UTF-8: {0}")]
    NotUtf8(String),
}

impl BundleItem {
    /// Open and verify a bundle.
    ///
    /// Two rules that are not obvious:
    ///
    /// * **Every checksummed file must be present, and every checksum must match.** A missing attachment is a refusal, not
    ///   a warning: the manifest said it was there, so a bundle without it is not the bundle that was exported.
    /// * **`content.md` is required even when the manifest does not checksum it.** The exporter always writes it; a
    ///   manifest that checksums only attachments is a bundle with no body, and importing it would create an empty work.
    pub fn open(bytes: &[u8]) -> Result<Self, BundleError> {
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| BundleError::Archive(e.to_string()))?;

        let manifest_bytes =
            read_entry(&mut archive, "bundle.json")?.ok_or(BundleError::NoManifest)?;
        let manifest: Manifest =
            serde_json::from_slice(&manifest_bytes).map_err(|e| BundleError::Manifest(e.to_string()))?;
        if manifest.estate_bundle != SUPPORTED_VERSION {
            return Err(BundleError::UnsupportedVersion(manifest.estate_bundle));
        }
        if manifest.kind != "work" {
            return Err(BundleError::UnsupportedKind(manifest.kind.clone()));
        }

        let body_bytes = read_entry(&mut archive, "content.md")?
            .ok_or_else(|| BundleError::MissingEntry("content.md".into()))?;

        // Verify every file the manifest names, `content.md` included, against the bytes actually in the archive.
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (path, expected) in &manifest.checksums {
            let bytes = if path == "content.md" {
                body_bytes.clone()
            } else {
                read_entry(&mut archive, path)?.ok_or_else(|| BundleError::UnlistedEntry(path.clone()))?
            };
            let actual = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
            if &actual != expected {
                return Err(BundleError::ChecksumMismatch {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
            if path != "content.md" {
                files.insert(path.clone(), bytes);
            }
        }

        let body = String::from_utf8(body_bytes).map_err(|e| BundleError::NotUtf8(e.to_string()))?;
        Ok(BundleItem {
            manifest,
            body,
            files,
        })
    }
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>, BundleError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(BundleError::Archive(e.to_string())),
    };
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .map_err(|e| BundleError::Archive(e.to_string()))?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    type Checksums = Option<Vec<(&'static str, String)>>;

    fn digest(body: &str) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(body.as_bytes())))
    }

    /// A bundle built in memory.
    ///
    /// A fixture on disk is one more thing that can be missing on a fresh checkout, and the schema is small enough to
    /// synthesise — what a fixture adds is a real exporter's quirks, which is what the `#[ignore]`d real-bundle test is
    /// for. `checksums` lets a test sign a body it does not carry, which is the only way to produce a tampered archive.
    fn make(entries: &[(&str, &str)], checksums: Checksums) -> Vec<u8> {
        let checksums = checksums.unwrap_or_else(|| {
            entries.iter().map(|(name, body)| (*name, digest(body))).collect()
        });
        let manifest = Manifest {
            estate_bundle: 1,
            kind: "work".into(),
            id: "78eb8d57-e28f-49d8-b58e-ebe6181bac3b".into(),
            site: "darkhold".into(),
            title: "Wish upon the Stars".into(),
            tags: vec!["time loop".into()],
            audience: Some("public".into()),
            canonical_url: Some("https://darkhold.polarisocial.xyz/w/78eb8d57-e28f-49d8-b58e-ebe6181bac3b".into()),
            created_at: Some("2026-10-27T15:05:58Z".into()),
            exported_at: Some("2026-10-27T15:05:58Z".into()),
            generator: Some("polaris-export/1.0 (estate 2026.11.3)".into()),
            checksums: checksums.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        pack(&bytes, entries)
    }

    fn pack(manifest: &[u8], entries: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("bundle.json", options).unwrap();
            zip.write_all(manifest).unwrap();
            for (name, body) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        out
    }

    const BODY: &str = "# Wish upon the Stars\n\nThe loop remembers.\n";
    const ATTACH: &str = "{\"files\":[]}\n";

    #[test]
    fn good_bundle_reading_round_trips() {
        let item = BundleItem::open(&make(&[("content.md", BODY), ("attachments.json", ATTACH)], None))
            .expect("a well-formed bundle must open");
        assert_eq!(item.manifest.title, "Wish upon the Stars");
        assert_eq!(item.manifest.tags, vec!["time loop".to_string()]);
        assert_eq!(item.body, BODY);
        assert!(item.files.contains_key("attachments.json"));
        assert!(!item.files.contains_key("content.md"), "the body is not duplicated into files");
    }

    #[test]
    fn a_tampered_content_is_refused() {
        // The manifest signs the true body; the archive carries a different one. This is what a corrupted download looks
        // like, and the case a reader that skipped the digest would import as a mangled work.
        let checksums = vec![("content.md", digest(BODY)), ("attachments.json", digest(ATTACH))];
        let bytes = make(
            &[("content.md", "# Wish upon the Stars\n\nThe loop forgets.\n"), ("attachments.json", ATTACH)],
            Some(checksums),
        );
        match BundleItem::open(&bytes) {
            Err(BundleError::ChecksumMismatch { path, .. }) => assert_eq!(path, "content.md"),
            other => panic!("expected a checksum refusal for content.md, got {other:?}"),
        }
    }

    #[test]
    fn a_bundle_without_a_manifest_is_refused() {
        // Written with the raw writer because `pack` always writes bundle.json — the absence is the point of the test.
        let bytes = {
            let mut out = Vec::new();
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            zip.start_file("content.md", SimpleFileOptions::default()).unwrap();
            zip.write_all(BODY.as_bytes()).unwrap();
            zip.finish().unwrap();
            out
        };
        assert!(matches!(BundleItem::open(&bytes), Err(BundleError::NoManifest)));
    }

    #[test]
    fn version_two_is_refused_by_name() {
        let manifest = serde_json::json!({
            "estate_bundle": 2, "kind": "work", "id": "x", "site": "darkhold", "title": "t",
            "checksums": { "content.md": digest(BODY) }
        });
        let bytes = pack(&serde_json::to_vec(&manifest).unwrap(), &[("content.md", BODY)]);
        match BundleItem::open(&bytes) {
            Err(BundleError::UnsupportedVersion(v)) => assert_eq!(v, 2),
            other => panic!("expected an unsupported-version refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_checked_file_missing_from_the_archive_is_refused() {
        // The manifest checksums attachments.json but the archive does not carry it.
        let checksums = vec![("content.md", digest(BODY)), ("attachments.json", digest("{}"))];
        let bytes = make(&[("content.md", BODY)], Some(checksums));
        match BundleItem::open(&bytes) {
            Err(BundleError::UnlistedEntry(path)) => assert_eq!(path, "attachments.json"),
            other => panic!("expected a missing-entry refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_bundle_with_no_content_md_is_refused() {
        let bytes = make(&[("attachments.json", ATTACH)], None);
        match BundleItem::open(&bytes) {
            Err(BundleError::MissingEntry(path)) => assert_eq!(path, "content.md"),
            other => panic!("expected a missing body refusal, got {other:?}"),
        }
    }

    /// The only test the synthetic ones cannot replace: a bundle the estate's own exporter produced. Ignored by default
    /// because it needs the artefact — run it with
    /// `cargo test -p darkhold-domain bundle -- --ignored real_bundle` after
    /// `python3 tools/export_bundle.py --kind work --id <id> --out /tmp/rt2 --zip` in the estate repo.
    #[test]
    #[ignore]
    fn real_bundle() {
        let bytes = std::fs::read("/tmp/rt2.zip").expect("/tmp/rt2.zip — export one first");
        let item = BundleItem::open(&bytes).expect("the exporter's own bundle must verify");
        println!(
            "verified {} file(s), body {} bytes, title {:?}",
            item.files.len(),
            item.body.len(),
            item.manifest.title
        );
        assert_eq!(item.manifest.estate_bundle, 1);
        assert!(!item.body.is_empty());
    }
}
