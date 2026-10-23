//! URL validation and local redirect unwrapping.

use std::collections::HashSet;

use url::Url;

const MAX_URL_BYTES: usize = 16 * 1024;
const MAX_REDIRECTS: usize = 6;

#[derive(Clone, Debug)]
pub struct PreparedUrl {
    pub original: Url,
    pub effective: Url,
    pub destination: String,
    pub document_type: Option<&'static str>,
}

impl PreparedUrl {
    pub fn parse(input: &str, unwrap_redirects: bool) -> Result<Self, String> {
        if input.len() > MAX_URL_BYTES {
            return Err(format!("URL is longer than {MAX_URL_BYTES} bytes"));
        }
        let original = Url::parse(input.trim()).map_err(|error| format!("invalid URL: {error}"))?;
        validate_scheme(&original)?;

        let effective = if unwrap_redirects {
            unwrap(original.clone())
        } else {
            original.clone()
        };
        let destination = effective
            .host_str()
            .map(str::to_owned)
            .unwrap_or_else(|| effective.path().to_owned());
        let document_type = office_document_type(effective.path());

        Ok(Self {
            original,
            effective,
            destination,
            document_type,
        })
    }
}

pub fn validate_scheme(url: &Url) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" | "file" => Ok(()),
        scheme => Err(format!("unsupported URL scheme: {scheme}")),
    }
}

fn unwrap(mut url: Url) -> Url {
    let mut seen = HashSet::new();
    for _ in 0..MAX_REDIRECTS {
        if !seen.insert(url.as_str().to_owned()) {
            break;
        }
        let Some(next) = redirect_target(&url) else {
            break;
        };
        let Ok(parsed) = Url::parse(&next) else {
            break;
        };
        if !matches!(parsed.scheme(), "http" | "https") || parsed == url {
            break;
        }
        url = parsed;
    }
    url
}

fn redirect_target(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    let is_safelinks = host == "safelinks.protection.outlook.com"
        || host.ends_with(".safelinks.protection.outlook.com");
    let is_google_redirect = host == "google.com"
        || host.ends_with(".google.com")
        || host == "google.co.uk"
        || host.ends_with(".google.co.uk");
    if !is_safelinks && !(is_google_redirect && url.path() == "/url") {
        return None;
    }

    let keys = if is_safelinks {
        ["url", "q"]
    } else {
        ["url", "q"]
    };
    url.query_pairs().find_map(|(key, value)| {
        keys.iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
            .then(|| value.into_owned())
    })
}

fn office_document_type(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "doc" | "docx" | "docm" | "dot" | "dotx" => Some("Word document"),
        "xls" | "xlsx" | "xlsm" | "xlt" | "xltx" => Some("Excel spreadsheet"),
        "ppt" | "pptx" | "pptm" | "pot" | "potx" => Some("PowerPoint presentation"),
        _ => None,
    }
}
