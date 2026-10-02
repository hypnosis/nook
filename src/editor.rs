//! Редактор рядов «Основной ряд» и «Панель»: клоны иконок перетаскиваются мышью.
//!
//! Индексы: иконки панели слева направо −n…−1, разделитель панели — 0,
//! иконки основного ряда слева направо 1…m.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSEvent, NSFont, NSImage, NSImageView, NSTextField, NSTitlePosition,
    NSView,
};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};

use crate::strings::{self, Lang};

// HARDCODE: размеры редактора; вынести в конфиг позже.
pub const EDITOR_WIDTH: f64 = 520.0;
const TITLE_HEIGHT: f64 = 22.0;
/// Зазор под заголовком ряда.
const TITLE_GAP: f64 = 4.0;
/// Размер плитки, если у снимка иконки нет картинки.
const FALLBACK_ICON_SIZE: NSSize = NSSize::new(24.0, 24.0);
const ROW_HEIGHT: f64 = 52.0;
const ROW_GAP: f64 = 14.0;
const PADDING: f64 = 10.0;
const ICON_SPACING: f64 = 8.0;
const CORNER_RADIUS: f64 = 8.0;

const MAIN: usize = 0;
const PANEL: usize = 1;

/// Клон иконки: номер окна настоящей иконки и картинка.
struct Tile {
    id: u32,
    image: Retained<NSImageView>,
}

/// Перетаскиваемая иконка: где лежала и за какую точку её взяли.
struct Drag {
    row: usize,
    position: usize,
    grab: NSPoint,
}

pub struct EditorIvars {
    rows: RefCell<[Vec<Tile>; 2]>,
    boxes: RefCell<Vec<Retained<NSBox>>>,
    drag: RefCell<Option<Drag>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = EditorIvars]
    pub struct EditorView;

    unsafe impl NSObjectProtocol for EditorView {}

    impl EditorView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Все щелчки внутри редактора достаются ему, а не клонам и подложкам.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            let size = self.bounds().size;
            let inside = local.x >= 0.0 && local.y >= 0.0 && local.x <= size.width && local.y <= size.height;
            inside.then(|| Retained::into_super(self.retain()))
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.location(event);
            let rows = self.ivars().rows.borrow();
            for (row, tiles) in rows.iter().enumerate() {
                for (position, tile) in tiles.iter().enumerate() {
                    let frame = tile.image.frame();
                    if contains(frame, point) {
                        tile.image.removeFromSuperview();
                        self.addSubview(&tile.image);
                        let grab = NSPoint::new(point.x - frame.origin.x, point.y - frame.origin.y);
                        *self.ivars().drag.borrow_mut() = Some(Drag { row, position, grab });
                        return;
                    }
                }
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let drag = self.ivars().drag.borrow();
            let Some(drag) = drag.as_ref() else { return };
            let point = self.location(event);
            let rows = self.ivars().rows.borrow();
            let tile = &rows[drag.row][drag.position];
            tile.image.setFrameOrigin(NSPoint::new(point.x - drag.grab.x, point.y - drag.grab.y));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let Some(drag) = self.ivars().drag.borrow_mut().take() else { return };
            let point = self.location(event);
            {
                let mut rows = self.ivars().rows.borrow_mut();
                let tile = rows[drag.row].remove(drag.position);
                let target = self.row_at(point.y);
                let position = rows[target]
                    .iter()
                    .filter(|other| center_x(other.image.frame()) < point.x)
                    .count();
                rows[target].insert(position, tile);
            }
            self.layout_tiles();
        }
    }
);

