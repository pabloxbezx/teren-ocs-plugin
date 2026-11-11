//! Jednoduchý zápis DXF (verze R12, ASCII) pro kontrolu výsledků
//! v Open CAD Studiu nebo jiném CAD programu.
//!
//! Text se zapisuje v kódové stránce Windows-1250 (`$DWGCODEPAGE ANSI_1250`),
//! aby se správně zobrazila česká diakritika.

use crate::body::Bod;
use crate::povrch::Povrch;
use crate::vrstevnice::Vrstevnice;
use std::fmt::Write as _;

pub struct Hladina {
    pub nazev: &'static str,
    /// Index barvy DXF (ACI).
    pub barva: i32,
}

pub const H_BODY: Hladina = Hladina { nazev: "TER_BODY", barva: 30 };
pub const H_BODY_CISLO: Hladina = Hladina { nazev: "TER_BODY_CISLO", barva: 7 };
pub const H_BODY_VYSKA: Hladina = Hladina { nazev: "TER_BODY_VYSKA", barva: 7 };
pub const H_BODY_POPIS: Hladina = Hladina { nazev: "TER_BODY_POPIS", barva: 8 };
pub const H_TIN: Hladina = Hladina { nazev: "TER_TIN", barva: 8 };
pub const H_VRST_HLAVNI: Hladina = Hladina { nazev: "TER_VRSTEVNICE_HLAVNI", barva: 22 };
pub const H_VRST_VEDLEJSI: Hladina = Hladina { nazev: "TER_VRSTEVNICE_VEDLEJSI", barva: 32 };

const HLADINY: [Hladina; 7] = [H_BODY, H_BODY_CISLO, H_BODY_VYSKA, H_BODY_POPIS, H_TIN, H_VRST_HLAVNI, H_VRST_VEDLEJSI];

pub struct NastaveniDxf {
    /// Výška textu popisu bodů [m].
    pub vyska_textu: f64,
    pub popisy_bodu: bool,
}

impl Default for NastaveniDxf {
    fn default() -> Self {
        Self { vyska_textu: 0.5, popisy_bodu: true }
    }
}

#[derive(Default)]
pub struct Dxf {
    entity: String,
    rozsah: Option<(f64, f64, f64, f64)>,
}

