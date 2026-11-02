use core::str;
use std::path::Path;
use std::path::PathBuf;

use block2::Block;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSModalResponse, NSOpenPanel, NSPopUpButton, NSSavePanel, NSTextField, NSView, NSWindow,
    NSWindowLevel,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use raw_window_handle::RawWindowHandle;

use super::super::{
    modal_future::{AsModal, InnerModal},
    utils::{FocusManager, PolicyManager},
};
use crate::backend::macos::utils::window_from_raw_window_handle;
use crate::save_filter::macos_format_menu_selection;
use crate::FileDialog;

extern "C" {
    pub fn CGShieldingWindowLevel() -> i32;
}

pub struct Panel {
    // Either `NSSavePanel` or the subclass `NSOpenPanel`
    pub(crate) panel: Retained<NSSavePanel>,
    parent: Option<Retained<NSWindow>>,
    /// The format menu of [`Panel::build_save_file_with_filter`]. A control
    /// does not retain its target, so the panel keeps it alive.
    format_menu: Option<Retained<FormatMenu>>,
    _focus_manager: FocusManager,
    _policy_manager: PolicyManager,
}

impl AsModal for Panel {
    fn inner_modal(&self) -> &(impl InnerModal + 'static) {
        &*self.panel
    }
}

#[allow(unused_unsafe)]
impl InnerModal for NSSavePanel {
    fn begin_modal(&self, window: &NSWindow, handler: &Block<dyn Fn(NSModalResponse)>) {
        unsafe { self.beginSheetModalForWindow_completionHandler(window, handler) }
    }

    fn run_modal(&self) -> NSModalResponse {
        unsafe { self.runModal() }
    }
}

#[allow(unused_unsafe)]
impl Panel {
    fn as_open_panel(&self) -> Option<&NSOpenPanel> {
        self.panel.downcast_ref::<NSOpenPanel>()
    }

    pub fn new(panel: Retained<NSSavePanel>, parent: Option<&RawWindowHandle>) -> Self {
        let _policy_manager = PolicyManager::new(MainThreadMarker::from(&*panel));

        let _focus_manager = FocusManager::new(MainThreadMarker::from(&*panel));

        panel.setLevel(unsafe { CGShieldingWindowLevel() } as NSWindowLevel);
        Self {
            panel,
            parent: parent.map(window_from_raw_window_handle),
            format_menu: None,
            _focus_manager,
            _policy_manager,
        }
    }

    pub fn run_modal(&self) -> NSModalResponse {
        if let Some(parent) = self.parent.clone() {
            let completion = block2::StackBlock::new(|_: isize| {});

            unsafe {
                self.panel
                    .beginSheetModalForWindow_completionHandler(&parent, &completion)
            }
        }

        unsafe { self.panel.runModal() }
    }

    pub fn get_result(&self) -> PathBuf {
        unsafe {
            let url = self.panel.URL().unwrap();
            url.path().unwrap().to_string().into()
        }
    }

    /// The index of the filter selected in the format menu, or 0 for a
    /// panel without one.
    pub fn selected_filter(&self) -> usize {
        self.format_menu
            .as_ref()
            .and_then(|menu| usize::try_from(menu.selected_index()).ok())
            .unwrap_or(0)
    }

    pub fn get_results(&self) -> Vec<PathBuf> {
        unsafe {
            let urls = self.as_open_panel().unwrap().URLs();

            let mut res = Vec::new();
            for url in urls {
                res.push(url.path().unwrap().to_string().into());
            }

            res
        }
    }
}

#[allow(unused_unsafe)]
trait PanelExt {
    fn panel(&self) -> &NSSavePanel;

    fn set_can_create_directories(&self, can: bool) {
        unsafe { self.panel().setCanCreateDirectories(can) }
    }

    fn add_filters(&self, opt: &FileDialog) {
        let mut exts: Vec<String> = Vec::new();

        for filter in opt.filters.iter() {
            exts.append(&mut filter.extensions.to_vec());
        }

        let f_raw: Vec<_> = exts.iter().map(|ext| NSString::from_str(ext)).collect();
        let array = NSArray::from_retained_slice(&f_raw);

        unsafe {
            #[allow(deprecated)]
            self.panel().setAllowedFileTypes(Some(&array));
        }
    }

