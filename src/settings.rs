//! Окно настроек: боковая панель с разделами «Основные», «Расположение», «Разрешения».

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn,
    NSControlTextEditingDelegate, NSControlSize, NSFont, NSGridView, NSImage, NSImageView,
    NSProgressIndicator, NSProgressIndicatorStyle, NSLayoutAttribute,
    NSScrollView, NSSplitViewController, NSSplitViewItem, NSStackView, NSSwitch, NSTableCellView,
    NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow,
    NSWindowDidBecomeKeyNotification, NSWindowStyleMask, NSWorkspace,
};
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_graphics::CGPreflightScreenCaptureAccess;
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSNotificationCenter, NSSize, NSString,
    NSUserDefaults, NSURL,
};

use crate::editor::{EditorView, EDITOR_WIDTH};
use crate::strings::{self, Lang};

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const SCREEN_RECORDING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const SHOW_PANEL_KEY: &str = "showPanel";
const LAYOUT_PANE: usize = 1;

// HARDCODE: размеры окна настроек; вынести в конфиг позже.
const WINDOW_SIZE: NSSize = NSSize::new(780.0, 360.0);
const SIDEBAR_WIDTH: f64 = 200.0;
const SIDEBAR_ROW_HEIGHT: f64 = 28.0;
const CONTENT_INSET: f64 = 24.0;
const CONTENT_TOP: f64 = 48.0;
const SPACING: f64 = 12.0;
/// Зазор между значком и подписью в строке боковой панели.
const SIDEBAR_ICON_GAP: f64 = 6.0;
/// Отступ строки боковой панели от левого края.
const SIDEBAR_ROW_INSET: f64 = 4.0;

/// Показывать ли панель под строкой меню (по умолчанию да).
pub fn show_panel() -> bool {
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str(SHOW_PANEL_KEY);
    defaults.objectForKey(&key).is_none() || defaults.boolForKey(&key)
}

pub fn set_show_panel(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &NSString::from_str(SHOW_PANEL_KEY));
}

pub struct Settings {
    window: Retained<NSWindow>,
    _sidebar: Retained<Sidebar>,
    pub editor: Retained<EditorView>,
    show_panel: Retained<NSSwitch>,
    login: Retained<NSSwitch>,
    permissions: [(Retained<NSTextField>, Retained<NSButton>); 2],
    apply: Retained<NSButton>,
    progress: Retained<NSProgressIndicator>,
    progress_label: Retained<NSTextField>,
    lang: Lang,
}

impl Settings {
    /// `target` — контроллер: `onToggleShowPanel:`, `onToggleLogin:`, `onOpenAccessibility:`,
    /// `onOpenScreenRecording:`, `onSettingsFocus:`, `onApplyLayout:`.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Self {
        let show_panel = switch(mtm, target, sel!(onToggleShowPanel:));
        let login = switch(mtm, target, sel!(onToggleLogin:));
        let general = pane(
            mtm,
            &[&*grid(
                mtm,
                &[
                    [
                        &*label(mtm, strings::settings_show_panel(lang)) as &NSView,
                        &*show_panel,
                    ],
                    [&*label(mtm, strings::menu_login(lang)), &*login],
                    [
                        &*label(mtm, strings::settings_version(lang)),
                        &*label(mtm, env!("CARGO_PKG_VERSION")),
                    ],
                ],
            )],
        );

