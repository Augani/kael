// Run this public default-policy regression in its own native integration-test
// process so parallel library tests cannot occupy its process-wide loader slots.
// The test uses headless TestAppContext and controlled HTTP responses; no real
// network, display, timing deadline, or injected loader limit is involved.
#![cfg(not(target_arch = "wasm32"))]

#[cfg(test)]
mod public_loader_admission_regression {
    use futures::{FutureExt as _, channel::oneshot, future::Shared};
    use kael::{
        Asset as _, LottieAnimation, LottieAssetLoader, LottieAssetSource, LottieError, Resource,
        Task, TestAppContext,
    };
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    const TINY: &[u8] = br#"{"fr":30,"ip":0,"op":2,"w":1,"h":1,"layers":[]}"#;
    type Loaded = Shared<Task<Result<Arc<LottieAnimation>, LottieError>>>;

    fn spawn(cx: &mut TestAppContext, name: &str) -> Loaded {
        let source = LottieAssetSource::Resource(Resource::Uri(
            format!("https://example.invalid/{name}.json").into(),
        ));
        cx.update(|cx| {
            let future = LottieAssetLoader::load(source, cx);
            cx.background_executor().spawn(future).shared()
        })
    }

    #[test]
    fn public_loader_starts_two_of_three_across_apps_and_direct_constructors_still_work() {
        let mut first = TestAppContext::single();
        let mut second = TestAppContext::single();
        let started = Arc::new(AtomicUsize::new(0));
        let receivers = Arc::new(Mutex::new(VecDeque::new()));
        let mut gates = Vec::new();
        for _ in 0..3 {
            let (sender, receiver) = oneshot::channel::<()>();
            gates.push(Some(sender));
            receivers.lock().unwrap().push_back(receiver);
        }
        let client = http_client::FakeHttpClient::create({
            let started = started.clone();
            move |_| {
                started.fetch_add(1, Ordering::SeqCst);
                let receiver = receivers
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("one gate per request");
                async move {
                    receiver
                        .await
                        .map_err(|_| anyhow::anyhow!("controlled request canceled"))?;
                    Ok(http_client::Response::builder()
                        .status(200)
                        .body(http_client::AsyncBody::from(TINY.to_vec()))
                        .unwrap())
                }
            }
        });
        first.update(|cx| cx.set_http_client(client.clone()));
        second.update(|cx| cx.set_http_client(client));
        let a = spawn(&mut first, "a");
        let b = spawn(&mut first, "b");
        first.run_until_parked();
        let c = spawn(&mut second, "c");
        second.run_until_parked();
        assert_eq!(
            started.load(Ordering::SeqCst),
            2,
            "third source IO must wait process-wide"
        );
        assert!(a.clone().now_or_never().is_none());
        assert!(b.clone().now_or_never().is_none());
        assert!(c.clone().now_or_never().is_none());

        // Both loader slots are occupied, but these public synchronous calls
        // retain their established contract and render only a 1x1 poster.
        assert_eq!(
            LottieAnimation::from_bytes(Arc::<[u8]>::from(TINY))
                .unwrap()
                .poster_frame()
                .decoded_bytes(),
            4
        );
        assert_eq!(
            LottieAnimation::from_json_str(std::str::from_utf8(TINY).unwrap())
                .unwrap()
                .poster_frame()
                .decoded_bytes(),
            4
        );
        gates[0].take().unwrap().send(()).unwrap();
        first.run_until_parked();
        second.run_until_parked();
        assert_eq!(
            started.load(Ordering::SeqCst),
            3,
            "freed slot must admit the queued source"
        );
        for sender in gates.iter_mut().skip(1) {
            sender.take().unwrap().send(()).unwrap();
        }
        first.run_until_parked();
        second.run_until_parked();
        for task in [a, b, c] {
            assert_eq!(
                task.now_or_never()
                    .unwrap()
                    .unwrap()
                    .poster_frame()
                    .decoded_bytes(),
                4
            );
        }
    }
}
