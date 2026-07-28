use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAX_LICENSE_BYTES: usize = 1024 * 1024;

const MAX_FILENAME_BYTES: usize = 255;
const MAX_TITLE_CHARS: usize = 160;
const DIGEST_SLUG_LENGTH: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseDraft {
    slug: String,
    title: String,
    body: String,
    source_filename: String,
    sha256: String,
    uploaded_at_ms: i64,
}

impl LicenseDraft {
    pub fn from_upload(
        filename: &str,
        title: Option<&str>,
        slug: Option<&str>,
        bytes: &[u8],
        uploaded_at_ms: i64,
    ) -> Result<Self, LicenseValidationError> {
        validate_filename(filename)?;
        if bytes.len() > MAX_LICENSE_BYTES {
            return Err(LicenseValidationError::BodyTooLarge);
        }
        let body = std::str::from_utf8(bytes)
            .map_err(|_| LicenseValidationError::InvalidUtf8)?
            .to_string();
        if body.trim().is_empty() {
            return Err(LicenseValidationError::EmptyBody);
        }

        let title = derive_title(filename, title)?;
        let sha256 = hex::encode(Sha256::digest(bytes));
        let slug = derive_slug(filename, slug, &sha256)?;
        Ok(Self {
            slug,
            title,
            body,
            source_filename: filename.to_string(),
            sha256,
            uploaded_at_ms,
        })
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn source_filename(&self) -> &str {
        &self.source_filename
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn uploaded_at_ms(&self) -> i64 {
        self.uploaded_at_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LicenseValidationError {
    #[error("the uploaded filename is invalid")]
    InvalidFilename,
    #[error("the license title is invalid")]
    InvalidTitle,
    #[error("the license slug is invalid")]
    InvalidSlug,
    #[error("the license file is empty")]
    EmptyBody,
    #[error("the license file is not valid UTF-8")]
    InvalidUtf8,
    #[error("the license file exceeds 1 MiB")]
    BodyTooLarge,
}

fn validate_filename(filename: &str) -> Result<(), LicenseValidationError> {
    let invalid = filename.is_empty()
        || filename.len() > MAX_FILENAME_BYTES
        || filename == "."
        || filename == ".."
        || filename.contains(['/', '\\'])
        || filename.chars().any(char::is_control);
    if invalid {
        return Err(LicenseValidationError::InvalidFilename);
    }
    Ok(())
}

fn derive_title(
    filename: &str,
    requested: Option<&str>,
) -> Result<String, LicenseValidationError> {
    let value = requested.unwrap_or_else(|| filename_stem(filename)).trim();
    let invalid = value.is_empty()
        || value.chars().count() > MAX_TITLE_CHARS
        || value.chars().any(char::is_control);
    if invalid {
        return Err(LicenseValidationError::InvalidTitle);
    }
    Ok(value.to_string())
}

fn filename_stem(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .map_or(filename, |(stem, _extension)| stem)
}

fn derive_slug(
    filename: &str,
    requested: Option<&str>,
    digest: &str,
) -> Result<String, LicenseValidationError> {
    if let Some(value) = requested {
        let trimmed = value.trim();
        let allowed = !trimmed.is_empty()
            && !trimmed.contains("..")
            && trimmed
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || " ._-".contains(character));
        if !allowed {
            return Err(LicenseValidationError::InvalidSlug);
        }
        let normalized = slugify(trimmed);
        return valid_slug(normalized);
    }

    let normalized = slugify(filename_stem(filename));
    if normalized.is_empty() {
        return Ok(format!("license-{}", &digest[..DIGEST_SLUG_LENGTH]));
    }
    valid_slug(normalized)
}

fn valid_slug(slug: String) -> Result<String, LicenseValidationError> {
    if slug.is_empty() || slug.len() > 96 {
        return Err(LicenseValidationError::InvalidSlug);
    }
    Ok(slug)
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator_pending = false;
        } else if !slug.is_empty() {
            separator_pending = true;
        }
    }
    slug
}
