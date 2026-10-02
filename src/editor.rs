//! Редактор рядов «Основной ряд» и «Панель»: клоны иконок перетаскиваются мышью.
//!
//! Иконки идут слева направо и переносятся на следующую строку рамки, порядок
//! сквозной. Индексы: иконки панели слева направо −n…−1, разделитель панели — 0,
//! иконки основного ряда слева направо 1…m.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSEvent, NSImage, NSImageView, NSLayoutConstraint, NSTextField,
    NSTitlePosition, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};

use crate::strings::{self, Lang};
use crate::ui_style::{self, EditorMetrics};

const MAIN: usize = 0;
const PANEL: usize = 1;

/// Клон иконки: номер окна настоящей иконки и картинка.
struct Tile {
    id: u32,
    image: Retained<NSImageView>,
}

/// Перетаскиваемая иконка: за какую точку её взяли и куда она встанет — ряд и место.
struct Drag {
    tile: Tile,
    grab: NSPoint,
    target: (usize, usize),
}

/// Место иконки в ряду: строка и левый край.
struct Slot {
    line: usize,
    x: f64,
}

pub struct EditorIvars {
    rows: RefCell<[Vec<Tile>; 2]>,
    /// Порядок строки меню из последнего снимка: (панель, основной ряд).
    loaded: RefCell<(Vec<u32>, Vec<u32>)>,
    titles: [Retained<NSTextField>; 2],
    boxes: [Retained<NSBox>; 2],
    row_frames: RefCell<[NSRect; 2]>,
    height: OnceCell<Retained<NSLayoutConstraint>>,
    laid_out_width: Cell<f64>,
    drag: RefCell<Option<Drag>>,
    metrics: EditorMetrics,
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

        /// Ширина изменилась — иконки переносятся заново.
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let width = self.bounds().size.width;
            if width != self.ivars().laid_out_width.get() {
                self.ivars().laid_out_width.set(width);
                self.layout_tiles(false);
            }
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
            let mut rows = self.ivars().rows.borrow_mut();
            let Some((row, position)) = rows.iter().enumerate().find_map(|(row, tiles)| {
                tiles
                    .iter()
                    .position(|tile| contains(tile.image.frame(), point))
                    .map(|position| (row, position))
            }) else {
                return;
            };
            let tile = rows[row].remove(position);
            drop(rows);
            let frame = tile.image.frame();
            tile.image.removeFromSuperview();
            self.addSubview(&tile.image);
            let grab = NSPoint::new(point.x - frame.origin.x, point.y - frame.origin.y);
            *self.ivars().drag.borrow_mut() = Some(Drag { tile, grab, target: (row, position) });
        }

        /// Иконка идёт за мышью, остальные расступаются там, куда она встанет.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let point = self.location(event);
            let target = self.target_at(point);
            let moved = {
                let mut drag = self.ivars().drag.borrow_mut();
                let Some(drag) = drag.as_mut() else { return };
                drag.tile.image.setFrameOrigin(NSPoint::new(point.x - drag.grab.x, point.y - drag.grab.y));
                std::mem::replace(&mut drag.target, target) != target
            };
            if moved {
                self.layout_tiles(true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let Some(drag) = self.ivars().drag.borrow_mut().take() else { return };
            let (row, position) = drag.target;
            self.ivars().rows.borrow_mut()[row].insert(position, drag.tile);
            self.layout_tiles(true);
        }
    }
);

