# rfd in Cady

The `cady` branch of this fork is [rfd](https://github.com/PolyMeilex/rfd)
0.17.2 (upstream commit `cca0cfa5fc38c2752c9c98f9f5168904a515fd55`) with
the changes below, licensed under MIT (`LICENSE`). Cady's root `Cargo.toml`
takes it through `[patch.crates-io]` as a git dependency pinned to a commit
on this branch.

## Changes

Cady's Save As lets the user pick the file format in the native dialog and
needs to know which one was picked. rfd reports only the chosen path, so the
fork adds one API and leaves the existing ones as they are:

- `AsyncFileDialog::save_file_with_filter()` returns the chosen file and the
  index of the filter selected when the dialog closed, or `None` when the
  dialog does not report it. The path's extension does not stand in for a
  missing index: several filters can list the same extension, as Cady's DXF
  versions all list `dxf`, so the caller decides what an unknown filter
  means.
  `AsyncFileDialog::set_filter_label()` sets the label of the macOS format
  menu.
  - macOS: the save panel gets an accessory view with the label and an
    `NSPopUpButton` of the filter names (`FormatMenu` in
    `backend/macos/file_dialog/panel_ffi.rs`). Picking a filter sets that
    filter's extensions, lowercased, as `allowedFileTypes` and swaps the
    extension in the name field. AppKit keeps an extension in the name field
    only when it is an allowed type or a UTI the application exports, and it
    compares the lowercased extension to the allowed list case-sensitively.
    `allowsOtherFileTypes` stays at its default of NO. When the user clicks
    Save, AppKit appends the selected format's extension or shows its own
    warning. The name the dialog returns already ends in an allowed
    extension, so Cady's replace question, which asks only when Cady itself
    renamed the path onto an existing file, does not appear on macOS. An
    entry such as `plan.ncz` with DXF selected becomes `plan.ncz.dxf`, or
    AppKit shows its warning; it does not become `plan.dxf`.
  - Linux (XDG portal): the portal response parser also reads
    `current_filter`, matched to a filter by its label. A response without
    it, or naming no filter of the dialog, and the zenity fallback, which
    prints only the path, report `None`.
  - Windows: `IFileDialog::GetFileTypeIndex` after the dialog closes, or
    `None` when that call fails.
  - Not available with the `gtk3` feature or on WASM32.
- `path_with_filter_extension` is the rule the macOS format menu renames
  the name field with: a name keeps an extension of the chosen filter, an
  extension of another filter is replaced, and any other name gets the
  chosen filter's first extension appended. Cady applies the same function
  to every path its save dialogs return. On macOS the returned path already
  ends in the selected extension, so the call leaves it unchanged.
- `src/save_filter.rs` holds the platform-independent helpers and their
  tests.
- `FormatMenu::attach` carries `#[expect(unsafe_code)]`. `unsafe` there is
  the class definition, its `init`, and the popup's target and action. Each
  block has a `// SAFETY:` comment. `apply_format_menu` calls
  `nameFieldStringValue`, `setNameFieldStringValue`, and
  `setAllowedFileTypes`, which are safe methods in the objc2 version Cady
  locks, so that function has no `unsafe`. `setAllowedFileTypes` is
  deprecated; the call carries `#[expect(deprecated)]`.
- `Cargo.toml` enables the `NSPopUpButton` and `NSTextField` features of
  `objc2-app-kit`. The `deprecated` and `unused_unsafe` allows sit on the
  upstream backend modules, not on the crate, so the format menu is still
  checked. The `objc2-app-kit` requirement is 0.3.2, the first release
  where the panel and pop-up button methods used here are safe to call.
- `Cargo.lock` locks objc2, objc2-app-kit, objc2-foundation,
  objc2-core-foundation, block2 and dispatch2 at the versions Cady locks, so
  the fork's tests build the macOS backend against the same API as Cady.

`git diff cca0cfa5fc38c2752c9c98f9f5168904a515fd55 cady` shows every change
against upstream.

## Tests

The upstream `rust.yml` and `cargo-deny.yml` workflows also run on pushes
to `cady`, so the tests build and run on Linux (GTK and XDG portal),
Windows, macOS and WASM32. Locally:

```sh
cargo test --lib --no-default-features --features xdg-portal
```
