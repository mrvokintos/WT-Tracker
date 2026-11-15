#[derive(Debug, PartialEq, Eq)]
pub struct KeyDef {
    pub names: &'static [&'static str],
    pub evdev: u16,
    pub vk: u16,
    pub keysym: &'static str,
}

impl KeyDef {
    pub fn name(&self) -> &'static str {
        self.names[0]
    }

    /// Produces a character when typed in chat.
    pub fn types_text(&self) -> bool {
        let n = self.names[0];
        n.chars().count() == 1 || n == "space" || n.starts_with("num ")
    }
}

macro_rules! k {
    ([$($n:expr),+], $ev:expr, $vk:expr, $sym:expr) => {
        KeyDef { names: &[$($n),+], evdev: $ev, vk: $vk, keysym: $sym }
    };
}

pub static KEYS: &[KeyDef] = &[
    k!(["a"], 30, 0x41, "a"), k!(["b"], 48, 0x42, "b"), k!(["c"], 46, 0x43, "c"),
    k!(["d"], 32, 0x44, "d"), k!(["e"], 18, 0x45, "e"), k!(["f"], 33, 0x46, "f"),
    k!(["g"], 34, 0x47, "g"), k!(["h"], 35, 0x48, "h"), k!(["i"], 23, 0x49, "i"),
    k!(["j"], 36, 0x4A, "j"), k!(["k"], 37, 0x4B, "k"), k!(["l"], 38, 0x4C, "l"),
    k!(["m"], 50, 0x4D, "m"), k!(["n"], 49, 0x4E, "n"), k!(["o"], 24, 0x4F, "o"),
    k!(["p"], 25, 0x50, "p"), k!(["q"], 16, 0x51, "q"), k!(["r"], 19, 0x52, "r"),
    k!(["s"], 31, 0x53, "s"), k!(["t"], 20, 0x54, "t"), k!(["u"], 22, 0x55, "u"),
    k!(["v"], 47, 0x56, "v"), k!(["w"], 17, 0x57, "w"), k!(["x"], 45, 0x58, "x"),
    k!(["y"], 21, 0x59, "y"), k!(["z"], 44, 0x5A, "z"),
    k!(["1"], 2, 0x31, "1"), k!(["2"], 3, 0x32, "2"), k!(["3"], 4, 0x33, "3"),
    k!(["4"], 5, 0x34, "4"), k!(["5"], 6, 0x35, "5"), k!(["6"], 7, 0x36, "6"),
    k!(["7"], 8, 0x37, "7"), k!(["8"], 9, 0x38, "8"), k!(["9"], 10, 0x39, "9"),
    k!(["0"], 11, 0x30, "0"),
    k!(["f1"], 59, 0x70, "F1"), k!(["f2"], 60, 0x71, "F2"), k!(["f3"], 61, 0x72, "F3"),
    k!(["f4"], 62, 0x73, "F4"), k!(["f5"], 63, 0x74, "F5"), k!(["f6"], 64, 0x75, "F6"),
    k!(["f7"], 65, 0x76, "F7"), k!(["f8"], 66, 0x77, "F8"), k!(["f9"], 67, 0x78, "F9"),
    k!(["f10"], 68, 0x79, "F10"), k!(["f11"], 87, 0x7A, "F11"), k!(["f12"], 88, 0x7B, "F12"),
    k!(["\\", "backslash"], 43, 0xDC, "backslash"),
    k!(["/", "slash"], 53, 0xBF, "slash"),
    k!([";", "semicolon"], 39, 0xBA, "semicolon"),
    k!(["'", "apostrophe", "quote"], 40, 0xDE, "apostrophe"),
    k!([",", "comma"], 51, 0xBC, "comma"),
    k!([".", "period", "dot"], 52, 0xBE, "period"),
    k!(["[", "bracketleft"], 26, 0xDB, "bracketleft"),
    k!(["]", "bracketright"], 27, 0xDD, "bracketright"),
    k!(["-", "minus"], 12, 0xBD, "minus"),
    k!(["=", "equal", "equals"], 13, 0xBB, "equal"),
    k!(["`", "grave", "tilde", "ё"], 41, 0xC0, "grave"),
    k!(["<", "102nd"], 86, 0xE2, "less"),
    k!(["space"], 57, 0x20, "space"),
    k!(["tab"], 15, 0x09, "Tab"),
    k!(["enter", "return"], 28, 0x0D, "Return"),
    k!(["esc", "escape"], 1, 0x1B, "Escape"),
    k!(["backspace"], 14, 0x08, "BackSpace"),
    k!(["caps lock", "capslock"], 58, 0x14, "Caps_Lock"),
    k!(["insert", "ins"], 110, 0x2D, "Insert"),
    k!(["delete", "del"], 111, 0x2E, "Delete"),
    k!(["home"], 102, 0x24, "Home"),
    k!(["end"], 107, 0x23, "End"),
    k!(["page up", "pageup", "pgup"], 104, 0x21, "Prior"),
    k!(["page down", "pagedown", "pgdn"], 109, 0x22, "Next"),
    k!(["up"], 103, 0x26, "Up"),
    k!(["down"], 108, 0x28, "Down"),
    k!(["left"], 105, 0x25, "Left"),
    k!(["right"], 106, 0x27, "Right"),
    k!(["pause", "break"], 119, 0x13, "Pause"),
    k!(["scroll lock", "scrolllock"], 70, 0x91, "Scroll_Lock"),
    k!(["print screen", "printscreen", "print"], 99, 0x2C, "Print"),
    k!(["menu", "apps"], 127, 0x5D, "Menu"),
    k!(["num 0", "num0", "kp0"], 82, 0x60, "KP_0"),
    k!(["num 1", "num1", "kp1"], 79, 0x61, "KP_1"),
    k!(["num 2", "num2", "kp2"], 80, 0x62, "KP_2"),
    k!(["num 3", "num3", "kp3"], 81, 0x63, "KP_3"),
    k!(["num 4", "num4", "kp4"], 75, 0x64, "KP_4"),
    k!(["num 5", "num5", "kp5"], 76, 0x65, "KP_5"),
    k!(["num 6", "num6", "kp6"], 77, 0x66, "KP_6"),
    k!(["num 7", "num7", "kp7"], 71, 0x67, "KP_7"),
    k!(["num 8", "num8", "kp8"], 72, 0x68, "KP_8"),
    k!(["num 9", "num9", "kp9"], 73, 0x69, "KP_9"),
    k!(["num *", "num*", "multiply"], 55, 0x6A, "KP_Multiply"),
    k!(["num +", "num+", "add"], 78, 0x6B, "KP_Add"),
    k!(["num -", "num-", "subtract"], 74, 0x6D, "KP_Subtract"),
    k!(["num .", "num.", "decimal"], 83, 0x6E, "KP_Decimal"),
    k!(["num /", "num/", "divide"], 98, 0x6F, "KP_Divide"),
];