impl EditorView {
    /// Высота редактора следует за числом строк в рядах: держит её сам.
    pub fn new(mtm: MainThreadMarker, lang: Lang, width: f64) -> Retained<Self> {
        let titles = [strings::editor_main(lang), strings::editor_panel(lang)].map(|title| {
            let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
            label.setFont(Some(&ui_style::section_font()));
            label
        });
        let boxes = std::array::from_fn(|_| {
            let backing = NSBox::new(mtm);
            backing.setBoxType(NSBoxType::Primary);
            backing.setTitlePosition(NSTitlePosition::NoTitle);
            backing
        });
        let this = mtm.alloc().set_ivars(EditorIvars {
            rows: RefCell::new([Vec::new(), Vec::new()]),
            loaded: RefCell::new((Vec::new(), Vec::new())),
            titles,
            boxes,
            row_frames: RefCell::new([NSRect::ZERO; 2]),
            height: OnceCell::new(),
            laid_out_width: Cell::new(width),
            drag: RefCell::new(None),
            metrics: ui_style::editor_metrics(mtm),
        });
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 0.0));
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        for (title, backing) in this.ivars().titles.iter().zip(&this.ivars().boxes) {
            this.addSubview(title);
            this.addSubview(backing);
        }
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        let height = this.heightAnchor().constraintEqualToConstant(0.0);
        height.setActive(true);
        let _ = this.ivars().height.set(height);
        this.layout_tiles(false);
        this
    }

    /// Картинки иконок и порядок строки меню: `panel` и `main` слева направо.
    /// Неприменённый порядок сохраняется: обновляются картинки, новые иконки встают
    /// на свои места в строке меню, пропавшие уходят.
    pub fn set_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>, panel: &[u32], main: &[u32]) {
        let shots: Vec<(u32, Retained<NSImage>)> =
            ids.iter().map(|id| id.unsignedIntValue()).zip(images.iter()).collect();
        let shown = |ids: &[u32]| -> Vec<u32> {
            ids.iter().copied().filter(|id| shots.iter().any(|(shot, _)| shot == id)).collect()
        };
        let system = [shown(main), shown(panel)];
        let order = if self.has_changes() {
            let (panel, main) = self.order();
            merged_order([main, panel], &system)
        } else {
            system.clone()
        };

        let mut old: Vec<Tile> = self.ivars().rows.borrow_mut().iter_mut().flat_map(std::mem::take).collect();
        let mut rows: [Vec<Tile>; 2] = [Vec::new(), Vec::new()];
        for (row, ids) in order.iter().enumerate() {
            for id in ids {
                let Some((_, image)) = shots.iter().find(|(shot, _)| shot == id) else { continue };
                let tile = match old.iter().position(|tile| tile.id == *id) {
                    Some(index) => {
                        let tile = old.swap_remove(index);
                        tile.image.setImage(Some(image));
                        tile
                    }
                    None => self.make_tile(*id, image),
                };
                rows[row].push(tile);
            }
        }
        for tile in old {
            tile.image.removeFromSuperview();
        }
        *self.ivars().rows.borrow_mut() = rows;
        let [main, panel] = system;
        *self.ivars().loaded.borrow_mut() = (panel, main);
        self.layout_tiles(false);
    }

    /// Номера окон слева направо: (панель, основной ряд).
    pub fn order(&self) -> (Vec<u32>, Vec<u32>) {
        let rows = self.ivars().rows.borrow();
        let ids = |row: usize| rows[row].iter().map(|tile| tile.id).collect();
        (ids(PANEL), ids(MAIN))
    }

    pub fn is_dragging(&self) -> bool {
        self.ivars().drag.borrow().is_some()
    }

    /// Порядок отправлен в строку меню: следующий снимок покажет её как есть.
    pub fn mark_applied(&self) {
        *self.ivars().loaded.borrow_mut() = self.order();
    }

    /// Порядок в редакторе отличается от строки меню из последнего снимка.
    fn has_changes(&self) -> bool {
        self.order() != *self.ivars().loaded.borrow()
    }

    fn make_tile(&self, id: u32, image: &NSImage) -> Tile {
        let view = NSImageView::imageViewWithImage(image, self.mtm());
        self.addSubview(&view);
        Tile { id, image: view }
    }

    /// Раскладывает клоны по рядам с переносом строк, рамки и подписи — вокруг них;
    /// высота редактора следует за числом строк. `animated` — сдвиги идут плавно.
    fn layout_tiles(&self, animated: bool) {
        let reduce_motion: bool =
            unsafe { msg_send![&*NSWorkspace::sharedWorkspace(), accessibilityDisplayShouldReduceMotion] };
        let animated = animated && !reduce_motion;
        let rows = self.ivars().rows.borrow();
        let drag = self.ivars().drag.borrow();
        let metrics = &self.ivars().metrics;
        let width = self.bounds().size.width;
        let mut frames = [NSRect::ZERO; 2];
        let mut top = 0.0;
        for (row, tiles) in rows.iter().enumerate() {
            let mut sizes: Vec<NSSize> = tiles.iter().map(|tile| tile_size(tile, metrics)).collect();
            let gap = drag.as_ref().filter(|drag| drag.target.0 == row);
            if let Some(drag) = gap {
                sizes.insert(drag.target.1, tile_size(&drag.tile, metrics));
            }
            let gap = gap.map(|drag| drag.target.1);
            let (slots, lines) = flow(sizes.iter().map(|size| size.width), width, metrics);
            let box_top = top + metrics.title_height;
            let frame = NSRect::new(
                NSPoint::new(0.0, box_top),
                NSSize::new(width, row_height(metrics, lines)),
            );
            place(
                &self.ivars().titles[row],
                NSRect::new(
                    NSPoint::new(0.0, top),
                    NSSize::new(width, metrics.title_height - metrics.title_gap),
                ),
                animated,
            );
            place(&self.ivars().boxes[row], frame, animated);
            let placed = slots
                .iter()
                .zip(&sizes)
                .enumerate()
                .filter(|(index, _)| Some(*index) != gap)
                .map(|(_, place)| place);
            for (tile, (slot, size)) in tiles.iter().zip(placed) {
                let y = box_top
                    + metrics.padding
                    + slot.line as f64 * line_pitch(metrics)
                    + (metrics.line_height - size.height) / 2.0;
                place(&tile.image, NSRect::new(NSPoint::new(slot.x, y), *size), animated);
            }
            frames[row] = frame;
            top = box_top + frame.size.height + metrics.row_gap;
        }
        *self.ivars().row_frames.borrow_mut() = frames;
        let height = frames[PANEL].origin.y + frames[PANEL].size.height;
        if let Some(constraint) = self.ivars().height.get() {
            if constraint.constant() != height {
                constraint.setConstant(height);
            }
        }
    }

    /// Куда встанет иконка, отпущенная в `point`: ряд и место среди остальных.
    fn target_at(&self, point: NSPoint) -> (usize, usize) {
        let row = self.row_at(point.y);
        let rows = self.ivars().rows.borrow();
        let metrics = &self.ivars().metrics;
        let sizes: Vec<NSSize> = rows[row].iter().map(|tile| tile_size(tile, metrics)).collect();
        let (slots, lines) = flow(sizes.iter().map(|size| size.width), self.bounds().size.width, metrics);
        let box_top = self.ivars().row_frames.borrow()[row].origin.y;
        let line = ((point.y - box_top - metrics.padding) / line_pitch(metrics))
            .floor()
            .clamp(0.0, (lines - 1) as f64) as usize;
        let position = slots
            .iter()
            .zip(&sizes)
            .filter(|(slot, size)| {
                slot.line < line || (slot.line == line && slot.x + size.width / 2.0 < point.x)
            })
            .count();
        (row, position)
    }

    /// Ряд под точкой по вертикали: граница — посередине между рамками.
    fn row_at(&self, y: f64) -> usize {
        let frames = self.ivars().row_frames.borrow();
        let main_bottom = frames[MAIN].origin.y + frames[MAIN].size.height;
        if y < (main_bottom + frames[PANEL].origin.y) / 2.0 {
            MAIN
        } else {
            PANEL
        }
    }

    fn location(&self, event: &NSEvent) -> NSPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }
}