    fn set_path(&self, path: &Path, file_name: Option<&str>) {
        // if file_name is some, and path is a dir
        let path = if let (Some(name), true) = (file_name, path.is_dir()) {
            let mut path = path.to_owned();
            // add a name to the end of path
            path.push(name);
            path
        } else {
            path.to_owned()
        };

        if let Some(path) = path.to_str() {
            unsafe {
                let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true);
                self.panel().setDirectoryURL(Some(&url));
            }
        }
    }

    fn set_file_name(&self, file_name: &str) {
        unsafe {
            self.panel()
                .setNameFieldStringValue(&NSString::from_str(file_name))
        }
    }

    fn set_title(&self, title: &str) {
        unsafe { self.panel().setMessage(Some(&NSString::from_str(title))) }
    }
}

impl PanelExt for Retained<NSSavePanel> {
    fn panel(&self) -> &NSSavePanel {
        &self
    }
}

impl PanelExt for Retained<NSOpenPanel> {
    fn panel(&self) -> &NSSavePanel {
        &self
    }
}

#[allow(unused_unsafe)]
impl Panel {
    pub fn build_pick_file(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(file_name) = &opt.file_name {
            panel.set_file_name(file_name);
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        unsafe { panel.setCanChooseDirectories(false) };
        unsafe { panel.setCanChooseFiles(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_save_file(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSSavePanel::savePanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(file_name) = &opt.file_name {
            panel.set_file_name(file_name);
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        Self::new(panel, opt.parent.as_ref())
    }

    /// A save panel offering `opt.filters` in a format menu below the file
    /// browser. Picking a filter sets that filter's extensions, lowercased,
    /// as the panel's allowed file types and renames the name field.
    /// `allowsOtherFileTypes` stays NO, so Save appends the selected
    /// format's extension or AppKit shows its own warning.
    ///
    /// AppKit keeps an extension in the name field only when
    /// `allowedFileTypes` lists it, or when the application exports it as a
    /// UTI. The panel lowercases the name's extension and compares that
    /// string to the allowed list case-sensitively, so the list is lowercase.
    pub fn build_save_file_with_filter(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = NSSavePanel::savePanel(mtm);

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(file_name) = &opt.file_name {
            panel.set_file_name(file_name);
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        panel.setExtensionHidden(false);

        let format_menu = (!opt.filters.is_empty()).then(|| FormatMenu::attach(&panel, opt, mtm));

        let mut panel = Self::new(panel, opt.parent.as_ref());
        panel.format_menu = format_menu;
        panel
    }

    pub fn build_pick_folder(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(false) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_folders(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(false) };
        unsafe { panel.setAllowsMultipleSelection(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_files(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        unsafe { panel.setCanChooseDirectories(false) };
        unsafe { panel.setCanChooseFiles(true) };
        unsafe { panel.setAllowsMultipleSelection(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_file_or_folder(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(true) };
        unsafe { panel.setAllowsMultipleSelection(false) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_files_or_folders(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(true) };
        unsafe { panel.setAllowsMultipleSelection(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }
}

struct FormatMenuIvars {
    panel: Retained<NSSavePanel>,
    popup: Retained<NSPopUpButton>,
    /// The extensions of each filter, in menu order.
    extensions: Vec<Vec<String>>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - `FormatMenu` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RFDSavePanelFormatMenu"]
    #[ivars = FormatMenuIvars]
    struct FormatMenu;

    unsafe impl NSObjectProtocol for FormatMenu {}

    impl FormatMenu {
        // SAFETY: an `NSControl` action receives the sender as an object
        // pointer and returns nothing, which this signature matches.
        #[unsafe(method(formatChanged:))]
        fn format_changed(&self, _sender: Option<&AnyObject>) {
            let Ok(index) = usize::try_from(self.selected_index()) else {
                return;
            };
            apply_format_menu(&self.ivars().panel, &self.ivars().extensions, index);
        }
    }
);

impl FormatMenu {
    /// Adds a menu of `opt.filters`, labelled by `opt.filter_label`, as the
    /// accessory view of `panel`, with the first filter selected.
    #[deny(unsafe_code)]
    #[expect(
        unsafe_code,
        reason = "objc2 marks NSObject init and the popup target and action unsafe"
    )]
    fn attach(
        panel: &Retained<NSSavePanel>,
        opt: &FileDialog,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let popup =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        for filter in &opt.filters {
            popup.addItemWithTitle(&NSString::from_str(&filter.name));
        }
        popup.selectItemAtIndex(0);
        let accessory = format_accessory(opt.filter_label.as_deref(), &popup, mtm);
        let menu = Self::alloc(mtm).set_ivars(FormatMenuIvars {
            panel: panel.clone(),
            popup,
            extensions: opt
                .filters
                .iter()
                .map(|filter| filter.extensions.clone())
                .collect(),
        });
        // SAFETY: `NSObject`'s `init` takes no arguments and returns the
        // initialized object, which `Retained<Self>` takes ownership of.
        let menu: Retained<Self> = unsafe { msg_send![super(menu), init] };
        // SAFETY: the popup does not retain its target. `menu` stays alive
        // while the panel shows the popup, because `Panel::format_menu` owns
        // it, and `menu` implements the `formatChanged:` action.
        unsafe {
            menu.ivars().popup.setTarget(Some(&menu));
            menu.ivars().popup.setAction(Some(sel!(formatChanged:)));
        }
        panel.setAccessoryView(Some(&accessory));
        apply_format_menu(panel, &menu.ivars().extensions, 0);
        menu
    }

    fn selected_index(&self) -> isize {
        self.ivars().popup.indexOfSelectedItem()
    }
}

/// Points `panel` at the extensions of filter `index` and shows that
/// extension on the current name.
///
/// Allowed types are set before the name. Setting them afterwards makes the
/// panel replace the name's extension with the first allowed type.
///
/// `nameFieldStringValue`, `setNameFieldStringValue`, and
/// `setAllowedFileTypes` are safe methods in the objc2 version Cady locks.
fn apply_format_menu(panel: &NSSavePanel, extensions: &[Vec<String>], index: usize) {
    let name = panel.nameFieldStringValue().to_string();
    let Some(selection) = macos_format_menu_selection(Path::new(&name), extensions, index) else {
        return;
    };
    let allowed: Vec<_> = selection
        .allowed_types
        .iter()
        .map(|extension| NSString::from_str(extension))
        .collect();
    let array = NSArray::from_retained_slice(&allowed);
    // `array` holds at least one extension. An empty array makes
    // `setAllowedFileTypes` raise, and the panel copies the array.
    #[expect(
        deprecated,
        reason = "setAllowedFileTypes lists the extensions the name field may keep"
    )]
    panel.setAllowedFileTypes(Some(&array));
    if let Some(renamed) = selection.name.to_str() {
        if renamed != name {
            panel.setNameFieldStringValue(&NSString::from_str(renamed));
        }
    }
}

/// `label` and `popup` on one row, as the accessory view of a save panel.
fn format_accessory(
    label: Option<&str>,
    popup: &NSPopUpButton,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    const MARGIN: f64 = 12.0;
    const GAP: f64 = 8.0;

    let label = label.map(|label| NSTextField::labelWithString(&NSString::from_str(label), mtm));
    if let Some(label) = &label {
        label.sizeToFit();
    }
    popup.sizeToFit();

    let label_size = label
        .as_ref()
        .map_or(NSSize::ZERO, |label| label.frame().size);
    let popup_size = popup.frame().size;
    let gap = if label.is_some() { GAP } else { 0.0 };
    let height = label_size.height.max(popup_size.height) + 2.0 * MARGIN;
    let width = label_size.width + gap + popup_size.width + 2.0 * MARGIN;

    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(width, height)),
    );
    if let Some(label) = &label {
        label.setFrameOrigin(NSPoint::new(MARGIN, (height - label_size.height) / 2.0));
        view.addSubview(label);
    }
    popup.setFrameOrigin(NSPoint::new(
        MARGIN + label_size.width + gap,
        (height - popup_size.height) / 2.0,
    ));
    view.addSubview(popup);
    view
}
