use std::time::Duration;

use rs_typing_trainer::analysis;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::{InputEffect, Session};
use rs_typing_trainer::metrics::stats::GameResult;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn exact(text: &str) -> Item {
    Item::exact(text)
}

#[test]
fn injected_session_log_drives_the_result_metrics() {
    let mut session = Session::new(Duration::from_secs(5), Duration::from_secs(2), exact("abc"));
    session.activate(Duration::ZERO);

    assert_eq!(session.input_char(ms(100), "a"), InputEffect::Correct);
    assert_eq!(session.input_char(ms(150), "x"), InputEffect::Mistype);
    assert_eq!(session.input_char(ms(300), "b"), InputEffect::Correct);
    assert_eq!(session.input_char(ms(500), "c"), InputEffect::ItemCompleted);
    assert_eq!(session.input_char(ms(550), "z"), InputEffect::Dropped);

    session.advance(exact("abc"), ms(600));
    session.activate(ms(700));
    session.input_char(ms(800), "a");
    session.input_char(ms(1_000), "b");
    session.input_char(ms(1_300), "c");

    session.advance(exact("abc"), ms(1_400));
    session.activate(ms(1_500));
    assert!(session.tick(ms(3_500)).item_timed_out);

    session.advance(exact("abc"), ms(3_600));
    session.activate(ms(3_700));
    session.input_char(ms(3_800), "a");
    session.input_char(ms(3_900), "x");
    session.input_char(ms(4_100), "b");
    session.input_char(ms(4_400), "c");
    assert!(session.tick(ms(5_000)).session_finished);

    let log = session.into_log();
    assert_eq!(log.keystrokes.len(), 11, "R-06 の捨てた打鍵は含めない");
    assert_eq!(log.items.len(), 4);

    let computed = analysis::analyze(&log);
    let shown = GameResult::from_log(&Config::default(), &log);
    assert_eq!(shown.analysis, computed);

    assert!((computed.net_wpm - 21.6).abs() < 1e-9);
    assert!((computed.accuracy - 9.0 / 11.0).abs() < 1e-9);
    assert_eq!(computed.iki_median, ms(200));
    assert!(log.keystrokes.windows(2).all(|pair| pair[0].t_us <= pair[1].t_us));
}

#[test]
fn input_at_session_deadline_is_dropped() {
    let mut session = Session::new(Duration::from_secs(1), Duration::from_secs(1), exact("a"));
    session.activate(Duration::ZERO);
    assert_eq!(session.input_char(Duration::from_millis(1_100), "a"), InputEffect::Dropped);
    assert!(session.tick(Duration::from_millis(1_100)).session_finished);
    assert!(session.log().keystrokes.is_empty());
    assert_eq!(session.log().items[0].done_us, Some(1_000_000));
}

#[test]
fn input_at_item_deadline_times_out_before_judging() {
    let mut session = Session::new(Duration::from_secs(2), Duration::from_millis(500), exact("a"));
    session.activate(Duration::from_millis(100));
    assert_eq!(session.input_char(Duration::from_millis(600), "a"), InputEffect::Dropped);
    assert!(session.log().keystrokes.is_empty());
    assert_eq!(session.log().items[0].outcome, rs_typing_trainer::engine::session::ItemOutcome::Timeout);
}
