use rs_typing_trainer::i18n::{Lang, Msg};

#[test]
fn every_message_has_english_and_japanese_text() {
    for message in Msg::ALL {
        assert!(!message.text(Lang::En).trim().is_empty(), "missing English text for {message:?}");
        assert!(!message.text(Lang::Ja).trim().is_empty(), "missing Japanese text for {message:?}");
    }
}

#[test]
fn japanese_messages_use_project_punctuation() {
    for message in Msg::ALL {
        let text = message.text(Lang::Ja);
        assert!(!text.contains('、'), "Japanese comma in {message:?}: {text}");
        assert!(!text.contains('。'), "Japanese period in {message:?}: {text}");
    }
}