        let editor = EditorView::new(mtm, lang);
        editor.setTranslatesAutoresizingMaskIntoConstraints(false);
        editor
            .widthAnchor()
            .constraintEqualToConstant(EDITOR_WIDTH)
            .setActive(true);
        editor
            .heightAnchor()
            .constraintEqualToConstant(EditorView::height())
            .setActive(true);
        let hint = label(mtm, strings::editor_hint(lang));
        hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let apply = button(mtm, strings::editor_apply(lang), target, sel!(onApplyLayout:));
        let progress = NSProgressIndicator::new(mtm);
        progress.setStyle(NSProgressIndicatorStyle::Spinning);
        progress.setControlSize(NSControlSize::Small);
        progress.setIndeterminate(true);
        progress.setDisplayedWhenStopped(false);
        let progress_label = label(mtm, strings::editor_applying(lang));
        progress_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        progress_label.setHidden(true);
        let apply_row = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[&*apply as &NSView, &*progress, &*progress_label]),
            mtm,
        );
        apply_row.setSpacing(SPACING / 2.0);
        let layout = pane(mtm, &[&*hint as &NSView, &*editor, &*apply_row]);

        let permissions = [
            (
                status_label(mtm),
                button(mtm, strings::settings_allow(lang), target, sel!(onOpenAccessibility:)),
            ),
            (
                status_label(mtm),
                button(mtm, strings::settings_allow(lang), target, sel!(onOpenScreenRecording:)),
            ),
        ];
        let titles = [
            strings::settings_accessibility(lang),
            strings::settings_screen_recording(lang),
        ];
        let title_labels: Vec<_> = titles.iter().map(|title| label(mtm, title)).collect();
        let permission_rows: Vec<[&NSView; 3]> = permissions
            .iter()
            .zip(&title_labels)
            .map(|((status, button), title)| [&**title as &NSView, &**status, &**button])
            .collect();
        let access = pane(mtm, &[&*grid(mtm, &permission_rows)]);

        let content = NSView::new(mtm);
        let sidebar = Sidebar::new(mtm, lang, content.clone(), vec![general, layout, access]);
        let split = NSSplitViewController::new(mtm);
        let sidebar_item =
            NSSplitViewItem::sidebarWithViewController(&controller(mtm, &sidebar.table_view(mtm)));
        sidebar_item.setMinimumThickness(SIDEBAR_WIDTH);
        sidebar_item.setMaximumThickness(SIDEBAR_WIDTH);
        split.addSplitViewItem(&sidebar_item);
        split.addSplitViewItem(&NSSplitViewItem::splitViewItemWithViewController(
            &controller(mtm, &content),
        ));

        let window = NSWindow::windowWithContentViewController(&split);
        window.setStyleMask(
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::FullSizeContentView,
        );
        window.setTitlebarAppearsTransparent(true);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(strings::settings_title(lang)));
        window.setContentSize(WINDOW_SIZE);
        window.center();
        sidebar.select(0);

        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                target,
                sel!(onSettingsFocus:),
                Some(NSWindowDidBecomeKeyNotification),
                Some(&window),
            );
        }
        Self {
            window,
            _sidebar: sidebar,
            editor,
            show_panel,
            login,
            permissions,
            apply,
            progress,
            progress_label,
            lang,
        }
    }

    /// «Применяю изменения…»: кнопка недоступна, рядом крутится индикатор.
    pub fn set_applying(&self, applying: bool) {
        self.apply.setEnabled(!applying);
        self.progress_label.setHidden(!applying);
        unsafe {
            if applying {
                self.progress.startAnimation(None);
            } else {
                self.progress.stopAnimation(None);
            }
        }
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        self.refresh();
        NSApplication::sharedApplication(mtm).activate();
        self.window.makeKeyAndOrderFront(None);
    }

    /// Перечитывает состояние: разрешения могли выдать в Системных настройках.
    pub fn refresh(&self) {
        set_switch(&self.show_panel, show_panel());
        set_switch(&self.login, crate::login::is_enabled());
        let granted = [
            unsafe { AXIsProcessTrusted() },
            CGPreflightScreenCaptureAccess(),
        ];
        for ((status, button), granted) in self.permissions.iter().zip(granted) {
            let (text, color) = if granted {
                (
                    strings::settings_granted(self.lang),
                    NSColor::systemGreenColor(),
                )
            } else {
                (
                    strings::settings_denied(self.lang),
                    NSColor::systemRedColor(),
                )
            };
            status.setStringValue(&NSString::from_str(text));
            status.setTextColor(Some(&color));
            button.setHidden(granted);
        }
    }
}

/// Открывает раздел «Конфиденциальность и безопасность» с нужным разрешением.
pub fn open_accessibility_pane() {
    open_url(ACCESSIBILITY_PANE);
}

pub fn open_screen_recording_pane() {
    open_url(SCREEN_RECORDING_PANE);
}

fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

pub struct SidebarIvars {
    titles: Vec<(&'static str, &'static str)>,
    content: Retained<NSView>,
    panes: Vec<Retained<NSView>>,
    table: RefCell<Option<Retained<NSTableView>>>,
}

define_class!(
    /// Список разделов слева; выбор строки показывает её раздел справа.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SidebarIvars]
    pub struct Sidebar;

    unsafe impl NSObjectProtocol for Sidebar {}

    unsafe impl NSControlTextEditingDelegate for Sidebar {}

    unsafe impl NSTableViewDataSource for Sidebar {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            self.ivars().titles.len() as NSInteger
        }
    }

    unsafe impl NSTableViewDelegate for Sidebar {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            self.cell(row as usize)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            let Some(table) = notification
                .object()
                .and_then(|o| o.downcast::<NSTableView>().ok())
            else {
                return;
            };
            let row = table.selectedRow();
            if row >= 0 {
                self.show_pane(row as usize);
            }
        }
    }
);

