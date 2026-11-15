use std::time::Duration;

use rs_typing_trainer::analysis::analyze;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::{InputEffect, KeystrokeKind, Session};

#[test]
fn advance_mistype_backspace_and_correction_leave_no_unresolved_error() {
    let mut session = Session::with_error_mode(Duration::from_secs(60), Duration::ZERO, Item::exact("ab"), ErrorMode::Advance);
    session.activate(Duration::ZERO);
    assert_eq!(session.input_char(Duration::from_secs(1), "x"), InputEffect::Mistype);
    assert_eq!(session.position(), 1);
    assert_eq!(session.input_backspace(Duration::from_secs(2)), InputEffect::Backspace);
    assert_eq!(session.position(), 0);
    assert_eq!(session.input_char(Duration::from_secs(3), "a"), InputEffect::Correct);
    assert_eq!(session.input_char(Duration::from_secs(4), "b"), InputEffect::ItemCompleted);
    session.tick(Duration::from_secs(60));
    let log = session.into_log();
    let analysis = analyze(&log);

    assert_eq!(log.keystrokes[1].kind, KeystrokeKind::Backspace);
    assert_eq!(analysis.uncorrected_errors, 0);
    assert!((analysis.net_wpm - 0.4).abs() < 1e-9);
    assert!((analysis.accuracy - 1.0).abs() < 1e-9);
}

#[test]
fn advance_item_with_unfixed_mistype_subtracts_from_net_wpm_and_accuracy() {
    let mut session = Session::with_error_mode(Duration::from_secs(60), Duration::ZERO, Item::exact("ab"), ErrorMode::Advance);
    session.activate(Duration::ZERO);
    assert_eq!(session.input_char(Duration::from_secs(1), "x"), InputEffect::Mistype);
    assert_eq!(session.input_char(Duration::from_secs(2), "b"), InputEffect::ItemCompletedMistyped);
    session.tick(Duration::from_secs(60));
    let analysis = analyze(&session.into_log());

    assert_eq!(analysis.uncorrected_errors, 1);
    assert!((analysis.net_wpm - 0.2).abs() < 1e-9);
    assert!((analysis.raw_wpm - 0.4).abs() < 1e-9);
    assert!((analysis.accuracy - 0.5).abs() < 1e-9);
}
