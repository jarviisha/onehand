//! Attachment metadata shared by composer staging, transcript rendering, and
//! ACP delivery. Paths remain the transport authority, while this model keeps
//! presentation and validation decisions out of individual widgets.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Images at or below this size are sent as inline ACP image blocks. Larger
/// images remain previewable locally but are delivered as resource links.
pub(crate) const MAX_INLINE_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

static NEXT_ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentId(u64);

impl AttachmentId {
    fn next() -> Self {
        Self(NEXT_ATTACHMENT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentDelivery {
    InlineImage,
    ResourceLink,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentSource {
    Picker,
    Clipboard,
}

/// An editable attachment in the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedAttachment {
    pub id: AttachmentId,
    pub path: PathBuf,
    pub name: String,
    pub bytes: Option<u64>,
    pub kind: AttachmentKind,
    pub delivery: AttachmentDelivery,
    pub source: AttachmentSource,
}

impl StagedAttachment {
    pub fn inspect(path: PathBuf, source: AttachmentSource) -> Self {
        let inspected = inspect_path(&path);
        Self {
            id: AttachmentId::next(),
            path,
            name: inspected.name,
            bytes: inspected.bytes,
            kind: inspected.kind,
            delivery: inspected.delivery,
            source,
        }
    }

    pub fn snapshot(self) -> AttachmentSnapshot {
        AttachmentSnapshot {
            path: self.path,
            name: self.name,
            bytes: self.bytes,
            kind: self.kind,
            delivery: self.delivery,
        }
    }
}

/// Immutable presentation data kept with a submitted prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentSnapshot {
    pub path: PathBuf,
    pub name: String,
    pub bytes: Option<u64>,
    pub kind: AttachmentKind,
    pub delivery: AttachmentDelivery,
}

impl AttachmentSnapshot {
    /// Restore the best available snapshot from a legacy path-only archive.
    pub fn from_path(path: PathBuf) -> Self {
        let inspected = inspect_path(&path);
        Self {
            path,
            name: inspected.name,
            bytes: inspected.bytes,
            kind: inspected.kind,
            delivery: inspected.delivery,
        }
    }

    pub fn is_available(&self) -> bool {
        !matches!(self.delivery, AttachmentDelivery::Unavailable) && self.path.is_file()
    }
}

struct InspectedPath {
    name: String,
    bytes: Option<u64>,
    kind: AttachmentKind,
    delivery: AttachmentDelivery,
}

fn inspect_path(path: &Path) -> InspectedPath {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string());
    let metadata = std::fs::metadata(path).ok().filter(|meta| meta.is_file());
    let bytes = metadata.as_ref().map(std::fs::Metadata::len);
    let kind = if is_previewable_image(path) {
        AttachmentKind::Image
    } else {
        AttachmentKind::File
    };
    let delivery = match bytes {
        None => AttachmentDelivery::Unavailable,
        Some(bytes) if inline_image_mime(path).is_some() && bytes <= MAX_INLINE_IMAGE_BYTES => {
            AttachmentDelivery::InlineImage
        }
        Some(_) => AttachmentDelivery::ResourceLink,
    };
    InspectedPath {
        name,
        bytes,
        kind,
        delivery,
    }
}

/// Formats that the local image widget can preview.
pub(crate) fn is_previewable_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
            )
        })
        .unwrap_or(false)
}

/// MIME types ACP accepts as inline image blocks.
pub(crate) fn inline_image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// How big an attachment is, in the shortest form that still says it.
///
/// Sizes are what tell two staged files apart when their names do not, and they
/// are the only warning that the 6 MB screenshot about to be sent will go as a
/// link rather than inline. One decimal below 10 of a unit and none above it:
/// `1.4 MB` is worth reading, `14.3 MB` is three characters spent on a digit
/// nobody is deciding anything with.
pub fn size_label(bytes: u64) -> String {
    const STEP: f64 = 1024.;
    let units = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= STEP && unit + 1 < units.len() {
        size /= STEP;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if size < 10. {
        format!("{size:.1} {}", units[unit])
    } else {
        format!("{size:.0} {}", units[unit])
    }
}

/// Put a pasted image on disk so it can be staged like any other attachment.
///
/// Everything downstream of the composer addresses an attachment by **path** —
/// the tray reads its size back, `Chat::submit` sends the path, and the
/// transcript reopens it to draw the thumbnail. A clipboard image has no path,
/// so it needs one before it can join that world at all.
///
/// Named by the content hash the caller already has, which makes the write
/// idempotent: pasting the same screenshot into four prompts leaves one file,
/// not four. The extension decides how it is delivered later (an inline image
/// block or a resource link), so it is filtered down to plain letters and
/// digits rather than trusted — a path is being built out of it.
///
/// Blocking, like every other file operation here. The caller puts it on a
/// background thread.
pub fn write_clipboard_image(id: u64, extension: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let extension: String = extension
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let extension = if extension.is_empty() {
        "png".to_string()
    } else {
        extension.to_ascii_lowercase()
    };

    let dir = std::env::temp_dir().join("onehand-pastes");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("pasted-{id:016x}.{extension}"));
    // Written every time rather than skipped when the name is taken: a
    // half-written file from a previous run has the same name as the whole one.
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pasted image has to come back as an ordinary, readable attachment --
    /// staged straight from the clipboard it would be `Unavailable`, which is
    /// the state that blocks Send.
    #[test]
    fn a_pasted_image_becomes_a_file_that_can_be_staged() {
        let bytes = b"\x89PNG\r\n\x1a\n not really a png";
        let path = write_clipboard_image(0xfeed_face, "PNG", bytes).expect("write");
        assert_eq!(std::fs::read(&path).expect("read"), bytes);

        let staged = StagedAttachment::inspect(path.clone(), AttachmentSource::Clipboard);
        assert_eq!(staged.kind, AttachmentKind::Image);
        assert_eq!(staged.delivery, AttachmentDelivery::InlineImage);
        assert_eq!(staged.bytes, Some(bytes.len() as u64));

        // The same image pasted twice is the same file, not a second copy.
        let again = write_clipboard_image(0xfeed_face, "png", bytes).expect("write");
        assert_eq!(again, path);

        // A format with nothing usable in its name still produces a path.
        let odd = write_clipboard_image(1, "../..", bytes).expect("write");
        assert_eq!(odd.extension().and_then(|e| e.to_str()), Some("png"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(odd);
    }

    #[test]
    fn a_size_reads_short_and_never_lies_about_its_unit() {
        assert_eq!(size_label(0), "0 B");
        assert_eq!(size_label(999), "999 B");
        assert_eq!(size_label(1024), "1.0 KB");
        assert_eq!(size_label(1024 * 1024 * 3 / 2), "1.5 MB");
        // Past ten of a unit the decimal is noise, not precision.
        assert_eq!(size_label(1024 * 1024 * 42), "42 MB");
        assert_eq!(size_label(u64::MAX), "17179869184 GB");
    }

    #[test]
    fn preview_and_inline_support_are_distinct() {
        assert!(is_previewable_image(Path::new("shot.bmp")));
        assert_eq!(inline_image_mime(Path::new("shot.bmp")), None);
        assert_eq!(
            inline_image_mime(Path::new("shot.JPEG")),
            Some("image/jpeg")
        );
    }

    #[test]
    fn missing_path_keeps_a_stable_display_name() {
        let attachment = AttachmentSnapshot::from_path(PathBuf::from("/missing/report.pdf"));
        assert_eq!(attachment.name, "report.pdf");
        assert_eq!(attachment.delivery, AttachmentDelivery::Unavailable);
    }
}