fn squash(s: &str) -> String {
    s.trim().to_lowercase().chars().filter(|c| *c != ' ' && *c != '_').collect()
}

pub fn parse(name: &str) -> Option<&'static KeyDef> {
    let raw = name.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }
    let sq = squash(&raw);
    KEYS.iter()
        .find(|k| k.names.iter().any(|n| *n == raw))
        .or_else(|| KEYS.iter().find(|k| k.names.iter().any(|n| squash(n) == sq)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_names() {
        assert_eq!(parse("\\").unwrap().evdev, 43);
        assert_eq!(parse("F5").unwrap().vk, 0x74);
        assert_eq!(parse("page_up").unwrap().keysym, "Prior");
        assert_eq!(parse("Num 5").unwrap().evdev, 76);
        assert!(parse("").is_none());
        assert!(parse("ctrl+shift+q").is_none());
        assert!(parse("\\").unwrap().types_text());
        assert!(parse("num 5").unwrap().types_text());
        assert!(!parse("f5").unwrap().types_text());
        assert!(!parse("insert").unwrap().types_text());
    }

    #[test]
    fn table_has_no_duplicate_codes() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a.evdev, b.evdev, "{:?} {:?}", a.names, b.names);
                assert_ne!(a.vk, b.vk, "{:?} {:?}", a.names, b.names);
            }
        }
    }
}
