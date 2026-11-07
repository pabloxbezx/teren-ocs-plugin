//! Zápis povrchu do LandXML 1.2 (výměnný formát povrchů).
//!
//! Zapisují se přesně trojúhelníky povrchu (cílový program je nepřepočítává),
//! vnější obrys jako hranice a body zaměření použité v povrchu (CgPoints).
//! LandXML uvádí souřadnice v pořadí sever (Y), východ (X), výška.

use crate::body::Bod;
use crate::povrch::Povrch;
use std::fmt::Write as _;

fn xml(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn c(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// Datum a čas (UTC) pro hlavičku: („2026-10-26“, „09:58:00“).
fn datum_cas() -> (String, String) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (dny, sek) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // převod dnů od 1970-01-01 na kalendářní datum (H. Hinnant)
    let z = dny + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}:{:02}", sek / 3600, sek / 60 % 60, sek % 60))
}

/// LandXML s jedním povrchem. `body` jsou body definice (indexy `Vrchol::bod`).
pub fn povrch(p: &Povrch, jmeno: &str, popis: &str, body: &[Bod], verze: &str) -> String {
    let pouzite = p.pouzite_vrcholy();
    // přečíslování na použité vrcholy (id od 1)
    let mut id = vec![0usize; p.vrcholy.len()];
    let mut n = 0;
    for (i, &u) in pouzite.iter().enumerate() {
        if u {
            n += 1;
            id[i] = n;
        }
    }
    let (datum, cas) = datum_cas();
    let mut s = String::new();
    let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        s,
        r#"<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2" date="{datum}" time="{cas}" language="Czech" readOnly="false">"#
    );
    let _ = writeln!(s, "  <Units>");
    let _ = writeln!(
        s,
        r#"    <Metric areaUnit="squareMeter" linearUnit="meter" volumeUnit="cubicMeter" temperatureUnit="celsius" pressureUnit="milliBars" diameterUnit="millimeter" angularUnit="decimal degrees" directionUnit="decimal degrees"/>"#
    );
    let _ = writeln!(s, "  </Units>");
    let _ = writeln!(s, r#"  <Application name="Terén (plugin pro Open CAD Studio)" manufacturer="Fluvio" version="{}"/>"#, xml(verze));

    // body zaměření, ze kterých povrch vznikl
    let cg: Vec<&Bod> =
        (0..p.vrcholy.len()).filter(|&i| pouzite[i]).filter_map(|i| p.vrcholy[i].bod.and_then(|b| body.get(b))).collect();
    if !cg.is_empty() {
        let _ = writeln!(s, "  <CgPoints>");
        for (k, b) in cg.iter().enumerate() {
            let nazev = if b.cislo.trim().is_empty() { format!("T{}", k + 1) } else { b.cislo.trim().to_string() };
            let kod = if b.popis.trim().is_empty() { String::new() } else { format!(r#" code="{}""#, xml(b.popis.trim())) };
            let _ = writeln!(s, r#"    <CgPoint name="{}"{kod}>{} {} {}</CgPoint>"#, xml(&nazev), c(b.y), c(b.x), c(b.z));
        }
        let _ = writeln!(s, "  </CgPoints>");
    }

    let _ = writeln!(s, "  <Surfaces>");
    let popis_attr = if popis.is_empty() { String::new() } else { format!(r#" desc="{}""#, xml(popis)) };
    let _ = writeln!(s, r#"    <Surface name="{}"{popis_attr}>"#, xml(jmeno));
    let obrys = p.obrys();
    if !obrys.is_empty() {
        let _ = writeln!(s, "      <SourceData>");
        let _ = writeln!(s, "        <Boundaries>");
        for (k, o) in obrys.iter().enumerate() {
            let mut body: Vec<String> = o.iter().map(|&(x, y, z)| format!("{} {} {}", c(y), c(x), c(z))).collect();
            if let Some(prvni) = body.first().cloned() {
                body.push(prvni);
            }
            let _ = writeln!(
                s,
                r#"          <Boundary name="Obrys {}" bndType="outer" edgeTrim="true"><PntList3D>{}</PntList3D></Boundary>"#,
                k + 1,
                body.join(" ")
            );
        }
        let _ = writeln!(s, "        </Boundaries>");
        let _ = writeln!(s, "      </SourceData>");
    }
    let _ = writeln!(s, r#"      <Definition surfType="TIN">"#);
    let _ = writeln!(s, "        <Pnts>");
    for i in 0..p.vrcholy.len() {
        if pouzite[i] {
            let (x, y, z) = p.globalni(i);
            let _ = writeln!(s, r#"          <P id="{}">{} {} {}</P>"#, id[i], c(y), c(x), c(z));
        }
    }
    let _ = writeln!(s, "        </Pnts>");
    let _ = writeln!(s, "        <Faces>");
    for t in &p.trojuhelniky {
        // LandXML: vrcholy ve směru hodinových ručiček v souřadnicích (sever, východ)
        let _ = writeln!(s, "          <F>{} {} {}</F>", id[t[0]], id[t[1]], id[t[2]]);
    }
    let _ = writeln!(s, "        </Faces>");
    let _ = writeln!(s, "      </Definition>");
    let _ = writeln!(s, "    </Surface>");
    let _ = writeln!(s, "  </Surfaces>");
    let _ = writeln!(s, "</LandXML>");
    s
}

#[cfg(test)]
mod testy {
    use super::*;
    use crate::povrch::Definice;

    #[test]
    fn landxml_povrch() {
        let body: Vec<Bod> = [(0.0, 0.0, 1.0), (10.0, 0.0, 2.0), (0.0, 10.0, 3.0), (10.0, 10.0, 4.0)]
            .iter()
            .enumerate()
            .map(|(i, &(x, y, z))| Bod { cislo: (i + 1).to_string(), x, y, z, popis: "T".into() })
            .collect();
        let p = Povrch::z_definice(&Definice { body: body.clone(), ..Default::default() }, 0.001).unwrap();
        let x = povrch(&p, "Terén & okolí", "", &body, "0.10");
        assert!(x.contains(r#"<Surface name="Terén &amp; okolí">"#));
        assert_eq!(x.matches("<P id=").count(), 4);
        assert_eq!(x.matches("<F>").count(), 2);
        assert_eq!(x.matches("<CgPoint ").count(), 4);
        // sever (Y) první: bod 2 je (x 10, y 0)
        assert!(x.contains(r#"<CgPoint name="2" code="T">0 10 2</CgPoint>"#), "{x}");
        let (d, _) = datum_cas();
        assert_eq!(d.len(), 10);
    }
}
