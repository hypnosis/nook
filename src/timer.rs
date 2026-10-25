//! Таймеры главного цикла: зовут селектор у цели.

use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2_foundation::NSTimer;

/// Таймер с владельцем: удалён — погашен.
pub struct Timer(Retained<NSTimer>);

impl Timer {
    /// Зовёт `selector` у `target` каждые `interval`.
    pub fn repeating(target: &AnyObject, interval: Duration, selector: Sel) -> Self {
        Self(schedule(target, interval, selector, true))
    }

    /// Зовёт `selector` у `target` один раз через `delay`.
    pub fn once(target: &AnyObject, delay: Duration, selector: Sel) -> Self {
        Self(schedule(target, delay, selector, false))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.0.invalidate();
    }
}

/// Разовый таймер без владельца: сработает и исчезнет сам.
pub fn after(target: &AnyObject, delay: Duration, selector: Sel) {
    schedule(target, delay, selector, false);
}

fn schedule(target: &AnyObject, interval: Duration, selector: Sel, repeats: bool) -> Retained<NSTimer> {
    // SAFETY: `target` отвечает на `selector` без аргументов или с таймером; NSTimer держит цель,
    // пока не погашен.
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            interval.as_secs_f64(),
            target,
            selector,
            None,
            repeats,
        )
    }
}
