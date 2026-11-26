//! The display answers in time while the window draws without pause. On its
//! own, so that no other test competes for the processor.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use degus_sim::Sim;
use degus_sim::image::RgbImage;
use degus_sim_gui::SimThread;
use support::*;

#[test]
fn answers_within_50_ms_while_another_thread_renders_full_hd_frames() {
    let sim = Arc::new(SimThread::new(Sim::new(full_hd())));
    let mut host = Host::new(&sim);

    let stop = Arc::new(AtomicBool::new(false));
    let renders = Arc::new(AtomicUsize::new(0));
    let longest = {
        let (sim, stop, renders) = (sim.clone(), stop.clone(), renders.clone());
        std::thread::spawn(move || {
            let mut fb = RgbImage::new(1, 1);
            let mut longest = Duration::ZERO;
            while !stop.load(Ordering::SeqCst) {
                let sim = sim.lock();
                let started = Instant::now();
                sim.render_into(&mut fb);
                longest = longest.max(started.elapsed());
                renders.fetch_add(1, Ordering::SeqCst);
            }
            longest
        })
    };
    while renders.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }

    let mut slowest = Duration::ZERO;
    for value in 0..100 {
        let sent = Instant::now();
        host.send(&write_frame(0x1010, &[value]));
        assert_eq!(host.frame(Duration::from_secs(5)), ACK);
        slowest = slowest.max(sent.elapsed());
    }
    let rendered = renders.load(Ordering::SeqCst);
    stop.store(true, Ordering::SeqCst);
    let longest = longest.join().unwrap();
    println!("{rendered} renders, the longest {longest:?}; the slowest answer {slowest:?}");
    assert!(rendered > 20, "{rendered} renders: no contention");
    assert!(slowest < ANSWER, "an answer took {slowest:?}");
}
