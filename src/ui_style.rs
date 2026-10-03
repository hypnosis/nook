//! Единый вид окон Nook: шрифты, цвета, промежутки, высоты рядов редактора и
//! стандартные подписи, кнопки и сетки.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSButton, NSColor, NSFont, NSGridView, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSStackView, NSStatusBar, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};

/// Промежуток между элементами окон.
// HARDCODE: промежуток окон; вынести в конфиг позже.
pub const SPACING: f64 = 12.0;
/// Зазор между названием настройки и пояснением под ним.
// HARDCODE: зазор строки настройки; вынести в конфиг позже.
const DETAIL_GAP: f64 = 2.0;

pub struct EditorMetrics {
    pub title_height: f64,
    pub title_gap: f64,
    /// Высота строки иконок — как у строки меню.
    pub line_height: f64,
    /// Высота рамки ряда в одну строку.
    pub row_height: f64,
    pub row_gap: f64,
    pub padding: f64,
    pub icon_spacing: f64,
}

pub fn standard_spacing(mtm: MainThreadMarker) -> f64 {
    NSStackView::new(mtm).spacing()
}

pub fn section_font() -> Retained<NSFont> {
    NSFont::boldSystemFontOfSize(NSFont::systemFontSize())
}

pub fn footer_font() -> Retained<NSFont> {
    NSFont::systemFontOfSize(NSFont::smallSystemFontSize())
}

pub fn secondary_label_color() -> Retained<NSColor> {
    NSColor::secondaryLabelColor()
}

pub fn granted_color() -> Retained<NSColor> {
    NSColor::systemGreenColor()
}

pub fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

/// Подпись, которая переносится по ширине раздела: уступает место раньше кнопок и окна.
pub fn wrapping_label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setSelectable(false);
    label.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        NSLayoutConstraintOrientation::Horizontal,
    );
    label
}

/// Название настройки и серое пояснение под ним.
pub fn title_with_detail(mtm: MainThreadMarker, title: &str, detail: &str) -> Retained<NSStackView> {
    let detail = wrapping_label(mtm, detail);
    detail.setFont(Some(&footer_font()));
    detail.setTextColor(Some(&secondary_label_color()));
    let stack = NSStackView::stackViewWithViews(
        &NSArray::from_slice(&[&*wrapping_label(mtm, title) as &NSView, &*detail]),
        mtm,
    );
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setSpacing(DETAIL_GAP);
    stack
}

pub fn button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    }
}

/// Сетка «подпись — элемент управления» построчно.
pub fn grid<const N: usize>(mtm: MainThreadMarker, rows: &[[&NSView; N]]) -> Retained<NSGridView> {
    let rows: Vec<Retained<NSArray<NSView>>> =
        rows.iter().map(|row| NSArray::from_slice(row)).collect();
    let grid = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&rows), mtm);
    grid.setRowSpacing(SPACING);
    grid.setColumnSpacing(SPACING * 2.0);
    grid
}

pub fn editor_metrics(mtm: MainThreadMarker) -> EditorMetrics {
    let spacing = standard_spacing(mtm);
    let font = section_font();
    let text_height = font.ascender() - font.descender() + font.leading();
    let line_height = NSStatusBar::systemStatusBar().thickness();
    EditorMetrics {
        title_height: text_height + spacing,
        title_gap: spacing,
        line_height,
        row_height: line_height + spacing * 2.0,
        row_gap: spacing * 2.0,
        padding: spacing,
        icon_spacing: spacing,
    }
}
