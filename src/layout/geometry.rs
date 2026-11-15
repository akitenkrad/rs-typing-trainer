//! ANSI and JIS physical keyboard geometry and touch-typing finger assignments.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finger {
    LeftPinky,
    LeftRing,
    LeftMiddle,
    LeftIndex,
    RightIndex,
    RightMiddle,
    RightRing,
    RightPinky,
    Thumb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRow {
    Number,
    Top,
    Home,
    Bottom,
    Control,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalKey {
    pub code: &'static str,
    pub finger: Finger,
    pub row: KeyRow,
    pub width: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardGeometry {
    pub name: &'static str,
    pub rows: Vec<Vec<PhysicalKey>>,
}

impl KeyboardGeometry {
    pub fn with_digit6_finger(mut self, finger: Finger) -> Self {
        if let Some(key) = self.rows.iter_mut().flatten().find(|key| key.code == "Digit6") {
            key.finger = finger;
        }
        self
    }
}

pub fn geometry(name: &str) -> Option<KeyboardGeometry> {
    match name {
        "ansi" => Some(ansi()),
        "jis" => Some(jis()),
        _ => None,
    }
}

fn key(code: &'static str, finger: Finger, row: KeyRow) -> PhysicalKey {
    PhysicalKey { code, finger, row, width: 1 }
}

fn wide(code: &'static str, finger: Finger, row: KeyRow, width: u16) -> PhysicalKey {
    PhysicalKey { code, finger, row, width }
}

fn number_row(jis: bool) -> Vec<PhysicalKey> {
    use Finger::*;
    use KeyRow::Number;
    let mut keys = vec![
        key("Backquote", LeftPinky, Number),
        key("Digit1", LeftPinky, Number),
        key("Digit2", LeftRing, Number),
        key("Digit3", LeftMiddle, Number),
        key("Digit4", LeftIndex, Number),
        key("Digit5", LeftIndex, Number),
        key("Digit6", LeftIndex, Number),
        key("Digit7", RightIndex, Number),
        key("Digit8", RightMiddle, Number),
        key("Digit9", RightRing, Number),
        key("Digit0", RightPinky, Number),
        key("Minus", RightPinky, Number),
        key("Equal", RightPinky, Number),
    ];
    if jis {
        keys.push(key("IntlYen", RightPinky, Number));
    }
    keys
}

fn letter_rows(jis: bool) -> Vec<Vec<PhysicalKey>> {
    use Finger::*;
    let mut top = vec![
        key("KeyQ", LeftPinky, KeyRow::Top),
        key("KeyW", LeftRing, KeyRow::Top),
        key("KeyE", LeftMiddle, KeyRow::Top),
        key("KeyR", LeftIndex, KeyRow::Top),
        key("KeyT", LeftIndex, KeyRow::Top),
        key("KeyY", RightIndex, KeyRow::Top),
        key("KeyU", RightIndex, KeyRow::Top),
        key("KeyI", RightMiddle, KeyRow::Top),
        key("KeyO", RightRing, KeyRow::Top),
        key("KeyP", RightPinky, KeyRow::Top),
        key("BracketLeft", RightPinky, KeyRow::Top),
        key("BracketRight", RightPinky, KeyRow::Top),
    ];
    top.push(key("Backslash", RightPinky, KeyRow::Top));
    let home = vec![
        key("KeyA", LeftPinky, KeyRow::Home),
        key("KeyS", LeftRing, KeyRow::Home),
        key("KeyD", LeftMiddle, KeyRow::Home),
        key("KeyF", LeftIndex, KeyRow::Home),
        key("KeyG", LeftIndex, KeyRow::Home),
        key("KeyH", RightIndex, KeyRow::Home),
        key("KeyJ", RightIndex, KeyRow::Home),
        key("KeyK", RightMiddle, KeyRow::Home),
        key("KeyL", RightRing, KeyRow::Home),
        key("Semicolon", RightPinky, KeyRow::Home),
        key("Quote", RightPinky, KeyRow::Home),
    ];
    let mut bottom = vec![
        key("KeyZ", LeftPinky, KeyRow::Bottom),
        key("KeyX", LeftRing, KeyRow::Bottom),
        key("KeyC", LeftMiddle, KeyRow::Bottom),
        key("KeyV", LeftIndex, KeyRow::Bottom),
        key("KeyB", LeftIndex, KeyRow::Bottom),
        key("KeyN", RightIndex, KeyRow::Bottom),
        key("KeyM", RightIndex, KeyRow::Bottom),
        key("Comma", RightMiddle, KeyRow::Bottom),
        key("Period", RightRing, KeyRow::Bottom),
        key("Slash", RightPinky, KeyRow::Bottom),
    ];
    if jis {
        bottom.push(key("IntlRo", RightPinky, KeyRow::Bottom));
    }
    vec![top, home, bottom]
}

fn control_row() -> Vec<PhysicalKey> {
    vec![
        wide("ShiftLeft", Finger::LeftPinky, KeyRow::Control, 3),
        wide("Space", Finger::Thumb, KeyRow::Control, 6),
        wide("ShiftRight", Finger::RightPinky, KeyRow::Control, 3),
    ]
}

fn ansi() -> KeyboardGeometry {
    let mut rows = vec![number_row(false)];
    rows.extend(letter_rows(false));
    rows.push(control_row());
    KeyboardGeometry { name: "ansi", rows }
}

fn jis() -> KeyboardGeometry {
    let mut rows = vec![number_row(true)];
    rows.extend(letter_rows(true));
    rows.push(control_row());
    KeyboardGeometry { name: "jis", rows }
}
