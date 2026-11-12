//! Kódování textových souborů bodů: rozpoznání a dekódování.
//!
//! Geodetické a CAD programy v Česku zapisují soubory nejčastěji v UTF-8
//! (případně s BOM), ve Windows-1250, starší programy v DOS kódové stránce
//! 852 (Latin-2) a některé exporty z Windows v UTF-16.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kodovani {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Cp1250,
    Cp852,
}

/// Kódování nabízená k ruční volbě.
pub const VSECHNA: [Kodovani; 6] =
    [Kodovani::Utf8, Kodovani::Utf8Bom, Kodovani::Cp1250, Kodovani::Cp852, Kodovani::Utf16Le, Kodovani::Utf16Be];

impl Kodovani {
    pub fn nazev(self) -> &'static str {
        match self {
            Kodovani::Utf8 => "UTF-8",
            Kodovani::Utf8Bom => "UTF-8 (BOM)",
            Kodovani::Utf16Le => "UTF-16 LE",
            Kodovani::Utf16Be => "UTF-16 BE",
            Kodovani::Cp1250 => "Windows-1250",
            Kodovani::Cp852 => "DOS 852",
        }
    }

    /// Klíč pro uložení do nastavení.
    pub fn klic(self) -> &'static str {
        match self {
            Kodovani::Utf8 => "utf8",
            Kodovani::Utf8Bom => "utf8bom",
            Kodovani::Utf16Le => "utf16le",
            Kodovani::Utf16Be => "utf16be",
            Kodovani::Cp1250 => "cp1250",
            Kodovani::Cp852 => "cp852",
        }
    }

    pub fn z_klice(s: &str) -> Option<Self> {
        VSECHNA.into_iter().find(|k| k.klic().eq_ignore_ascii_case(s.trim()))
    }
}

/// Znaky kódové stránky 852 pro bajty 0x80–0xFF.
const CP852: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'ů', 'ć', 'ç', 'ł', 'ë', 'Ő', 'ő', 'î', 'Ź', 'Ä', 'Ć', //
    'É', 'Ĺ', 'ĺ', 'ô', 'ö', 'Ľ', 'ľ', 'Ś', 'ś', 'Ö', 'Ü', 'Ť', 'ť', 'Ł', '×', 'č', //
    'á', 'í', 'ó', 'ú', 'Ą', 'ą', 'Ž', 'ž', 'Ę', 'ę', '¬', 'ź', 'Č', 'ş', '«', '»', //
    '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'Ě', 'Ş', '╣', '║', '╗', '╝', 'Ż', 'ż', '┐', //
    '└', '┴', '┬', '├', '─', '┼', 'Ă', 'ă', '╚', '╔', '╩', '╦', '╠', '═', '╬', '¤', //
    'đ', 'Đ', 'Ď', 'Ë', 'ď', 'Ň', 'Í', 'Î', 'ě', '┘', '┌', '█', '▄', 'Ţ', 'Ů', '▀', //
    'Ó', 'ß', 'Ô', 'Ń', 'ń', 'ň', 'Š', 'š', 'Ŕ', 'Ú', 'ŕ', 'Ű', 'ý', 'Ý', 'ţ', '´', //
    '\u{ad}', '˝', '˛', 'ˇ', '˘', '§', '÷', '¸', '°', '¨', '˙', 'ű', 'Ř', 'ř', '■', '\u{a0}',
];

fn dekoduj_852(b: &[u8]) -> String {
    b.iter().map(|&c| if c < 0x80 { c as char } else { CP852[(c - 0x80) as usize] }).collect()
}

/// Dekóduje bajty v zadaném kódování (BOM se odstraní).
pub fn dekoduj(b: &[u8], k: Kodovani) -> String {
    let t = match k {
        Kodovani::Utf8 | Kodovani::Utf8Bom => String::from_utf8_lossy(b).into_owned(),
        Kodovani::Utf16Le => encoding_rs::UTF_16LE.decode_without_bom_handling(b).0.into_owned(),
        Kodovani::Utf16Be => encoding_rs::UTF_16BE.decode_without_bom_handling(b).0.into_owned(),
        Kodovani::Cp1250 => encoding_rs::WINDOWS_1250.decode_without_bom_handling(b).0.into_owned(),
        Kodovani::Cp852 => dekoduj_852(b),
    };
    t.trim_start_matches('\u{feff}').to_string()
}

const CESKE: &str = "áčďéěíňóřšťúůýžÁČĎÉĚÍŇÓŘŠŤÚŮÝŽäöüôĺľŕÄÖÜÔĹĽŔ";