fn c(v: f64) -> String {
    let s = format!("{v:.4}");
    // odstranění koncových nul kvůli velikosti souboru
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

impl Dxf {
    fn g(&mut self, kod: i32, hodnota: impl std::fmt::Display) {
        let _ = write!(self.entity, "{kod:>3}\n{hodnota}\n");
    }

    fn rozsah(&mut self, x: f64, y: f64) {
        self.rozsah = Some(match self.rozsah {
            None => (x, y, x, y),
            Some((a, b, cc, d)) => (a.min(x), b.min(y), cc.max(x), d.max(y)),
        });
    }

    fn bodxyz(&mut self, zaklad: i32, x: f64, y: f64, z: f64) {
        self.rozsah(x, y);
        self.g(zaklad, c(x));
        self.g(zaklad + 10, c(y));
        self.g(zaklad + 20, c(z));
    }

    pub fn bod(&mut self, h: &Hladina, x: f64, y: f64, z: f64) {
        self.g(0, "POINT");
        self.g(8, h.nazev);
        self.bodxyz(10, x, y, z);
    }

    pub fn text(&mut self, h: &Hladina, x: f64, y: f64, z: f64, vyska: f64, text: &str) {
        if text.is_empty() {
            return;
        }
        self.g(0, "TEXT");
        self.g(8, h.nazev);
        self.bodxyz(10, x, y, z);
        self.g(40, c(vyska));
        self.g(1, text);
    }

    pub fn face3d(&mut self, h: &Hladina, p: [(f64, f64, f64); 3]) {
        self.g(0, "3DFACE");
        self.g(8, h.nazev);
        for (i, &(x, y, z)) in p.iter().chain(std::iter::once(&p[2])).enumerate() {
            self.bodxyz(10 + i as i32, x, y, z);
        }
    }

    /// 2D křivka ve výšce `z` (POLYLINE R12).
    pub fn krivka(&mut self, h: &Hladina, body: &[(f64, f64)], z: f64, uzavrena: bool) {
        self.g(0, "POLYLINE");
        self.g(8, h.nazev);
        self.g(66, 1);
        self.g(10, 0);
        self.g(20, 0);
        self.g(30, c(z));
        self.g(70, if uzavrena { 1 } else { 0 });
        for &(x, y) in body {
            self.g(0, "VERTEX");
            self.g(8, h.nazev);
            self.bodxyz(10, x, y, z);
        }
        self.g(0, "SEQEND");
        self.g(8, h.nazev);
    }

    pub fn pridej_body(&mut self, body: &[Bod], vyber: &[usize], n: &NastaveniDxf) {
        let v = n.vyska_textu;
        for &i in vyber {
            let b = &body[i];
            self.bod(&H_BODY, b.x, b.y, b.z);
            if n.popisy_bodu {
                self.text(&H_BODY_CISLO, b.x + 0.4 * v, b.y + 0.3 * v, b.z, v, &b.cislo);
                self.text(&H_BODY_VYSKA, b.x + 0.4 * v, b.y - 1.1 * v, b.z, v, &format!("{:.2}", b.z));
                self.text(&H_BODY_POPIS, b.x + 0.4 * v, b.y - 2.5 * v, b.z, 0.8 * v, &b.popis);
            }
        }
    }

    pub fn pridej_tin(&mut self, p: &Povrch) {
        for t in &p.trojuhelniky {
            self.face3d(&H_TIN, t.map(|i| p.globalni(i)));
        }
    }

    pub fn pridej_vrstevnice(&mut self, vrstevnice: &[Vrstevnice]) {
        for v in vrstevnice {
            let h = if v.hlavni { &H_VRST_HLAVNI } else { &H_VRST_VEDLEJSI };
            self.krivka(h, &v.body, v.vyska, v.uzavrena);
        }
    }

    /// Celý soubor DXF zakódovaný ve Windows-1250.
    pub fn do_bajtu(&self) -> Vec<u8> {
        let mut s = String::new();
        let mut g = |kod: i32, h: &dyn std::fmt::Display| {
            let _ = write!(s, "{kod:>3}\n{h}\n");
        };
        let (x0, y0, x1, y1) = self.rozsah.unwrap_or((0.0, 0.0, 1.0, 1.0));
        g(0, &"SECTION");
        g(2, &"HEADER");
        g(9, &"$ACADVER");
        g(1, &"AC1009");
        g(9, &"$DWGCODEPAGE");
        g(3, &"ANSI_1250");
        g(9, &"$INSUNITS");
        g(70, &6);
        g(9, &"$EXTMIN");
        g(10, &c(x0));
        g(20, &c(y0));
        g(9, &"$EXTMAX");
        g(10, &c(x1));
        g(20, &c(y1));
        g(9, &"$PDMODE");
        g(70, &3);
        g(9, &"$PDSIZE");
        g(40, &0.3);
        g(0, &"ENDSEC");
        g(0, &"SECTION");
        g(2, &"TABLES");
        g(0, &"TABLE");
        g(2, &"LAYER");
        g(70, &(HLADINY.len() + 1));
        for (nazev, barva) in std::iter::once(("0", 7)).chain(HLADINY.iter().map(|h| (h.nazev, h.barva))) {
            g(0, &"LAYER");
            g(2, &nazev);
            g(70, &0);
            g(62, &barva);
            g(6, &"CONTINUOUS");
        }
        g(0, &"ENDTAB");
        g(0, &"ENDSEC");
        g(0, &"SECTION");
        g(2, &"ENTITIES");
        s.push_str(&self.entity);
        let _ = write!(s, "  0\nENDSEC\n  0\nEOF\n");
        let (bajty, _, _) = encoding_rs::WINDOWS_1250.encode(&s);
        bajty.into_owned()
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn format_cisel() {
        assert_eq!(c(1.0), "1");
        assert_eq!(c(-823000.12346), "-823000.1235");
        assert_eq!(c(0.10), "0.1");
        assert_eq!(c(-0.00001), "0");
    }

    #[test]
    fn diakritika_cp1250() {
        let mut d = Dxf::default();
        d.text(&H_BODY_POPIS, 0.0, 0.0, 0.0, 1.0, "hrana příkopu");
        let b = d.do_bajtu();
        // 'ř' v cp1250 = 0xF8
        assert!(b.windows(2).any(|w| w == [b'p', 0xF8]));
        assert!(b.ends_with(b"EOF\n"));
    }
}