impl EditorView {
    pub fn new(mtm: MainThreadMarker, lang: Lang) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(EditorIvars {
            rows: RefCell::new([Vec::new(), Vec::new()]),
            boxes: RefCell::new(Vec::new()),
            drag: RefCell::new(None),
        });
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(EDITOR_WIDTH, Self::height()),
        );
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        for (row, title) in [strings::editor_main(lang), strings::editor_panel(lang)]
            .iter()
            .enumerate()
        {
            let top = row_top(row);
            let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
            label.setFont(Some(
                &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
            ));
            label.setFrame(NSRect::new(
                NSPoint::new(0.0, top - TITLE_HEIGHT),
                NSSize::new(EDITOR_WIDTH, TITLE_HEIGHT - TITLE_GAP),
            ));
            this.addSubview(&label);
            let backing = NSBox::new(mtm);
            backing.setBoxType(NSBoxType::Custom);
            backing.setTitlePosition(NSTitlePosition::NoTitle);
            backing.setBorderWidth(0.0);
            backing.setCornerRadius(CORNER_RADIUS);
            backing.setFillColor(&NSColor::quaternaryLabelColor());
            backing.setFrame(NSRect::new(
                NSPoint::new(0.0, top),
                NSSize::new(EDITOR_WIDTH, ROW_HEIGHT),
            ));
            this.addSubview(&backing);
            this.ivars().boxes.borrow_mut().push(backing);
        }
        this
    }

    pub fn height() -> f64 {
        row_top(PANEL) + ROW_HEIGHT
    }

    /// Новые снимки иконок левее ≡◂ в порядке строки меню: `panel_ids` идут в панель,
    /// остальные — в основной ряд.
    pub fn set_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>, panel_ids: &[u32]) {
        let mtm = self.mtm();
        let mut rows = self.ivars().rows.borrow_mut();
        for tile in rows.iter().flatten() {
            tile.image.removeFromSuperview();
        }
        let (panel, main): (Vec<Tile>, Vec<Tile>) = images
            .iter()
            .zip(ids.iter())
            .map(|(image, id)| self.make_tile(mtm, id.unsignedIntValue(), &image))
            .partition(|tile| panel_ids.contains(&tile.id));
        *rows = [main, panel];
        drop(rows);
        self.layout_tiles();
    }

    /// Номера окон слева направо: (панель, основной ряд).
    pub fn order(&self) -> (Vec<u32>, Vec<u32>) {
        let rows = self.ivars().rows.borrow();
        let ids = |row: usize| rows[row].iter().map(|tile| tile.id).collect();
        (ids(PANEL), ids(MAIN))
    }

    fn make_tile(&self, mtm: MainThreadMarker, id: u32, image: &NSImage) -> Tile {
        let view = NSImageView::imageViewWithImage(image, mtm);
        self.addSubview(&view);
        Tile { id, image: view }
    }

    /// Раскладывает клоны по рядам.
    fn layout_tiles(&self) {
        let rows = self.ivars().rows.borrow();
        for (row, tiles) in rows.iter().enumerate() {
            let top = row_top(row);
            let mut x = PADDING;
            for tile in tiles {
                let size = tile
                    .image
                    .image()
                    .map(|image| image.size())
                    .unwrap_or(FALLBACK_ICON_SIZE);
                let y = top + ((ROW_HEIGHT - size.height) / 2.0).max(0.0);
                tile.image.setFrame(NSRect::new(NSPoint::new(x, y), size));
                x += size.width + ICON_SPACING;
            }
        }
    }

    /// Ряд под точкой по вертикали: ближайшая подложка.
    fn row_at(&self, y: f64) -> usize {
        let boxes = self.ivars().boxes.borrow();
        let distance = |row: usize| {
            let frame = boxes[row].frame();
            (frame.origin.y + frame.size.height / 2.0 - y).abs()
        };
        if distance(PANEL) < distance(MAIN) {
            PANEL
        } else {
            MAIN
        }
    }

    fn location(&self, event: &NSEvent) -> NSPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }
}

fn row_top(row: usize) -> f64 {
    TITLE_HEIGHT + row as f64 * (ROW_HEIGHT + ROW_GAP + TITLE_HEIGHT)
}

fn contains(frame: NSRect, point: NSPoint) -> bool {
    point.x >= frame.origin.x
        && point.x <= frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y <= frame.origin.y + frame.size.height
}

fn center_x(frame: NSRect) -> f64 {
    frame.origin.x + frame.size.width / 2.0
}
