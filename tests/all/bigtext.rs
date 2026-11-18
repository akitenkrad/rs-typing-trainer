use ratatui::style::{Color, Modifier, Style};
use rs_typing_trainer::ui::bigtext::{BigTextSize, StyledCharacter, font};

fn symbols(line: &ratatui::text::Line<'_>) -> String {
    line.spans.iter().map(|span| span.content.as_ref()).collect()
}

#[test]
fn bdf_glyphs_render_with_quadrant_block_characters() {
    let font = font(BigTextSize::Dot12).unwrap();
    let cases = [
        ('A', [" ▖ ", "▗▚ ", "▐▐ ", "▛▀▌", "▌ ▌", "   "].as_slice()),
        ('あ', ["  ▌   ", " ▀▛▀  ", " ▗▀▛▖ ", "▗▜▗▘▐ ", "▐ ▛ ▞ ", " ▀ ▀  "].as_slice()),
        ('漢', ["▚▄▙▟▄▖", " ▗▙▟▄ ", "▚▐▄▙▟ ", " ▄▄▙▄ ", "▐▗▄▙▄ ", "▌▄▞▝▚▖"].as_slice()),
    ];

    for (character, expected) in cases {
        let rendered = font.render(&[StyledCharacter::new(character, Style::default())]);
        let actual = rendered.lines.iter().map(symbols).collect::<Vec<_>>();
        assert_eq!(actual, expected, "wrong raster for {character}");
        assert!(actual.iter().flat_map(|line| line.chars()).all(|cell| {
            matches!(
                cell,
                ' ' | '▘' | '▝' | '▖' | '▗' | '▀' | '▄' | '▌' | '▐' | '▚' | '▞' | '▛' | '▜' | '▙' | '▟' | '█'
            )
        }));
    }
}

#[test]
fn each_character_keeps_its_progress_or_mistype_style() {
    let font = font(BigTextSize::Dot12).unwrap();
    let correct = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let current = Style::default().fg(Color::Yellow);
    let mistype = Style::default().fg(Color::Red).add_modifier(Modifier::REVERSED);
    let rendered = font.render(&[
        StyledCharacter::new('A', correct),
        StyledCharacter::new('B', current),
        StyledCharacter::new('C', mistype),
    ]);

    for line in &rendered.lines {
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].style, correct);
        assert_eq!(line.spans[1].style, current);
        assert_eq!(line.spans[2].style, mistype);
    }
    let cursor = rendered.cursor_line(1, current);
    assert_eq!(symbols(&cursor), "   ▔▔▔   ");
    assert_eq!(cursor.spans[1].style, current);
}

#[test]
fn missing_unicode_is_centered_at_normal_size() {
    let font = font(BigTextSize::Dot12).unwrap();
    let rendered = font.render(&[StyledCharacter::new('😀', Style::default())]);
    let lines = rendered.lines.iter().map(symbols).collect::<Vec<_>>();

    assert_eq!(rendered.width, 6);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines.iter().filter(|line| line.contains('😀')).count(), 1);
    assert_eq!(unicode_width::UnicodeWidthStr::width(lines[3].as_str()), 6);
}

#[test]
fn width_uses_half_and_full_glyph_cells() {
    let dot12 = font(BigTextSize::Dot12).unwrap();
    let dot16 = font(BigTextSize::Dot16).unwrap();

    assert_eq!(dot12.text_width("A"), 3);
    assert_eq!(dot12.text_width("あ漢"), 12);
    assert_eq!(dot12.text_width("Aあ漢"), 15);
    assert_eq!(dot16.text_width("Aあ"), 12);
    assert_eq!(dot12.glyph_count(), 7_037);
    assert_eq!(dot16.glyph_count(), 7_037);
}
