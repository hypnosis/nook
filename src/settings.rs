//! Окно настроек: боковая панель с разделами «Основные», «Расположение», «Разрешения».

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSButton, NSControlSize, NSControlStateValueOff, NSControlStateValueOn,
    NSControlTextEditingDelegate, NSImage, NSImageView, NSLayoutAttribute, NSLayoutPriority,
    NSLayoutPriorityDragThatCanResizeWindow, NSProgressIndicator,
    NSProgressIndicatorStyle, NSScrollView, NSSplitViewController, NSSplitViewItem,
    NSSplitViewItemAccessoryViewController, NSStackView, NSSwitch, NSTableCellView,
    NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSSize, NSString, NSUserDefaults,
};

use crate::editor::EditorView;
use crate::permissions::PermissionRows;
use crate::strings::{self, Lang};
use crate::ui_style::{self, button, grid, label, wrapping_label, SPACING};

const SHOW_PANEL_KEY: &str = "showPanel";
const AUTOMATIC_LAYOUT_KEY: &str = "automaticLayout";
const LAYOUT_PANE: usize = 1;

// HARDCODE: размеры окна настроек; вынести в конфиг позже.
/// Ширина и наименьшая высота окна: выше оно становится, только если раздел не влезает.
const WINDOW_SIZE: NSSize = NSSize::new(640.0, 360.0);
/// Окно тянется по ширине и не уже этого.
const WINDOW_MIN_WIDTH: f64 = 600.0;
/// Высота окна держится наименьшей сильнее, чем её тянет мышь, но слабее подписей.
const FIT_PRIORITY: NSLayoutPriority = NSLayoutPriorityDragThatCanResizeWindow + 1.0;
const SIDEBAR_WIDTH: f64 = 200.0;
const SIDEBAR_ROW_HEIGHT: f64 = 28.0;
const CONTENT_INSET: f64 = 24.0;
/// Редактор занимает раздел от поля до поля.
const EDITOR_WIDTH: f64 = WINDOW_SIZE.width - SIDEBAR_WIDTH - CONTENT_INSET * 2.0;
const CONTENT_TOP: f64 = 48.0;
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

/// Панель показывает всё, что не поместилось в строку меню, без ручной раскладки (по умолчанию нет).
pub fn automatic_layout() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(AUTOMATIC_LAYOUT_KEY))
}

pub fn set_automatic_layout(on: bool) {
    NSUserDefaults::standardUserDefaults()
        .setBool_forKey(on, &NSString::from_str(AUTOMATIC_LAYOUT_KEY));
}

pub struct Settings {
    window: Retained<NSWindow>,
    sidebar: Retained<Sidebar>,
    pub editor: Retained<EditorView>,
    show_panel: Retained<NSSwitch>,
    login: Retained<NSSwitch>,
    automatic_layout: Retained<NSSwitch>,
    hint: Retained<NSTextField>,
    permissions: PermissionRows,
    apply_row: Retained<NSStackView>,
    apply: Retained<NSButton>,
    progress: Retained<NSProgressIndicator>,
    progress_label: Retained<NSTextField>,
    apply_note: Retained<NSTextField>,
}

impl Settings {
    /// `target` — контроллер: `onToggleShowPanel:`, `onToggleLogin:`, `onOpenAccessibility:`,
    /// `onOpenScreenRecording:`, `onApplyLayout:`.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Self {
        let show_panel = switch(mtm, target, sel!(onToggleShowPanel:));
        let login = switch(mtm, target, sel!(onToggleLogin:));
        let general = pane(
            mtm,
            &[&*grid(
                mtm,
                &[
                    [
                        &*wrapping_label(mtm, strings::settings_show_panel(lang)) as &NSView,
                        &*show_panel,
                    ],
                    [&*wrapping_label(mtm, strings::menu_login(lang)), &*login],
                ],
            )],
        );

