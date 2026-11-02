# rfd in Cady

This directory is the crates.io package of [rfd](https://github.com/PolyMeilex/rfd)
0.17.2 (upstream commit `cca0cfa5fc38c2752c9c98f9f5168904a515fd55`, see
`.cargo_vcs_info.json`), licensed under MIT (`LICENSE`). The root
`Cargo.toml` uses it through `[patch.crates-io]` and keeps it out of the
workspace.

## Changes

Cady's Save As lets the user pick the file format in the native dialog and
needs to know which one was picked. rfd reports only the chosen path, so the
fork adds one API and leaves the existing ones as they are:

- `AsyncFileDialog::save_file_with_filter()` returns the chosen file and the
  index of the filter selected when the dialog closed.
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
    `current_filter`, matched to a filter by its label; without it, and in
    the zenity fallback, the path's extension picks the filter.
  - Windows: `IFileDialog::GetFileTypeIndex` after the dialog closes. If that
    call fails, the path's extension picks the filter, as the Linux portal
    does when it reports no current filter.
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
  checked. An empty `[workspace]` table makes the crate its own workspace,
  so Cargo does not look for an enclosing one when it runs the fork's tests.
- `Cargo.lock` locks objc2, objc2-app-kit, objc2-foundation,
  objc2-core-foundation, block2 and dispatch2 at the versions Cady locks, so
  the fork's tests build the macOS backend against the same API as Cady.

Diffing `vendor/rfd` against the commit
`chore(vendor): vendor rfd 0.17.2 unchanged` shows every change against
upstream.

## Tests

The fork is outside Cady's workspace, so nextest does not run its tests.
CI runs them on Linux in their own step:

```sh
pixi run cargo test --locked --manifest-path vendor/rfd/Cargo.toml --lib --no-default-features --features xdg-portal
```

Without Cady's winit, which loads libwayland at runtime, the `wayland`
feature links `wayland-client` through pkg-config; none of the tests need
it.

The portal parser tests run only on Linux and need `libdbus-1.so.3`.
