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
    /// Physical width in quarter-unit increments. Four equals one key unit.
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
    wide(code, finger, row, 4)
}

fn wide(code: &'static str, finger: Finger, row: KeyRow, width: u16) -> PhysicalKey {
    PhysicalKey { code, finger, row, width }
}

fn ansi() -> KeyboardGeometry {
    use Finger::*;
    use KeyRow::*;

    let rows = vec![
        vec![
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
            wide("Backspace", RightPinky, Number, 8),
        ],
        vec![
            wide("Tab", LeftPinky, Top, 6),
            key("KeyQ", LeftPinky, Top),
            key("KeyW", LeftRing, Top),
            key("KeyE", LeftMiddle, Top),
            key("KeyR", LeftIndex, Top),
            key("KeyT", LeftIndex, Top),
            key("KeyY", RightIndex, Top),
            key("KeyU", RightIndex, Top),
            key("KeyI", RightMiddle, Top),
            key("KeyO", RightRing, Top),
            key("KeyP", RightPinky, Top),
            key("BracketLeft", RightPinky, Top),
            key("BracketRight", RightPinky, Top),
            wide("Backslash", RightPinky, Top, 6),
        ],
        vec![
            wide("CapsLock", LeftPinky, Home, 7),
            key("KeyA", LeftPinky, Home),
            key("KeyS", LeftRing, Home),
            key("KeyD", LeftMiddle, Home),
            key("KeyF", LeftIndex, Home),
            key("KeyG", LeftIndex, Home),
            key("KeyH", RightIndex, Home),
            key("KeyJ", RightIndex, Home),
            key("KeyK", RightMiddle, Home),
            key("KeyL", RightRing, Home),
            key("Semicolon", RightPinky, Home),
            key("Quote", RightPinky, Home),
            wide("Enter", RightPinky, Home, 9),
        ],
        vec![
            wide("ShiftLeft", LeftPinky, Bottom, 9),
            key("KeyZ", LeftPinky, Bottom),
            key("KeyX", LeftRing, Bottom),
            key("KeyC", LeftMiddle, Bottom),
            key("KeyV", LeftIndex, Bottom),
            key("KeyB", LeftIndex, Bottom),
            key("KeyN", RightIndex, Bottom),
            key("KeyM", RightIndex, Bottom),
            key("Comma", RightMiddle, Bottom),
            key("Period", RightRing, Bottom),
            key("Slash", RightPinky, Bottom),
            wide("ShiftRight", RightPinky, Bottom, 11),
        ],
        ansi_control_row(),
    ];
    KeyboardGeometry { name: "ansi", rows }
}

fn ansi_control_row() -> Vec<PhysicalKey> {
    use Finger::*;
    use KeyRow::Control;
    vec![
        wide("ControlLeft", LeftPinky, Control, 5),
        wide("MetaLeft", LeftPinky, Control, 5),
        wide("AltLeft", LeftPinky, Control, 5),
        wide("Space", Thumb, Control, 25),
        wide("AltRight", RightPinky, Control, 5),
        wide("MetaRight", RightPinky, Control, 5),
        wide("ContextMenu", RightPinky, Control, 5),
        wide("ControlRight", RightPinky, Control, 5),
    ]
}

fn jis() -> KeyboardGeometry {
    use Finger::*;
    use KeyRow::*;

    let rows = vec![
        vec![
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
            key("IntlYen", RightPinky, Number),
            wide("Backspace", RightPinky, Number, 8),
        ],
        vec![
            wide("Tab", LeftPinky, Top, 6),
            key("KeyQ", LeftPinky, Top),
            key("KeyW", LeftRing, Top),
            key("KeyE", LeftMiddle, Top),
            key("KeyR", LeftIndex, Top),
            key("KeyT", LeftIndex, Top),
            key("KeyY", RightIndex, Top),
            key("KeyU", RightIndex, Top),
            key("KeyI", RightMiddle, Top),
            key("KeyO", RightRing, Top),
            key("KeyP", RightPinky, Top),
            key("BracketLeft", RightPinky, Top),
            key("BracketRight", RightPinky, Top),
            wide("Enter", RightPinky, Top, 6),
        ],
        vec![
            wide("CapsLock", LeftPinky, Home, 7),
            key("KeyA", LeftPinky, Home),
            key("KeyS", LeftRing, Home),
            key("KeyD", LeftMiddle, Home),
            key("KeyF", LeftIndex, Home),
            key("KeyG", LeftIndex, Home),
            key("KeyH", RightIndex, Home),
            key("KeyJ", RightIndex, Home),
            key("KeyK", RightMiddle, Home),
            key("KeyL", RightRing, Home),
            key("Semicolon", RightPinky, Home),
            key("Quote", RightPinky, Home),
            key("Backslash", RightPinky, Home),
            wide("Enter", RightPinky, Home, 5),
        ],
        vec![
            wide("ShiftLeft", LeftPinky, Bottom, 9),
            key("KeyZ", LeftPinky, Bottom),
            key("KeyX", LeftRing, Bottom),
            key("KeyC", LeftMiddle, Bottom),
            key("KeyV", LeftIndex, Bottom),
            key("KeyB", LeftIndex, Bottom),
            key("KeyN", RightIndex, Bottom),
            key("KeyM", RightIndex, Bottom),
            key("Comma", RightMiddle, Bottom),
            key("Period", RightRing, Bottom),
            key("Slash", RightPinky, Bottom),
            key("IntlRo", RightPinky, Bottom),
            wide("ShiftRight", RightPinky, Bottom, 7),
        ],
        jis_control_row(),
    ];
    KeyboardGeometry { name: "jis", rows }
}

fn jis_control_row() -> Vec<PhysicalKey> {
    use Finger::*;
    use KeyRow::Control;
    vec![
        wide("ControlLeft", LeftPinky, Control, 5),
        key("MetaLeft", LeftPinky, Control),
        key("AltLeft", LeftPinky, Control),
        wide("NonConvert", Thumb, Control, 5),
        wide("Space", Thumb, Control, 18),
        wide("Convert", Thumb, Control, 5),
        wide("KanaMode", Thumb, Control, 5),
        key("AltRight", RightPinky, Control),
        key("MetaRight", RightPinky, Control),
        wide("ControlRight", RightPinky, Control, 6),
    ]
}
