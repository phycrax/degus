//! The display on its thread, with a host on its UART: answers, uploads,
//! changes from tools, the monitor, and the end.

mod support;

use std::assert_matches;
use std::io::{ErrorKind, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use degus_set::controls::Point;
use degus_sim::{Event, Sim};
use degus_sim_gui::{ActivityKind, Monitor, SimThread};
use support::*;

/// What `monitor` saw until it saw `done`, waiting up to [`WAIT`].
#[track_caller]
fn seen_until(monitor: &Monitor, done: impl Fn(&ActivityKind) -> bool) -> Vec<ActivityKind> {
    let mut seen = Vec::new();
    eventually("the monitor sees it", || {
        seen.extend(monitor.take().into_iter().map(|activity| activity.kind));
        seen.iter().any(&done)
    });
    seen
}

/// The frame of `content`, without CRC.
fn frame(content: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x5A, 0xA5, content.len() as u8];
    frame.extend(content);
    frame
}

#[test]
fn acknowledges_a_write_sent_to_the_uart() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut host = Host::new(&sim);
    host.send(&write_frame(0x1010, &[0x1234]));
    assert_eq!(host.frame(ANSWER), ACK);
    assert_eq!(sim.lock().vp().get(0x1010), 0x1234);

    // Byte by byte, as a slow UART delivers it.
    for byte in write_frame(0x1011, &[5, 6]) {
        host.send(&[byte]);
    }
    assert_eq!(host.frame(ANSWER), ACK);
    assert_eq!(sim.lock().vp().read(0x1011, 2).unwrap(), [5, 6]);
}

#[test]
fn a_touch_on_a_key_code_uploads_its_vp() {
    let sim = SimThread::new(Sim::new(panel()));
    let host = Host::new(&sim);
    sim.update(|sim| sim.press(Point::new(20, 20)));
    assert_eq!(host.frame(ANSWER), upload(KEY_VP, &[KEY_VALUE]));
    assert!(sim.lock().is_pressed());
    sim.update(|sim| sim.release());
    assert!(!sim.lock().is_pressed());
    // Bare screen.
    sim.update(|sim| {
        sim.press(Point::new(150, 100));
        sim.release();
    });
    assert!(host.quiet(Duration::from_millis(200)));
}

#[test]
fn dropping_the_handle_ends_the_thread_and_the_hosts_read() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut host = Host::new(&sim);
    let monitor = sim.monitor();
    host.send(&write_frame(0x1010, &[1]));
    assert_eq!(host.frame(ANSWER), ACK);

    let started = Instant::now();
    drop(sim);
    // The thread is joined.
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(host.closes(WAIT), "the host's read did not return 0");
    let error = host.uart.write_all(&write_frame(0x1010, &[1])).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    // The monitor ends too.
    while monitor.recv().is_some() {}
}

#[test]
fn every_handle_is_the_same_uart() {
    let sim = SimThread::new(Sim::new(panel()));
    let host = Host::new(&sim);
    let mut writer = sim.uart();
    writer.write_all(&write_frame(0x1010, &[1])).unwrap();
    assert_eq!(host.frame(ANSWER), ACK);
}

#[test]
fn what_the_display_sends_before_a_host_is_attached_is_lost() {
    let sim = SimThread::new(Sim::new(panel()));
    sim.update(|sim| {
        sim.press(Point::new(20, 20));
        sim.release();
    });
    std::thread::sleep(Duration::from_millis(100));
    let host = Host::new(&sim);
    assert!(host.quiet(Duration::from_millis(200)));
}

#[test]
fn the_monitor_sees_the_traffic_and_the_events_in_order() {
    let sim = SimThread::new(Sim::new(panel()));
    let monitor = sim.monitor();
    let mut host = Host::new(&sim);
    let write = write_frame(0x1010, &[0x0001, 0x0203]);
    host.send(&write);
    host.frame(ANSWER);
    let ack = ActivityKind::ToHost(frame(&ACK));
    let seen = seen_until(&monitor, |kind| *kind == ack);
    assert_eq!(seen, [ActivityKind::FromHost(write), ack]);

    sim.update(|sim| {
        sim.press(Point::new(20, 20));
        sim.release();
        sim.show_page(1);
    });
    let page = ActivityKind::Event(Event::Page(1));
    let seen = seen_until(&monitor, |kind| *kind == page);
    let upload = ActivityKind::ToHost(frame(&upload(KEY_VP, &[KEY_VALUE])));
    // The touch sound between them.
    assert_matches!(
        &seen[..],
        [first, ActivityKind::Event(Event::Sound(_)), last] if *first == upload && *last == page
    );
}

#[test]
fn a_tool_takes_the_answer_to_its_own_frame() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut host = Host::new(&sim);
    let answer = sim.update(|sim| {
        sim.feed(&write_frame(0x1010, &[7]));
        sim.take_tx()
    });
    assert_eq!(answer, frame(&ACK));
    assert!(host.quiet(Duration::from_millis(200)));
    // The host's own frames are still answered.
    host.send(&write_frame(0x1010, &[8]));
    assert_eq!(host.frame(ANSWER), ACK);
}

#[test]
fn a_slow_window_does_not_delay_answers() {
    let sim = SimThread::new(Sim::new(panel()));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    sim.on_change(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
    });
    let mut host = Host::new(&sim);
    for value in 0..5 {
        host.send(&write_frame(0x1010, &[value]));
        assert_eq!(host.frame(ANSWER), ACK);
    }
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

#[test]
fn tells_the_window_when_there_is_something_new() {
    let sim = SimThread::new(Sim::new(panel()));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    sim.on_change(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    eventually("the first call", || calls.load(Ordering::SeqCst) >= 1);
    // Calls are only for something new.
    std::thread::sleep(Duration::from_millis(200));
    let before = calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(calls.load(Ordering::SeqCst), before);
    sim.update(|sim| sim.show_page(1));
    eventually("a call for the page", || {
        calls.load(Ordering::SeqCst) > before
    });
}

#[test]
fn a_reload_keeps_the_page_and_the_vps() {
    let sim = SimThread::new(Sim::new(panel()));
    sim.update(|sim| {
        sim.write_vp(0x1010, &[42]).unwrap();
        sim.show_page(1);
        sim.reload(panel());
    });
    let display = sim.lock();
    assert_eq!(display.page(), 1);
    assert_eq!(display.vp().get(0x1010), 42);
}
