//! Deterministic test support for Kael applications.
//!
//! [`kael::test`](macro@crate::test) supplies test application contexts plus foreground
//! and background executors whose scheduling is controlled by a reproducible
//! seed. Its generated functions use the standard Rust test harness, so they
//! work with `cargo test`, `cargo-nextest`, and compatible runners.
//!
//! A test may request multiple contexts to model collaboration or independent
//! windows. Set `SEED` to an unsigned integer to reproduce a run. Set
//! `ITERATIONS` to a positive integer to override the attribute's iteration
//! count; with `SEED`, that many consecutive wrapping seed values are used and
//! explicit attribute seeds are ignored.
//!
//! ## Example
//!
//! ```
//! use kael::TestAppContext;
//!
//! #[kael::test]
//! async fn test_example(_cx: &mut TestAppContext) {
//!     assert_eq!(2 + 2, 4);
//! }
//!
//! #[kael::test]
//! async fn test_collaboration_example(
//!     _cx_a: &mut TestAppContext,
//!     _cx_b: &mut TestAppContext,
//! ) {
//!     assert!(true);
//! }
//! ```
use crate::{Entity, Subscription, TestAppContext, TestDispatcher};
use futures::StreamExt as _;
use rand::prelude::*;
use smol::channel;
use std::{
    env,
    panic::{self, RefUnwindSafe},
    pin::Pin,
};

/// Runs a generated Kael test with deterministic seeds and bounded retries.
///
/// This is the runtime entry point for [`kael::test`](macro@crate::test) and generally
/// should not be called directly. `num_iterations` must be greater than zero;
/// `SEED` and `ITERATIONS` environment variables override the generated plan as
/// described in the [module documentation](self).
pub fn run_test(
    num_iterations: usize,
    explicit_seeds: &[u64],
    max_retries: usize,
    test_fn: &mut (dyn RefUnwindSafe + Fn(TestDispatcher, u64)),
    on_fail_fn: Option<fn()>,
) {
    let num_iterations = u64::try_from(num_iterations)
        .expect("test iteration count exceeds the supported u64 range");
    let (seeds, is_multiple_runs) = calculate_seeds(num_iterations, explicit_seeds);

    for seed in seeds {
        let mut attempt = 0;
        loop {
            if is_multiple_runs {
                eprintln!("seed = {seed}");
            }
            let result = panic::catch_unwind(|| {
                let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(seed));
                test_fn(dispatcher, seed);
            });

            match result {
                Ok(_) => break,
                Err(error) => {
                    if attempt < max_retries {
                        eprintln!(
                            "attempt {} failed, retrying ({}/{max_retries})",
                            attempt + 1,
                            attempt + 1
                        );
                        attempt += 1;
                        // The panic payload might itself trigger an unwind on drop:
                        // https://doc.rust-lang.org/std/panic/fn.catch_unwind.html#notes
                        std::mem::forget(error);
                    } else {
                        if is_multiple_runs {
                            eprintln!("failing seed: {}", seed);
                        }
                        if let Some(on_fail_fn) = on_fail_fn {
                            on_fail_fn()
                        }
                        panic::resume_unwind(error);
                    }
                }
            }
        }
    }
}

fn calculate_seeds(
    iterations: u64,
    explicit_seeds: &[u64],
) -> (impl Iterator<Item = u64> + '_, bool) {
    calculate_seeds_from_values(
        iterations,
        explicit_seeds,
        read_unsigned_env("ITERATIONS"),
        read_unsigned_env("SEED"),
    )
}

fn calculate_seeds_from_values(
    iterations: u64,
    explicit_seeds: &[u64],
    iterations_override: Option<u64>,
    starting_seed: Option<u64>,
) -> (impl Iterator<Item = u64> + '_, bool) {
    let iterations = iterations_override.unwrap_or(iterations);
    assert!(iterations > 0, "ITERATIONS must be greater than zero");

    let generated_count =
        if starting_seed.is_none() && iterations == 1 && !explicit_seeds.is_empty() {
            0
        } else {
            iterations
        };
    let generated_start = starting_seed.unwrap_or(0);
    let explicit_seeds = if starting_seed.is_some() {
        &[]
    } else {
        explicit_seeds
    };

    let iter = (0..generated_count)
        .map(move |offset| generated_start.wrapping_add(offset))
        .chain(explicit_seeds.iter().copied());
    let is_multiple_runs = iter.clone().nth(1).is_some();
    (iter, is_multiple_runs)
}

fn read_unsigned_env(name: &str) -> Option<u64> {
    let value = env::var_os(name)?;
    let value = value
        .into_string()
        .unwrap_or_else(|_| panic!("{name} must be a valid UTF-8 unsigned integer"));
    Some(
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be an unsigned integer, got {value:?}")),
    )
}

/// A stream that owns the subscription backing an entity observation.
#[must_use = "the observation stream must be retained and polled to receive changes"]
pub struct Observation<T> {
    rx: Pin<Box<channel::Receiver<T>>>,
    _subscription: Subscription,
}

impl<T: 'static> futures::Stream for Observation<T> {
    type Item = T;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_next_unpin(cx)
    }
}

/// Observes an [`Entity`] and returns a stream item after each change event.
pub fn observe<T: 'static>(entity: &Entity<T>, cx: &mut TestAppContext) -> Observation<()> {
    let (tx, rx) = smol::channel::unbounded();
    let _subscription = cx.update(|cx| {
        cx.observe(entity, move |_, _| {
            let _ = smol::block_on(tx.send(()));
        })
    });
    let rx = Box::pin(rx);

    Observation { rx, _subscription }
}

#[cfg(test)]
mod tests {
    use super::calculate_seeds_from_values;

    fn seeds(
        iterations: u64,
        explicit: &[u64],
        iterations_override: Option<u64>,
        starting_seed: Option<u64>,
    ) -> (Vec<u64>, bool) {
        let (seeds, multiple) =
            calculate_seeds_from_values(iterations, explicit, iterations_override, starting_seed);
        (seeds.collect(), multiple)
    }

    #[test]
    fn explicit_seeds_replace_the_implicit_single_run() {
        assert_eq!(seeds(1, &[10, 20], None, None), (vec![10, 20], true));
        assert_eq!(seeds(1, &[], None, None), (vec![0], false));
    }

    #[test]
    fn iterations_and_explicit_seeds_are_combined() {
        assert_eq!(
            seeds(3, &[10, 20], None, None),
            (vec![0, 1, 2, 10, 20], true)
        );
    }

    #[test]
    fn environment_seed_sets_the_start_without_duplication() {
        assert_eq!(seeds(1, &[99], Some(3), Some(10)), (vec![10, 11, 12], true));
    }

    #[test]
    fn seed_ranges_wrap_without_panicking() {
        assert_eq!(
            seeds(2, &[], None, Some(u64::MAX)),
            (vec![u64::MAX, 0], true)
        );
    }

    #[test]
    #[should_panic(expected = "ITERATIONS must be greater than zero")]
    fn zero_iterations_are_rejected() {
        let _ = seeds(1, &[], Some(0), None);
    }
}
