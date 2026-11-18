//! Filesystem paths used for document-local configuration discovery.

use std::path::PathBuf;

use lsp_types::Uri;

pub(crate) fn local_file_path(uri: &Uri) -> Option<PathBuf> {
    if !uri.scheme()?.as_str().eq_ignore_ascii_case("file")
        || !uri.path().is_absolute()
        || uri.authority().is_some_and(|authority| {
            let authority = authority.as_str();
            !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost")
        })
    {
        return None;
    }
    let decoded = uri.path().as_estr().decode();
    if decoded.as_bytes().contains(&0) {
        return None;
    }
    #[cfg(unix)]
    let path = {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        PathBuf::from(OsString::from_vec(decoded.into_bytes().into_owned()))
    };
    #[cfg(not(unix))]
    let path = {
        let decoded = decoded.into_string().ok()?;
        let text: &str = &decoded;
        #[cfg(windows)]
        let text = text.strip_prefix('/')?;
        PathBuf::from(text)
    };
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_paths_and_ignores_virtual_and_remote_uris() {
        #[cfg(unix)]
        let (uri, path) = (
            "file://localhost/tmp/%D1%82%D0%B5%D1%81%D1%82%20a%23b%2Bc.sql",
            "/tmp/тест a#b+c.sql",
        );
        #[cfg(windows)]
        let (uri, path) = (
            "file:///C:/%D1%82%D0%B5%D1%81%D1%82%20a%23b%2Bc.sql",
            "C:/тест a#b+c.sql",
        );
        #[cfg(any(unix, windows))]
        assert_eq!(
            local_file_path(&uri.parse().unwrap()),
            Some(PathBuf::from(path))
        );
        for uri in [
            "untitled:query.sql",
            "file://remote/tmp/query.sql",
            "file:///bad%00.sql",
        ] {
            assert_eq!(local_file_path(&uri.parse().unwrap()), None);
        }
    }
}
