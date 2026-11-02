//! Platform-independent parts of
//! [`AsyncFileDialog::save_file_with_filter`](crate::AsyncFileDialog::save_file_with_filter).
//!
//! Each backend uses only its own helpers; they live together so their tests
//! run on every platform.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[cfg(any(
    all(
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
        ),
        not(feature = "gtk3"),
    ),
    target_os = "windows",
    test,
))]
use crate::file_dialog::Filter;

/// The index of the first of `filters` listing the extension of `path`,
/// ignoring ASCII case, or 0 when none does.
#[cfg(any(
    all(
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
        ),
        not(feature = "gtk3"),
    ),
    target_os = "windows",
    test,
))]
pub(crate) fn filter_index_for_path(filters: &[Filter], path: &Path) -> usize {
    path.extension()
        .and_then(|extension| {
            filters.iter().position(|filter| {
                filter
                    .extensions
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
        })
        .unwrap_or(0)
}

/// The index of the filter the XDG portal reported as `current_filter`,
/// matched by its label. The portal may leave `current_filter` out of its
/// response; the extension of `path` decides then.
#[cfg(any(
    all(
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
        ),
        not(feature = "gtk3"),
    ),
    test,
))]
pub(crate) fn filter_index_from_portal(
    filters: &[Filter],
    current_filter: Option<&str>,
    path: &Path,
) -> usize {
    current_filter
        .and_then(|label| filters.iter().position(|filter| filter.name == label))
        .unwrap_or_else(|| filter_index_for_path(filters, path))
}

/// The filter index of `IFileDialog::GetFileTypeIndex`, which counts the
/// file types from 1.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn filter_index_from_file_type_index(file_type_index: u32) -> usize {
    usize::try_from(file_type_index.saturating_sub(1)).unwrap_or(0)
}

/// The filter index of a Windows save the user confirmed.
///
/// `file_type_index` is `IFileDialog::GetFileTypeIndex` when that call
/// succeeds. When it fails, the extension of `path` picks the filter, as
/// the Linux portal does when it reports no current filter.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn filter_index_after_confirmed_save(
    file_type_index: Option<u32>,
    filters: &[Filter],
    path: &Path,
) -> usize {
    match file_type_index {
        Some(index) => filter_index_from_file_type_index(index),
        None => filter_index_for_path(filters, path),
    }
}

/// `path` ending in an extension of `filter_extensions[chosen]`, the
/// extensions of the filter chosen in a save dialog:
/// - a path ending in one of the chosen filter's extensions, ignoring ASCII
///   case, stays as it is;
/// - an extension of another filter is replaced by the chosen filter's first
///   extension, as `plan.ncz` saved as DXF becomes `plan.dxf`;
/// - any other path gets that extension appended, as `plan.txt` saved as DXF
///   becomes `plan.txt.dxf`.
///
/// A path without a file name, and a `chosen` filter that does not exist or
/// has no extension, leave `path` as it is.
///
/// The macOS format menu renames the name field with this rule, after
/// lowercasing the chosen filter's extensions. On macOS, AppKit then appends
/// the selected format's extension or shows its own warning before the
/// dialog returns, so the path a caller sees already ends in an allowed
/// extension and this rule leaves it unchanged. Linux and Windows return the
/// name as the user left it, and the caller applies this rule.
pub fn path_with_filter_extension<F, E>(
    path: &Path,
    filter_extensions: &[F],
    chosen: usize,
) -> PathBuf
where
    F: AsRef<[E]>,
    E: AsRef<str>,
{
    let Some(chosen) = filter_extensions.get(chosen).map(AsRef::as_ref) else {
        return path.to_owned();
    };
    let Some(default) = chosen.first() else {
        return path.to_owned();
    };
    if path.file_name().is_none() {
        return path.to_owned();
    }
    let listed = |extensions: &[E], existing: &OsStr| {
        extensions
            .iter()
            .any(|extension| existing.eq_ignore_ascii_case(extension.as_ref()))
    };
    match path.extension() {
        Some(existing) if listed(chosen, existing) => path.to_owned(),
        Some(existing)
            if filter_extensions
                .iter()
                .any(|extensions| listed(extensions.as_ref(), existing)) =>
        {
            path.with_extension(default.as_ref())
        }
        _ => {
            let mut appended = path.as_os_str().to_owned();
            appended.push(".");
            appended.push(default.as_ref());
            PathBuf::from(appended)
        }
    }
}

