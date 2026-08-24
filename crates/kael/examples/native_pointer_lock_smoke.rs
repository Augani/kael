use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, ensure};
use kael::prelude::*;
use kael::{
    App, Application, Context, GameInputAvailability, PointerLockStatus, Render, TitlebarOptions,
    Window, WindowOptions, div,
};

struct Smoke {
    relative_motion_seen: Arc<AtomicBool>,
}

impl Render for Smoke {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let relative_motion_seen = self.relative_motion_seen.clone();
        div()
            .id("native-pointer-lock-smoke")
            .size_full()
            .on_pointer_event(move |event, _, _| {
                if f32::from(event.movement.x) != 0.0 || f32::from(event.movement.y) != 0.0 {
                    relative_motion_seen.store(true, Ordering::Release);
                }
            })
    }
}

fn main() -> anyhow::Result<()> {
    let outcome = Arc::new(AtomicU8::new(0));
    let relative_motion_seen = Arc::new(AtomicBool::new(false));
    let require_motion = std::env::var_os("KAEL_POINTER_LOCK_REQUIRE_MOTION").is_some();
    let app_outcome = outcome.clone();
    let app_motion = relative_motion_seen.clone();
    Application::try_new()?.run(move |cx: &mut App| {
        let window = match cx.open_window(
            WindowOptions {
                show: false,
                titlebar: Some(TitlebarOptions {
                    title: Some("Kael native pointer lock smoke".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(move |_| Smoke {
                    relative_motion_seen: app_motion,
                })
            },
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("NATIVE_POINTER_LOCK_SMOKE_FAIL: open window: {error:#}");
                app_outcome.store(2, Ordering::Release);
                cx.quit();
                return;
            }
        };

        cx.activate(true);
        if let Err(error) = window.update(cx, |_, window, _| {
            window.show_window();
            window.activate_window();
        }) {
            eprintln!("NATIVE_POINTER_LOCK_SMOKE_FAIL: show window: {error:#}");
            app_outcome.store(2, Ordering::Release);
            cx.quit();
            return;
        }

        let outcome = app_outcome;
        let relative_motion_seen = relative_motion_seen.clone();
        cx.spawn(async move |cx| {
            let activation_deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let active = window
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false);
                if active {
                    break;
                }
                if Instant::now() >= activation_deadline {
                    eprintln!("NATIVE_POINTER_LOCK_SMOKE_FAIL: window did not become active");
                    outcome.store(2, Ordering::Release);
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }

            let explicit_cycle = window.update(cx, |_, window, _| -> anyhow::Result<()> {
                ensure!(
                    window.game_input_capabilities().pointer_lock
                        == GameInputAvailability::Available,
                    "native backend did not advertise pointer lock"
                );
                window.request_pointer_lock()?;
                ensure!(
                    window.pointer_lock_status() == PointerLockStatus::Locked,
                    "native request did not synchronously acquire pointer lock"
                );
                window.exit_pointer_lock()?;
                ensure!(
                    window.pointer_lock_status() == PointerLockStatus::Unlocked,
                    "explicit exit did not release pointer lock"
                );
                window.request_pointer_lock()?;
                ensure!(
                    window.pointer_lock_status() == PointerLockStatus::Locked,
                    "second native request did not acquire pointer lock"
                );
                Ok(())
            });
            if let Err(error) = explicit_cycle
                .context("update native pointer-lock smoke window")
                .and_then(|result| result)
            {
                eprintln!("NATIVE_POINTER_LOCK_SMOKE_FAIL: {error:#}");
                outcome.store(2, Ordering::Release);
                let _ = cx.update(|cx| cx.quit());
                return;
            }

            if require_motion {
                println!("NATIVE_POINTER_LOCK_SMOKE_STAGE: locked-awaiting-motion");
                let motion_deadline = Instant::now() + Duration::from_secs(5);
                while !relative_motion_seen.load(Ordering::Acquire) {
                    if Instant::now() >= motion_deadline {
                        eprintln!(
                            "NATIVE_POINTER_LOCK_SMOKE_FAIL: XI2 relative motion was not delivered"
                        );
                        outcome.store(2, Ordering::Release);
                        let _ = cx.update(|cx| cx.quit());
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                }
                println!("NATIVE_POINTER_LOCK_SMOKE_STAGE: relative-motion");
            }

            if let Err(error) = window.update(cx, |_, window, _| window.minimize_window()) {
                eprintln!("NATIVE_POINTER_LOCK_SMOKE_FAIL: minimize window: {error:#}");
                outcome.store(2, Ordering::Release);
                let _ = cx.update(|cx| cx.quit());
                return;
            }

            let cleanup_deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let status = window
                    .update(cx, |_, window, _| window.pointer_lock_status())
                    .unwrap_or(PointerLockStatus::Unsupported);
                if status == PointerLockStatus::Unlocked {
                    println!(
                        "NATIVE_POINTER_LOCK_SMOKE_OK: explicit exit and focus-loss cleanup passed"
                    );
                    outcome.store(1, Ordering::Release);
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
                if Instant::now() >= cleanup_deadline {
                    eprintln!(
                        "NATIVE_POINTER_LOCK_SMOKE_FAIL: focus-loss cleanup left status {status:?}"
                    );
                    outcome.store(2, Ordering::Release);
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }
        })
        .detach();
    });

    ensure!(
        outcome.load(Ordering::Acquire) == 1,
        "native pointer-lock smoke failed"
    );
    Ok(())
}
