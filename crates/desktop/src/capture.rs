//! Opt-in developer captures use the real window and normal worker teardown.
use crate::app::MainWindow;
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

#[derive(Default)]
pub struct Capture {
    _timer: slint::Timer,
    error: Rc<RefCell<Option<String>>>,
}
fn number(name: &str, fallback: u32, low: u32, high: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(fallback)
        .clamp(low, high)
}
fn snapshot(ui: &MainWindow, path: &Path) -> anyhow::Result<()> {
    let pixels = ui.window().take_snapshot()?;
    image::save_buffer(
        path,
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )?;
    Ok(())
}
impl Capture {
    pub fn start(ui: &MainWindow) -> Self {
        let capture = Self::default();
        let Ok(path) = std::env::var("AUVEMIR_SCREENSHOT") else {
            return capture;
        };
        let mobile = std::env::var_os("AUVEMIR_MOBILE").is_some();
        let width = number(
            "AUVEMIR_SCREENSHOT_WIDTH",
            if mobile { 390 } else { 1180 },
            320,
            2560,
        );
        let height = number(
            "AUVEMIR_SCREENSHOT_HEIGHT",
            if mobile { 844 } else { 780 },
            540,
            1800,
        );
        ui.window()
            .set_size(slint::LogicalSize::new(width as f32, height as f32));
        let delay = number("AUVEMIR_SCREENSHOT_DELAY_MS", 2000, 100, 60000);
        let weak = ui.as_weak();
        let error = capture.error.clone();
        let frames_dir = std::env::var_os("AUVEMIR_CAPTURE_DIR").map(PathBuf::from);
        if let Some(dir) = frames_dir {
            let frames = number("AUVEMIR_CAPTURE_FRAMES", 100, 2, 240);
            let fps = number("AUVEMIR_CAPTURE_FPS", 20, 1, 30);
            let started = std::time::Instant::now();
            let frame = Cell::new(0);
            capture._timer.start(
                slint::TimerMode::Repeated,
                Duration::from_millis((1000 / fps) as u64),
                move || {
                    if started.elapsed() < Duration::from_millis(delay as u64) {
                        return;
                    }
                    let index = frame.get();
                    if let Some(ui) = weak.upgrade() {
                        let result = std::fs::create_dir_all(&dir)
                            .map_err(anyhow::Error::from)
                            .and_then(|()| {
                                snapshot(&ui, &dir.join(format!("frame-{index:04}.png")))
                            });
                        if let Err(e) = result {
                            *error.borrow_mut() = Some(e.to_string());
                            let _ = slint::quit_event_loop();
                            return;
                        }
                    }
                    frame.set(index + 1);
                    if index + 1 >= frames {
                        let _ = slint::quit_event_loop();
                    }
                },
            );
        } else {
            capture._timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(delay as u64),
                move || {
                    if let Some(ui) = weak.upgrade()
                        && let Err(e) = snapshot(&ui, Path::new(&path))
                    {
                        *error.borrow_mut() = Some(e.to_string());
                    }
                    let _ = slint::quit_event_loop();
                },
            );
        }
        capture
    }
    pub fn finish(self) -> anyhow::Result<()> {
        if let Some(error) = self.error.borrow_mut().take() {
            anyhow::bail!("Could not save capture: {error}");
        }
        Ok(())
    }
}