/// The allowed file types and name field for one choice in the macOS save
/// panel's format menu.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacosFormatMenuSelection {
    /// Lowercase extensions for `NSSavePanel.allowedFileTypes`. Never empty.
    pub(crate) allowed_types: Vec<String>,
    /// The name field, including the chosen filter's extension.
    pub(crate) name: PathBuf,
}

/// What the macOS format menu tells the save panel for `chosen`.
///
/// AppKit shows an extension in the name field only when `allowedFileTypes`
/// lists it, or when the application exports that extension as a UTI. The
/// panel lowercases the name's extension and compares that string to the
/// allowed list with a case-sensitive match, so an uppercase entry such as
/// `NCZ` never matches and `setNameFieldStringValue` drops it. The allowed
/// list and the extension this appends are therefore lowercase. An extension
/// the name already has, ignoring ASCII case, keeps its case.
///
/// Returns `None` when `chosen` is not a filter or that filter has no
/// extension. An empty `allowedFileTypes` array makes the panel raise.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_format_menu_selection<F, E>(
    name: &Path,
    filter_extensions: &[F],
    chosen: usize,
) -> Option<MacosFormatMenuSelection>
where
    F: AsRef<[E]>,
    E: AsRef<str>,
{
    let mut allowed_types = Vec::new();
    for extension in filter_extensions.get(chosen)?.as_ref() {
        let extension = extension.as_ref().to_ascii_lowercase();
        if extension.is_empty() || allowed_types.contains(&extension) {
            continue;
        }
        allowed_types.push(extension);
    }
    if allowed_types.is_empty() {
        return None;
    }

    let mut filters: Vec<Vec<String>> = filter_extensions
        .iter()
        .map(|extensions| {
            extensions
                .as_ref()
                .iter()
                .map(|extension| extension.as_ref().to_owned())
                .collect()
        })
        .collect();
    filters[chosen].clone_from(&allowed_types);
    Some(MacosFormatMenuSelection {
        name: path_with_filter_extension(name, &filters, chosen),
        allowed_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters() -> Vec<Filter> {
        [
            ("Cady Project (.cady)", &["cady"][..]),
            ("NCZ (.ncz)", &["NCZ"][..]),
            ("DXF (.dxf)", &["dxf", "DXF"][..]),
        ]
        .into_iter()
        .map(|(name, extensions)| Filter {
            name: name.to_owned(),
            extensions: extensions
                .iter()
                .map(|extension| extension.to_string())
                .collect(),
        })
        .collect()
    }

    #[test]
    fn portal_current_filter_picks_the_filter_by_label() {
        assert_eq!(
            filter_index_from_portal(&filters(), Some("DXF (.dxf)"), Path::new("/plan.cady")),
            2
        );
        assert_eq!(
            filter_index_from_portal(&filters(), Some("NCZ (.ncz)"), Path::new("/plan")),
            1
        );
    }

    #[test]
    fn without_portal_current_filter_the_extension_picks_the_filter() {
        let filters = filters();
        for (path, index) in [
            ("/plan.ncz", 1),
            ("/plan.DXF", 2),
            ("/plan.cady", 0),
            ("/plan", 0),
            ("/plan.txt", 0),
        ] {
            assert_eq!(
                filter_index_from_portal(&filters, None, Path::new(path)),
                index,
                "{path}"
            );
        }
        assert_eq!(
            filter_index_from_portal(&filters, Some("Unknown"), Path::new("/plan.dxf")),
            2
        );
    }

    #[test]
    fn windows_file_type_index_counts_from_one() {
        assert_eq!(filter_index_from_file_type_index(1), 0);
        assert_eq!(filter_index_from_file_type_index(3), 2);
        assert_eq!(filter_index_from_file_type_index(0), 0);
    }

    #[test]
    fn a_missing_windows_file_type_index_uses_the_path_extension() {
        let filters = filters();
        assert_eq!(
            filter_index_after_confirmed_save(Some(1), &filters, Path::new("plan.dxf")),
            0
        );
        assert_eq!(
            filter_index_after_confirmed_save(None, &filters, Path::new("plan.dxf")),
            2
        );
        assert_eq!(
            filter_index_after_confirmed_save(None, &filters, Path::new("plan.ncz")),
            1
        );
        assert_eq!(
            filter_index_after_confirmed_save(None, &filters, Path::new("plan")),
            0
        );
    }

    #[test]
    fn the_chosen_filter_decides_the_extension() {
        let extensions = [&["cady"][..], &["NCZ"], &["dxf", "DXF"]];
        for (path, chosen, expected) in [
            ("plan.cady", 2, "plan.dxf"),
            ("plan.dxf", 1, "plan.NCZ"),
            ("plan.NCZ", 0, "plan.cady"),
            ("plan.ncz", 1, "plan.ncz"),
            ("plan.DXF", 2, "plan.DXF"),
            ("plan.v2.ncz", 2, "plan.v2.dxf"),
            ("plan", 2, "plan.dxf"),
            ("plan", 0, "plan.cady"),
            ("plan.txt", 2, "plan.txt.dxf"),
            ("plan.", 0, "plan..cady"),
            (".cady", 2, ".cady.dxf"),
            ("folder.with.dot/plan", 0, "folder.with.dot/plan.cady"),
            ("", 2, ""),
            ("plan.dxf", 7, "plan.dxf"),
        ] {
            assert_eq!(
                path_with_filter_extension(Path::new(path), &extensions, chosen),
                Path::new(expected),
                "{path} -> {chosen}"
            );
        }
        assert_eq!(
            path_with_filter_extension(Path::new("plan"), &[] as &[&[&str]], 0),
            Path::new("plan")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_extension_rule_keeps_non_utf8_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let path = PathBuf::from(OsString::from_vec(b"plan-\xff".to_vec()));
        let renamed = path_with_filter_extension(&path, &[&["cady"][..]], 0);
        assert_eq!(renamed.as_os_str().as_bytes(), b"plan-\xff.cady");
    }

    #[test]
    fn the_save_panel_format_menu_uses_a_lowercase_extension() {
        let extensions = [&["cady"][..], &["NCZ"], &["dxf", "DXF"]];
        for (name, chosen, allowed, expected) in [
            ("one", 0, &["cady"][..], "one.cady"),
            ("one", 1, &["ncz"], "one.ncz"),
            ("one", 2, &["dxf"], "one.dxf"),
            ("replace-target.txt", 2, &["dxf"], "replace-target.txt.dxf"),
            ("one.cady", 1, &["ncz"], "one.ncz"),
            ("one.ncz", 2, &["dxf"], "one.dxf"),
            ("plan.NCZ", 1, &["ncz"], "plan.NCZ"),
            ("plan.DXF", 2, &["dxf"], "plan.DXF"),
            ("plan.txt", 2, &["dxf"], "plan.txt.dxf"),
        ] {
            let selection = macos_format_menu_selection(Path::new(name), &extensions, chosen)
                .expect("the filter has an extension");
            assert_eq!(selection.allowed_types, allowed, "{name} -> {chosen}");
            assert_eq!(selection.name, Path::new(expected), "{name} -> {chosen}");
        }
        assert!(macos_format_menu_selection(Path::new("one"), &[&[] as &[&str]], 0).is_none());
        assert!(macos_format_menu_selection(Path::new("one"), &extensions, 3).is_none());
    }
}
