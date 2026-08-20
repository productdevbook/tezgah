//! Where a file lives.
//!
//! tezgah stores none and asks a host for none: a product's image is a URL in
//! a column, and what serves that URL is the host's business. So this is the
//! product's, the same way the mailer is — one directory on disk, no
//! transforms, no CDN, no signed URLs.
//!
//! **A directory is a deliberate ceiling.** One machine, one disk, and a shop
//! outgrowing that wants object storage rather than a bigger disk here.
//! Because what is written into the product is an ordinary URL, moving to one
//! is putting the bucket behind the same path — nothing in the catalogue has
//! to change.
//!
//! Unset, unmounted. A shop that would rather host its images where it
//! already hosts them keeps doing that, and the panel goes on taking a URL.
//!
//! ## What is not trusted
//!
//! The name the browser sent, for one: it never reaches the disk. A file is
//! written as `<uuid>.<ext>`, where the extension comes from the content type
//! this module recognises rather than from anything in the request — which is
//! what makes path traversal impossible rather than merely handled.
//!
//! The content type is not trusted either, in the other direction: it is
//! matched against a list of five image types, and anything else is refused
//! rather than stored under a name that would let it be served back as
//! something a browser executes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

/// Most bytes one upload may be. Beyond this a shop is storing something that
/// is not a product image, and doing it in a directory with no quota.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// The only five, and the extension each is written as.
///
/// A list rather than a check for `image/`: `image/svg+xml` is an image by
/// that rule and a script by every other, and serving one back from the
/// shop's own origin is a cross-site scripting hole with a picture frame
/// around it.
const ALLOWED: [(&str, &str); 5] = [
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
    ("image/avif", "avif"),
];

pub fn extension_for(content_type: &str) -> Option<&'static str> {
    let bare = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    ALLOWED
        .iter()
        .find(|(kind, _)| *kind == bare)
        .map(|(_, ext)| *ext)
}

/// The content type a stored name is served back as, read from the name this
/// module gave it and never from the file's contents.
pub fn type_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?;
    ALLOWED
        .iter()
        .find(|(_, known)| *known == ext)
        .map(|(kind, _)| *kind)
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: Arc<Path>,
    /// What a stored file's URL starts with. A shop behind a CDN points this
    /// at the CDN; left alone it is this binary's own `/files`.
    base: Arc<str>,
}

impl Store {
    pub async fn open(dir: &str, base: &str) -> tezgah::Result<Store> {
        let dir = PathBuf::from(dir);
        tokio::fs::create_dir_all(&dir).await.map_err(|err| {
            tezgah::Error::invalid(format!("TEZGAH_FILE_DIR {}: {err}", dir.display()))
        })?;

        Ok(Store {
            dir: Arc::from(dir.as_path()),
            base: Arc::from(base.trim_end_matches('/')),
        })
    }

    /// Writes the bytes and answers the URL they are now at.
    ///
    /// The name is this module's — a uuid and an extension it chose — so
    /// nothing a caller sent decides where the file lands.
    pub async fn save(&self, content_type: &str, bytes: &[u8]) -> tezgah::Result<String> {
        let Some(ext) = extension_for(content_type) else {
            return Err(tezgah::Error::invalid(
                "that is not an image this shop stores — jpeg, png, webp, gif or avif",
            ));
        };

        if bytes.is_empty() {
            return Err(tezgah::Error::invalid("that file is empty"));
        }
        if bytes.len() > MAX_BYTES {
            return Err(tezgah::Error::invalid(format!(
                "that file is larger than {} MB",
                MAX_BYTES / 1024 / 1024
            )));
        }

        let name = format!("{}.{ext}", Uuid::now_v7().simple());
        tokio::fs::write(self.dir.join(&name), bytes)
            .await
            .map_err(|err| tezgah::Error::invalid(format!("could not store the file: {err}")))?;

        Ok(format!("{}/{name}", self.base))
    }

    /// Reads one back by the name this module gave it.
    ///
    /// The name is checked against that shape before it touches the path:
    /// thirty-two hex characters, a dot, and one of five extensions. A name
    /// that is not that is refused, so `..` never becomes a path segment and
    /// no symlink in the directory becomes a way out of it.
    pub async fn read(&self, name: &str) -> tezgah::Result<(Vec<u8>, &'static str)> {
        let Some((stem, ext)) = name.split_once('.') else {
            return Err(tezgah::Error::not_found("file"));
        };
        let shaped = stem.len() == 32
            && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
            && ALLOWED.iter().any(|(_, known)| *known == ext);
        if !shaped {
            return Err(tezgah::Error::not_found("file"));
        }

        let bytes = tokio::fs::read(self.dir.join(name))
            .await
            .map_err(|_| tezgah::Error::not_found("file"))?;
        let kind = type_for(name).ok_or_else(|| tezgah::Error::not_found("file"))?;

        Ok((bytes, kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_five_are_stored() {
        assert_eq!(extension_for("image/png"), Some("png"));
        assert_eq!(extension_for("image/JPEG"), Some("jpg"));
        assert_eq!(extension_for("image/png; charset=binary"), Some("png"));

        // The one worth naming: an SVG is an image by any prefix check and a
        // script by every other measure.
        assert_eq!(extension_for("image/svg+xml"), None);
        assert_eq!(extension_for("text/html"), None);
        assert_eq!(extension_for("application/octet-stream"), None);
    }

    #[test]
    fn a_name_that_is_not_ours_is_not_a_path() {
        for name in [
            "../../etc/passwd",
            "..%2f..%2fetc%2fpasswd",
            "a.png",
            "0123456789abcdef0123456789abcdef.svg",
            "0123456789abcdef0123456789abcdeg.png",
            "0123456789abcdef0123456789abcdef",
        ] {
            let Some((stem, ext)) = name.split_once('.') else {
                continue;
            };
            let shaped = stem.len() == 32
                && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
                && ALLOWED.iter().any(|(_, known)| *known == ext);
            assert!(!shaped, "{name} passed the shape check");
        }

        let good = format!("{}.png", Uuid::now_v7().simple());
        let (stem, ext) = good.split_once('.').expect("a dot");
        assert!(
            stem.len() == 32
                && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
                && ALLOWED.iter().any(|(_, known)| *known == ext)
        );
    }
}