        let automatic_layout = switch(mtm, target, sel!(onToggleAutomaticLayout:));
        let automatic_row = grid(
            mtm,
            &[[
                &*ui_style::title_with_detail(
                    mtm,
                    strings::settings_automatic_layout(lang),
                    strings::settings_automatic_layout_detail(lang),
                ) as &NSView,
                &*automatic_layout,
            ]],
        );
        let hint = wrapping_label(mtm, strings::editor_hint(lang));
        hint.setTextColor(Some(&ui_style::secondary_label_color()));
        let editor = EditorView::new(mtm, lang, EDITOR_WIDTH);
        let apply = button(
            mtm,
            strings::editor_apply(lang),
            target,
            sel!(onApplyLayout:),
        );
        let progress = NSProgressIndicator::new(mtm);
        progress.setStyle(NSProgressIndicatorStyle::Spinning);
        progress.setControlSize(NSControlSize::Small);
        progress.setIndeterminate(true);
        progress.setDisplayedWhenStopped(false);
        let progress_label = label(mtm, strings::editor_applying(lang));
        progress_label.setTextColor(Some(&ui_style::secondary_label_color()));
        progress_label.setHidden(true);
        let apply_row = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[&*apply as &NSView, &*progress, &*progress_label]),
            mtm,
        );
        apply_row.setSpacing(SPACING / 2.0);
        let apply_note = wrapping_label(mtm, "");
        apply_note.setTextColor(Some(&ui_style::secondary_label_color()));
        apply_note.setHidden(true);
        let layout = pane(
            mtm,
            &[&*automatic_row as &NSView, &*hint, &*editor, &*apply_row, &*apply_note],
        );
        editor
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&layout.trailingAnchor(), -CONTENT_INSET)
            .setActive(true);

        let permissions = PermissionRows::new(mtm, target, lang);
        let access = pane(mtm, &[permissions.view()]);

        let content = NSView::new(mtm);
        let sidebar = Sidebar::new(mtm, lang, content.clone(), vec![general, layout, access]);
        let split = NSSplitViewController::new(mtm);
        let sidebar_item =
            NSSplitViewItem::sidebarWithViewController(&controller(mtm, &sidebar.table_view(mtm)));
        sidebar_item.setMinimumThickness(SIDEBAR_WIDTH);
        sidebar_item.setMaximumThickness(SIDEBAR_WIDTH);
        sidebar_item.setCanCollapse(false);
        let version = label(
            mtm,
            &format!(
                "{} {}",
                strings::settings_version(lang),
                env!("CARGO_PKG_VERSION")
            ),
        );
        version.setFont(Some(&ui_style::footer_font()));
        version.setTextColor(Some(&ui_style::secondary_label_color()));
        let footer =
            NSStackView::stackViewWithViews(&NSArray::from_slice(&[&*version as &NSView]), mtm);
        let footer_controller = NSSplitViewItemAccessoryViewController::new(mtm);
        footer_controller.setView(&footer);
        sidebar_item.addBottomAlignedAccessoryViewController(&footer_controller);
        split.addSplitViewItem(&sidebar_item);
        split.addSplitViewItem(&NSSplitViewItem::splitViewItemWithViewController(
            &controller(mtm, &content),
        ));

        let window = NSWindow::windowWithContentViewController(&split);
        window.setStyleMask(
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Resizable
                | NSWindowStyleMask::FullSizeContentView,
        );
        window.setTitlebarAppearsTransparent(true);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(strings::settings_title(lang)));
        window.setContentSize(WINDOW_SIZE);
        window.setContentMinSize(NSSize::new(WINDOW_MIN_WIDTH, WINDOW_SIZE.height));
        window.center();
        sidebar.select(0);

        Self {
            window,
            sidebar,
            editor,
            show_panel,
            login,
            automatic_layout,
            hint,
            permissions,
            apply_row,
            apply,
            progress,
            progress_label,
            apply_note,
        }
    }

    /// «Применяю изменения…»: кнопка недоступна, рядом крутится индикатор, прошлое пояснение убрано.
    pub fn set_applying(&self, applying: bool) {
        if applying {
            self.set_apply_note(None);
        }
        self.apply.setEnabled(!applying);
        self.automatic_layout.setEnabled(!applying && show_panel());
        self.progress_label.setHidden(!applying);
        unsafe {
            if applying {
                self.progress.startAnimation(None);
            } else {
                self.progress.stopAnimation(None);
            }
        }
    }

    /// Пояснение под кнопкой «Применить»; None — убрать.
    pub fn set_apply_note(&self, note: Option<&str>) {
        self.apply_note.setStringValue(&NSString::from_str(note.unwrap_or_default()));
        self.apply_note.setHidden(note.is_none());
    }

    pub fn window(&self) -> &NSWindow {
        &self.window
    }

    /// Окно на экране и в нём открыт раздел «Расположение».
    pub fn is_layout_shown(&self) -> bool {
        self.window.isVisible() && self.sidebar.selected() == Some(LAYOUT_PANE)
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
        let automatic = automatic_layout();
        set_switch(&self.automatic_layout, automatic);
        // Кнопка «Применить» недоступна только на время переноса иконок.
        self.automatic_layout.setEnabled(show_panel() && self.apply.isEnabled());
        self.set_layout_mode(automatic);
        self.permissions.refresh();
    }

    /// При автоматическом расположении редактор и «Применить» скрыты: настраивать нечего.
    fn set_layout_mode(&self, automatic: bool) {
        self.hint.setHidden(automatic);
        self.editor.setHidden(automatic);
        self.apply_row.setHidden(automatic);
        if automatic {
            self.set_apply_note(None);
        }
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
    fn titles(lang: Lang) -> [(&'static str, &'static str); 3] {
        [
            ("gearshape", strings::settings_general(lang)),
            ("menubar.rectangle", strings::settings_layout(lang)),
            ("lock.shield", strings::settings_permissions(lang)),
        ]
    }

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
        let titles = Self::titles(lang).into_iter().collect();
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

    fn selected(&self) -> Option<usize> {
        let row = self.ivars().table.borrow().as_ref()?.selectedRow();
        usize::try_from(row).ok()
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

/// Раздел: элементы друг под другом от левого верхнего угла, не шире раздела.
/// Окно по высоте наименьшее и растёт, только если элементы не влезают.
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
    stack
        .trailingAnchor()
        .constraintEqualToAnchor_constant(&pane.trailingAnchor(), -CONTENT_INSET)
        .setActive(true);
    stack
        .bottomAnchor()
        .constraintLessThanOrEqualToAnchor_constant(&pane.bottomAnchor(), -CONTENT_INSET)
        .setActive(true);
    let fit = pane.heightAnchor().constraintEqualToConstant(WINDOW_SIZE.height);
    fit.setPriority(FIT_PRIORITY);
    fit.setActive(true);
    pane
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