/// Jak „česky“ text vypadá: písmena s diakritikou plus, rámečky,
/// řídicí a nahrazené znaky minus.
fn skore_textu(t: &str) -> i64 {
    t.chars()
        .map(|c| {
            if CESKE.contains(c) {
                2
            } else if ('\u{2500}'..='\u{25ff}').contains(&c) || c == '\u{fffd}' || ('\u{80}'..='\u{9f}').contains(&c) {
                -3
            } else {
                0
            }
        })
        .sum()
}

/// Rozpozná kódování podle BOM, platnosti UTF-8, nul (UTF-16) a četnosti
/// českých znaků (Windows-1250 proti DOS 852).
pub fn rozpoznej(b: &[u8]) -> Kodovani {
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Kodovani::Utf8Bom;
    }
    if b.starts_with(&[0xFF, 0xFE]) {
        return Kodovani::Utf16Le;
    }
    if b.starts_with(&[0xFE, 0xFF]) {
        return Kodovani::Utf16Be;
    }
    // UTF-16 bez BOM: v ASCII textu je každý druhý bajt nula
    let vzorek = &b[..b.len().min(4000)];
    if vzorek.len() >= 4 {
        let sude = vzorek.iter().step_by(2).filter(|&&c| c == 0).count();
        let liche = vzorek.iter().skip(1).step_by(2).filter(|&&c| c == 0).count();
        let pul = vzorek.len() / 2;
        if liche * 10 >= pul * 4 && sude * 10 < pul {
            return Kodovani::Utf16Le;
        }
        if sude * 10 >= pul * 4 && liche * 10 < pul {
            return Kodovani::Utf16Be;
        }
    }
    if std::str::from_utf8(b).is_ok() {
        return Kodovani::Utf8;
    }
    if skore_textu(&dekoduj_852(b)) > skore_textu(&dekoduj(b, Kodovani::Cp1250)) {
        Kodovani::Cp852
    } else {
        Kodovani::Cp1250
    }
}

/// Rozpozná kódování a vrátí text.
pub fn nacti(b: &[u8]) -> (String, Kodovani) {
    let k = rozpoznej(b);
    (dekoduj(b, k), k)
}

/// Text obsahuje znaky, které ukazují na špatně zvolené kódování.
pub fn podezrele_znaky(t: &str) -> usize {
    t.chars().filter(|&c| c == '\u{fffd}' || ('\u{2500}'..='\u{25ff}').contains(&c) || ('\u{80}'..='\u{9f}').contains(&c)).count()
}

#[cfg(test)]
mod testy {
    use super::*;

    const TEXT: &str = "1;745210,12;1043520,45;312,48;ŠA šachta\n2;745215,80;1043518,02;312,61;OB obrubník řez\n";

    fn kod_852(t: &str) -> Vec<u8> {
        t.chars()
            .map(|c| if (c as u32) < 0x80 { c as u8 } else { 0x80 + CP852.iter().position(|&x| x == c).expect("znak v 852") as u8 })
            .collect()
    }

    #[test]
    fn kodovani_utf8_bez_a_s_bom() {
        assert_eq!(nacti(TEXT.as_bytes()), (TEXT.to_string(), Kodovani::Utf8));
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(TEXT.as_bytes());
        assert_eq!(nacti(&b), (TEXT.to_string(), Kodovani::Utf8Bom));
    }

    #[test]
    fn kodovani_utf16le_s_bom_i_bez() {
        let u: Vec<u8> = TEXT.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        assert_eq!(nacti(&u), (TEXT.to_string(), Kodovani::Utf16Le));
        let mut b = vec![0xFF, 0xFE];
        b.extend_from_slice(&u);
        assert_eq!(nacti(&b), (TEXT.to_string(), Kodovani::Utf16Le));
        let be: Vec<u8> = TEXT.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
        assert_eq!(nacti(&be), (TEXT.to_string(), Kodovani::Utf16Be));
    }

    #[test]
    fn kodovani_cp1250_vs_852() {
        let w = crate::koduj_cp1250(TEXT);
        assert_eq!(nacti(&w), (TEXT.to_string(), Kodovani::Cp1250));
        let d = kod_852(TEXT);
        assert_eq!(nacti(&d), (TEXT.to_string(), Kodovani::Cp852));
    }

    #[test]
    fn spatne_kodovani_se_pozna() {
        let w = crate::koduj_cp1250(TEXT);
        assert!(podezrele_znaky(&dekoduj(&w, Kodovani::Utf8)) > 0);
        assert_ne!(dekoduj(&w, Kodovani::Cp852), TEXT);
        assert_eq!(podezrele_znaky(&dekoduj(&w, Kodovani::Cp1250)), 0);
    }

    #[test]
    fn klice() {
        for k in VSECHNA {
            assert_eq!(Kodovani::z_klice(k.klic()), Some(k));
        }
    }
}
