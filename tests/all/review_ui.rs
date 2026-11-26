use std::time::Duration;

use chrono::Utc;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::session::SessionLog;
use rs_typing_trainer::metrics::stats::GameResult;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui;

use crate::common::test_app;

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

#[test]
fn review_renders_at_small_and_large_sizes_with_the_hint_on_the_last_inner_row() {
    let config = Config::default();
    let log = SessionLog {
        duration_us: Duration::from_secs(30).as_micros() as u64,
        ..SessionLog::default()
    };
    let mut record = SessionRecord::from_log(&config, log, Utc::now());
    record.header.metrics_version = 2;
    let result = GameResult::from_log(&config, &record.log);
    let mut app = test_app(config);
    app.screen = Screen::Review;
    app.result_record = Some(record);
    app.last_result = Some(result);

    for (width, height) in [(80, 24), (120, 40)] {
        let rows = render(&app, width, height);
        let text = rows.join("\n");
        assert!(text.contains("REVIEW"), "{width}x{height}:\n{text}");
        assert!(text.contains("This Session"), "{width}x{height}:\n{text}");
        assert!(text.contains("✓ "), "good points should be checked: {width}x{height}:\n{text}");
        assert!(text.contains("1. "), "improvements should be numbered: {width}x{height}:\n{text}");
        assert!(text.contains("→ "), "improvement advice should be indented: {width}x{height}:\n{text}");
        assert!(text.contains("[D/Esc] Result"), "{width}x{height}:\n{text}");
        assert!(
            !text.contains("│■"),
            "review body must use the standard left inset: {width}x{height}:\n{text}"
        );
        assert!(rows[usize::from(height - 2)].contains("[D/Esc] Result"), "{width}x{height}:\n{text}");

        let headings = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.contains('■').then_some(index))
            .collect::<Vec<_>>();
        assert!(
            headings.windows(2).all(|pair| rows[pair[1] - 1].trim_matches(['│', ' ']).is_empty()),
            "sections must have one blank row between them: {width}x{height}:\n{text}"
        );
    }
}