/// Неприменённый порядок `user` поверх свежей строки меню `system`: пропавшие иконки
/// уходят, новые встают на своё место в ряду строки меню.
fn merged_order(user: [Vec<u32>; 2], system: &[Vec<u32>; 2]) -> [Vec<u32>; 2] {
    let fresh: Vec<u32> = system.iter().flatten().copied().collect();
    let known: Vec<u32> = user.iter().flatten().copied().collect();
    let mut rows = user.map(|ids| ids.into_iter().filter(|id| fresh.contains(id)).collect::<Vec<_>>());
    for (row, ids) in system.iter().enumerate() {
        for (index, id) in ids.iter().enumerate() {
            if !known.contains(id) {
                let at = index.min(rows[row].len());
                rows[row].insert(at, *id);
            }
        }
    }
    rows
}

/// Места иконок шириной `widths` слева направо: не влезшая в `width` переносится
/// на следующую строку. Возвращает места и число строк.
fn flow(widths: impl Iterator<Item = f64>, width: f64, metrics: &EditorMetrics) -> (Vec<Slot>, usize) {
    let mut line = 0;
    let mut x = metrics.padding;
    let slots = widths
        .map(|item| {
            if x > metrics.padding && x + item > width - metrics.padding {
                line += 1;
                x = metrics.padding;
            }
            let slot = Slot { line, x };
            x += item + metrics.icon_spacing;
            slot
        })
        .collect();
    (slots, line + 1)
}

/// Шаг строк иконок внутри рамки.
fn line_pitch(metrics: &EditorMetrics) -> f64 {
    metrics.line_height + metrics.icon_spacing
}

/// Высота рамки ряда из `lines` строк.
fn row_height(metrics: &EditorMetrics, lines: usize) -> f64 {
    metrics.row_height + (lines - 1) as f64 * line_pitch(metrics)
}

fn tile_size(tile: &Tile, metrics: &EditorMetrics) -> NSSize {
    tile.image
        .image()
        .map(|image| image.size())
        .unwrap_or_else(|| NSSize::new(metrics.line_height, metrics.line_height))
}

/// Ставит вид в `frame`; `animated` — плавно, через аниматор AppKit.
fn place(view: &NSView, frame: NSRect, animated: bool) {
    if animated {
        let animator: Retained<NSView> = unsafe { msg_send![view, animator] };
        animator.setFrame(frame);
    } else {
        view.setFrame(frame);
    }
}

fn contains(frame: NSRect, point: NSPoint) -> bool {
    point.x >= frame.origin.x
        && point.x <= frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y <= frame.origin.y + frame.size.height
}
