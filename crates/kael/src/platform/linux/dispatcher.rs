use crate::{PlatformDispatcher, TaskLabel};
use async_task::Runnable;
use calloop::{
    EventLoop,
    channel::{self, Sender},
    timer::TimeoutAction,
};
use parking::{Parker, Unparker};
use parking_lot::Mutex;
use std::{
    thread,
    time::{Duration, Instant},
};
use util::ResultExt;

struct TimerAfter {
    duration: Duration,
    runnable: Runnable,
}

pub(crate) struct LinuxDispatcher {
    parker: Mutex<Parker>,
    main_sender: Sender<Runnable>,
    timer_sender: Sender<TimerAfter>,
    background_sender: crossbeam_channel::Sender<Runnable>,
    _background_threads: Vec<thread::JoinHandle<()>>,
    main_thread_id: thread::ThreadId,
}

impl LinuxDispatcher {
    pub fn new(main_sender: Sender<Runnable>) -> Self {
        let (background_sender, background_receiver) = crossbeam_channel::unbounded::<Runnable>();
        let thread_count = std::thread::available_parallelism()
            .map(|i| i.get())
            .unwrap_or(1);

        let mut background_threads = Vec::with_capacity(thread_count + 1);
        for i in 0..thread_count {
            let receiver = background_receiver.clone();
            match std::thread::Builder::new()
                .name(format!("Worker-{i}"))
                .spawn(move || {
                    for runnable in receiver {
                        let start = Instant::now();

                        crate::platform::catch_platform_callback(
                            "Linux",
                            "background task",
                            (),
                            || {
                                runnable.run();
                            },
                        );

                        log::trace!(
                            "background thread {}: ran runnable. took: {:?}",
                            i,
                            start.elapsed()
                        );
                    }
                }) {
                Ok(thread) => background_threads.push(thread),
                Err(error) => log::error!("failed to spawn Linux worker {i}: {error}"),
            }
        }

        let (timer_sender, timer_channel) = calloop::channel::channel::<TimerAfter>();
        let timer_thread = std::thread::Builder::new()
            .name("Timer".to_owned())
            .spawn(|| {
                let mut event_loop: EventLoop<()> = match EventLoop::try_new() {
                    Ok(event_loop) => event_loop,
                    Err(error) => {
                        log::error!("failed to initialize Linux timer loop: {error:?}");
                        return;
                    }
                };

                let handle = event_loop.handle();
                let timer_handle = event_loop.handle();
                let timer_registration = handle.insert_source(timer_channel, move |e, _, _| {
                    if let channel::Event::Msg(timer) = e {
                        // This has to be in an option to satisfy the borrow checker. The callback below should only be scheduled once.
                        let mut runnable = Some(timer.runnable);
                        if let Err(error) = timer_handle.insert_source(
                            calloop::timer::Timer::from_duration(timer.duration),
                            move |_, _, _| {
                                if let Some(runnable) = runnable.take() {
                                    crate::platform::catch_platform_callback(
                                        "Linux",
                                        "timer task",
                                        (),
                                        || {
                                            runnable.run();
                                        },
                                    );
                                }
                                TimeoutAction::Drop
                            },
                        ) {
                            log::error!("failed to start Linux timer: {error:?}");
                        }
                    }
                });
                if let Err(error) = timer_registration {
                    log::error!("failed to register Linux timer channel: {error:?}");
                    return;
                }

                event_loop.run(None, &mut (), |_| {}).log_err();
            })
            .map_err(|error| log::error!("failed to spawn Linux timer thread: {error}"));

        if let Ok(timer_thread) = timer_thread {
            background_threads.push(timer_thread);
        }

        Self {
            parker: Mutex::new(Parker::new()),
            main_sender,
            timer_sender,
            background_sender,
            _background_threads: background_threads,
            main_thread_id: thread::current().id(),
        }
    }
}

impl PlatformDispatcher for LinuxDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread_id
    }

    fn dispatch(&self, runnable: Runnable, _: Option<TaskLabel>) {
        if self.background_sender.send(runnable).is_err() {
            log::debug!("discarding background task because the Linux dispatcher is stopping");
        }
    }

    fn dispatch_on_main_thread(&self, runnable: Runnable) {
        self.main_sender.send(runnable).unwrap_or_else(|runnable| {
            // NOTE: Runnable may wrap a Future that is !Send.
            //
            // This is usually safe because we only poll it on the main thread.
            // However if the send fails, we know that:
            // 1. main_receiver has been dropped (which implies the app is shutting down)
            // 2. we are on a background thread.
            // It is not safe to drop something !Send on the wrong thread, and
            // the app will exit soon anyway, so we must forget the runnable.
            std::mem::forget(runnable);
        });
    }

    fn dispatch_after(&self, duration: Duration, runnable: Runnable) {
        self.timer_sender
            .send(TimerAfter { duration, runnable })
            .ok();
    }

    fn park(&self, timeout: Option<Duration>) -> bool {
        if let Some(timeout) = timeout {
            self.parker.lock().park_timeout(timeout)
        } else {
            self.parker.lock().park();
            true
        }
    }

    fn unparker(&self) -> Unparker {
        self.parker.lock().unparker()
    }
}