impl Sidebar {
    /// Строка раздела: символ SF и название.
    fn cell(&self, row: usize) -> Option<Retained<NSView>> {
        let mtm = self.mtm();
        let &(symbol, title) = self.ivars().titles.get(row)?;
        let cell = NSTableCellView::new(mtm);
        let icon = NSImageView::new(mtm);
        icon.setImage(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                None,
            )
            .as_deref(),
        );
        let text = label(mtm, title);
        let stack = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[&*icon as &NSView, &*text]),
            mtm,
        );
        stack.setSpacing(SIDEBAR_ICON_GAP);
        cell.addSubview(&stack);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), SIDEBAR_ROW_INSET)
            .setActive(true);
        stack
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor())
            .setActive(true);
        // SAFETY: ячейка держит сильные ссылки на свои подвиды, оба добавлены в неё.
        unsafe {
            cell.setImageView(Some(&icon));
            cell.setTextField(Some(&text));
        }
        Some(Retained::into_super(cell))
    }

    fn new(
        mtm: MainThreadMarker,
        lang: Lang,
        content: Retained<NSView>,
        panes: Vec<Retained<NSView>>,
    ) -> Retained<Self> {
        let titles = vec![
            ("gearshape", strings::settings_general(lang)),
            ("menubar.rectangle", strings::settings_layout(lang)),
            ("lock.shield", strings::settings_permissions(lang)),
        ];
        let this = mtm.alloc().set_ivars(SidebarIvars {
            titles,
            content,
            panes,
            table: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Список-источник в прокрутке; таблица держит `self` как источник данных и делегат.
    fn table_view(&self, mtm: MainThreadMarker) -> Retained<NSScrollView> {
        let table = NSTableView::new(mtm);
        table.addTableColumn(&NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("section"),
        ));
        table.setHeaderView(None);
        table.setStyle(NSTableViewStyle::SourceList);
        table.setRowHeight(SIDEBAR_ROW_HEIGHT);
        let this: &AnyObject = self.as_ref();
        unsafe {
            let _: () = msg_send![&table, setDataSource: this];
            let _: () = msg_send![&table, setDelegate: this];
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        *self.ivars().table.borrow_mut() = Some(table);
        scroll.setDrawsBackground(false);
        scroll
    }

    /// Выделяет строку; раздел показывает `tableViewSelectionDidChange:`.
    fn select(&self, row: usize) {
        if let Some(table) = self.ivars().table.borrow().as_ref() {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        }
    }

    fn show_pane(&self, row: usize) {
        let content = &self.ivars().content;
        for view in content.subviews().iter() {
            view.removeFromSuperview();
        }
        let Some(pane) = self.ivars().panes.get(row) else {
            return;
        };
        content.addSubview(pane);
        pane.setTranslatesAutoresizingMaskIntoConstraints(false);
        pane.topAnchor()
            .constraintEqualToAnchor(&content.topAnchor())
            .setActive(true);
        pane.leadingAnchor()
            .constraintEqualToAnchor(&content.leadingAnchor())
            .setActive(true);
        pane.trailingAnchor()
            .constraintEqualToAnchor(&content.trailingAnchor())
            .setActive(true);
        pane.bottomAnchor()
            .constraintEqualToAnchor(&content.bottomAnchor())
            .setActive(true);
        if row == LAYOUT_PANE {
            notify_layout_shown(self.mtm());
        }
    }
}

/// Раздел «Расположение» открыт — контроллеру нужны свежие снимки иконок.
fn notify_layout_shown(mtm: MainThreadMarker) {
    if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
        let delegate: &AnyObject = delegate.as_ref();
        let _: () = unsafe { msg_send![delegate, onEditorNeedsIcons] };
    }
}

fn controller(mtm: MainThreadMarker, view: &NSView) -> Retained<NSViewController> {
    let controller = NSViewController::new(mtm);
    controller.setView(view);
    controller
}

/// Раздел: элементы друг под другом от левого верхнего угла.
fn pane(mtm: MainThreadMarker, views: &[&NSView]) -> Retained<NSView> {
    let pane = NSView::new(mtm);
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setSpacing(SPACING);
    pane.addSubview(&stack);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    stack
        .topAnchor()
        .constraintEqualToAnchor_constant(&pane.topAnchor(), CONTENT_TOP)
        .setActive(true);
    stack
        .leadingAnchor()
        .constraintEqualToAnchor_constant(&pane.leadingAnchor(), CONTENT_INSET)
        .setActive(true);
    pane
}

fn grid<const N: usize>(mtm: MainThreadMarker, rows: &[[&NSView; N]]) -> Retained<NSGridView> {
    let rows: Vec<Retained<NSArray<NSView>>> =
        rows.iter().map(|row| NSArray::from_slice(row)).collect();
    let grid = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&rows), mtm);
    grid.setRowSpacing(SPACING);
    grid.setColumnSpacing(SPACING * 2.0);
    grid
}

fn switch(mtm: MainThreadMarker, target: &AnyObject, action: Sel) -> Retained<NSSwitch> {
    let switch = NSSwitch::new(mtm);
    unsafe {
        switch.setTarget(Some(target));
        switch.setAction(Some(action));
    }
    switch
}

fn set_switch(switch: &NSSwitch, on: bool) {
    switch.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

fn status_label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let status = label(mtm, "");
    status.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
    status
}

fn button(mtm: MainThreadMarker, title: &str, target: &AnyObject, action: Sel) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    }
}
