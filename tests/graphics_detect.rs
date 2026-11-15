use std::time::Duration;

use rs_typing_trainer::graphics::{
    CellPixels, DetectionInput, GraphicsMode, GraphicsPreference, GraphicsReason, QUERY_TIMEOUT, QueryProtocol, QueryResult, TerminalPixelSize,
    decide_graphics, make_demo_image, update_cell_pixels,
};

const CELL: CellPixels = CellPixels::new(8, 16);

#[test]
fn terminal_query_timeout_is_two_seconds() {
    assert_eq!(QUERY_TIMEOUT, Duration::from_secs(2));
}

fn input(configured: GraphicsPreference, is_terminal: bool, in_tmux: bool, query_protocol: QueryProtocol) -> DetectionInput {
    DetectionInput {
        configured,
        override_mode: None,
        is_terminal,
        in_tmux,
        in_screen: false,
        query: Some(QueryResult::new(query_protocol, Some(CELL))),
        fallback_cell_pixels: None,
    }
}

#[test]
fn detection_rule_table_covers_preferences_terminal_tmux_and_query_results() {
    let preferences = [
        GraphicsPreference::Auto,
        GraphicsPreference::Kitty,
        GraphicsPreference::Sixel,
        GraphicsPreference::Iterm2,
        GraphicsPreference::Off,
    ];
    let query_protocols = [
        QueryProtocol::Kitty,
        QueryProtocol::Iterm2,
        QueryProtocol::Sixel,
        QueryProtocol::Halfblocks,
    ];

    for configured in preferences {
        for is_terminal in [false, true] {
            for in_tmux in [false, true] {
                for query_protocol in query_protocols {
                    let result = decide_graphics(input(configured, is_terminal, in_tmux, query_protocol));
                    let expected = if configured == GraphicsPreference::Off || !is_terminal || configured == GraphicsPreference::Auto && in_tmux {
                        GraphicsMode::Off
                    } else {
                        match configured {
                            GraphicsPreference::Auto => match query_protocol {
                                QueryProtocol::Kitty => GraphicsMode::Kitty,
                                QueryProtocol::Iterm2 => GraphicsMode::Iterm2,
                                QueryProtocol::Sixel => GraphicsMode::Sixel,
                                QueryProtocol::Halfblocks => GraphicsMode::Off,
                            },
                            GraphicsPreference::Kitty => GraphicsMode::Kitty,
                            GraphicsPreference::Sixel => GraphicsMode::Sixel,
                            GraphicsPreference::Iterm2 => GraphicsMode::Iterm2,
                            GraphicsPreference::Off => GraphicsMode::Off,
                        }
                    };

                    assert_eq!(
                        result.mode, expected,
                        "configured={configured:?} terminal={is_terminal} tmux={in_tmux} query={query_protocol:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn halfblocks_are_treated_as_no_image_protocol() {
    let result = decide_graphics(input(GraphicsPreference::Auto, true, false, QueryProtocol::Halfblocks));

    assert_eq!(result.mode, GraphicsMode::Off);
    assert_eq!(result.cell_pixels, None);
    assert_eq!(result.reason, GraphicsReason::NoImageProtocol);
}

#[test]
fn auto_prefers_kitty_then_iterm2_then_sixel() {
    let mut value = input(GraphicsPreference::Auto, true, false, QueryProtocol::Sixel);
    value.query = Some(QueryResult {
        kitty: true,
        iterm2: true,
        sixel: true,
        cell_pixels: Some(CELL),
    });
    assert_eq!(decide_graphics(value.clone()).mode, GraphicsMode::Kitty);

    value.query.as_mut().unwrap().kitty = false;
    assert_eq!(decide_graphics(value).mode, GraphicsMode::Iterm2);
}

#[test]
fn explicit_mode_uses_window_size_fallback_when_query_has_no_cell_size() {
    let mut value = input(GraphicsPreference::Kitty, true, false, QueryProtocol::Halfblocks);
    value.query.as_mut().unwrap().cell_pixels = None;
    value.fallback_cell_pixels = Some(CELL);

    let result = decide_graphics(value);

    assert_eq!(result.mode, GraphicsMode::Kitty);
    assert_eq!(result.cell_pixels, Some(CELL));
    assert_eq!(result.reason, GraphicsReason::ExplicitWithWindowSize);
}

#[test]
fn explicit_mode_without_any_cell_size_is_off_with_a_reason() {
    let mut value = input(GraphicsPreference::Sixel, true, false, QueryProtocol::Halfblocks);
    value.query.as_mut().unwrap().cell_pixels = None;

    let result = decide_graphics(value);

    assert_eq!(result.mode, GraphicsMode::Off);
    assert_eq!(result.reason, GraphicsReason::MissingCellSize(GraphicsMode::Sixel));
}

#[test]
fn cli_override_takes_precedence_over_configuration() {
    let mut value = input(GraphicsPreference::Off, true, false, QueryProtocol::Kitty);
    value.override_mode = Some(GraphicsPreference::Iterm2);

    assert_eq!(decide_graphics(value).mode, GraphicsMode::Iterm2);
}

#[test]
fn non_terminal_wins_over_an_explicit_mode() {
    let result = decide_graphics(input(GraphicsPreference::Kitty, false, false, QueryProtocol::Kitty));

    assert_eq!(result.mode, GraphicsMode::Off);
    assert_eq!(result.reason, GraphicsReason::NotTerminal);
}

#[test]
fn auto_is_off_in_screen_just_as_it_is_in_tmux() {
    let mut value = input(GraphicsPreference::Auto, true, false, QueryProtocol::Kitty);
    value.in_screen = true;

    let result = decide_graphics(value);

    assert_eq!(result.mode, GraphicsMode::Off);
    assert_eq!(result.reason, GraphicsReason::MultiplexerAuto);
}

#[test]
fn resize_recomputes_cell_pixels_without_a_query() {
    let terminal = TerminalPixelSize {
        columns: 120,
        rows: 40,
        width: 960,
        height: 720,
    };

    assert_eq!(update_cell_pixels(Some(CELL), terminal), Some(CellPixels::new(8, 18)));
}

#[test]
fn resize_with_zero_pixels_keeps_the_previous_value() {
    for terminal in [
        TerminalPixelSize {
            columns: 120,
            rows: 40,
            width: 0,
            height: 720,
        },
        TerminalPixelSize {
            columns: 120,
            rows: 40,
            width: 960,
            height: 0,
        },
        TerminalPixelSize {
            columns: 0,
            rows: 0,
            width: 960,
            height: 720,
        },
    ] {
        assert_eq!(update_cell_pixels(Some(CELL), terminal), Some(CELL));
    }
}

#[test]
fn demo_image_has_exact_cell_dimensions_grid_gradient_and_corner_marks() {
    let image = make_demo_image(CellPixels::new(4, 3), 5, 4);
    let rgba = image.to_rgba8();

    assert_eq!(rgba.dimensions(), (20, 12));
    assert_eq!(rgba.get_pixel(0, 0).0, [255, 64, 64, 255]);
    assert_eq!(rgba.get_pixel(19, 0).0, [64, 255, 64, 255]);
    assert_eq!(rgba.get_pixel(0, 11).0, [64, 128, 255, 255]);
    assert_eq!(rgba.get_pixel(19, 11).0, [255, 224, 64, 255]);
    assert_eq!(rgba.get_pixel(4, 5).0, [255, 255, 255, 255], "vertical one-pixel grid line");
    assert_eq!(rgba.get_pixel(6, 3).0, [255, 255, 255, 255], "horizontal one-pixel grid line");
    assert_ne!(rgba.get_pixel(6, 5), rgba.get_pixel(14, 8), "the interior must contain a gradient");
}
