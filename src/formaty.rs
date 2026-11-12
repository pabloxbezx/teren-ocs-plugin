//! Další zdroje bodů: LandXML (CgPoints), Trimble JobXML, Leica GSI, Excel.
//!
//! Každý zdroj se převede na text se záhlavím a tabulátory, který pak
//! prochází stejným náhledem a rozpoznáním jako běžný textový soubor
//! (role sloupců, S-JTSK, duplicity…).

use crate::kodovani::{self, Kodovani};
use crate::trf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DruhZdroje {
    Text,
    LandXml,
    JobXml,
    Gsi,
    Excel,
}

impl DruhZdroje {
    pub fn nazev(self) -> &'static str {
        match self {
            DruhZdroje::Text => crate::preklad::tr("text"),
            DruhZdroje::LandXml => "LandXML",
            DruhZdroje::JobXml => "Trimble JobXML",
            DruhZdroje::Gsi => "Leica GSI",
            DruhZdroje::Excel => "Excel",
        }
    }
}

/// Načtený zdroj připravený pro náhled.
#[derive(Debug, Clone)]
pub struct Zdroj {
    pub text: String,
    pub druh: DruhZdroje,
    /// Kódování textového souboru (u převedených zdrojů `None`).
    pub kodovani: Option<Kodovani>,
    /// Listy sešitu (jen Excel).
    pub listy: Vec<String>,
    pub list: usize,
}

const ZAHLAVI_EN: &str = "Point\tEasting\tNorthing\tElevation\tCode";

/// Načte soubor podle přípony a obsahu. `kodovani` = vynucené kódování
/// textu, `list` = list sešitu.
pub fn nacti(cesta: &str, bajty: &[u8], kodovani: Option<Kodovani>, list: usize) -> Result<Zdroj, String> {
    let pripona = std::path::Path::new(cesta).extension().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let prevedeny = |text: String, druh| Zdroj { text, druh, kodovani: None, listy: Vec::new(), list: 0 };
    if matches!(pripona.as_str(), "xlsx" | "xlsm" | "xls" | "ods") {
        let (listy, text) = excel(bajty, list)?;
        return Ok(Zdroj { text, druh: DruhZdroje::Excel, kodovani: None, listy, list });
    }
    let k = kodovani.unwrap_or_else(|| kodovani::rozpoznej(bajty));
    let text = kodovani::dekoduj(bajty, k);
    let zacatek: String = text.chars().take(4000).collect();
    if zacatek.contains("<LandXML") {
        return landxml(&text).map(|t| prevedeny(t, DruhZdroje::LandXml));
    }
    if zacatek.contains("<JOBFile") || pripona == "jxl" {
        return jobxml(&text).map(|t| prevedeny(t, DruhZdroje::JobXml));
    }
    if pripona == "gsi" || je_gsi(&text) {
        return gsi(&text).map(|t| prevedeny(t, DruhZdroje::Gsi));
    }
    Ok(Zdroj { text, druh: DruhZdroje::Text, kodovani: Some(k), listy: Vec::new(), list: 0 })
}

// ── XML (jen to, co je potřeba: prvky podle jména a atributy) ────────────────

fn xml_text(t: &str) -> String {
    t.trim().replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Prvky `<tag …>obsah</tag>` (i `<tag …/>`): (atributy, obsah).
fn prvky<'a>(text: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let otevreni = format!("<{tag}");
    let konec = format!("</{tag}>");
    let mut zbytek = text;
    while let Some(i) = zbytek.find(&otevreni) {
        let za = &zbytek[i + otevreni.len()..];
        // jen celé jméno (ne <CgPoints u <CgPoint)
        if !za.starts_with([' ', '>', '/', '\t', '\r', '\n']) {
            zbytek = za;
            continue;
        }
        let Some(gt) = za.find('>') else { break };
        let atributy = &za[..gt];
        if atributy.ends_with('/') {
            out.push((atributy.trim_end_matches('/'), ""));
            zbytek = &za[gt + 1..];
            continue;
        }
        let telo = &za[gt + 1..];
        let Some(k) = telo.find(&konec) else { break };
        out.push((atributy, &telo[..k]));
        zbytek = &telo[k + konec.len()..];
    }
    out
}

fn atribut(atributy: &str, jmeno: &str) -> Option<String> {
    let mut zbytek = atributy;
    while let Some(i) = zbytek.find(jmeno) {
        let pred_ok = i == 0 || zbytek[..i].ends_with(char::is_whitespace);
        let za = zbytek[i + jmeno.len()..].trim_start();
        if pred_ok && za.starts_with('=') {
            let za = za[1..].trim_start();
            let uvozovka = za.chars().next()?;
            if uvozovka == '"' || uvozovka == '\'' {
                let k = za[1..].find(uvozovka)?;
                return Some(xml_text(&za[1..1 + k]));
            }
        }
        zbytek = &zbytek[i + jmeno.len()..];
    }
    None
}

fn prvni(text: &str, tag: &str) -> Option<String> {
    prvky(text, tag).first().map(|(_, o)| xml_text(o))
}

fn radek(cislo: &str, e: &str, n: &str, z: &str, kod: &str) -> String {
    let cisty = |s: &str| s.replace(['\t', '\r', '\n'], " ");
    format!("{}\t{}\t{}\t{}\t{}\n", cisty(cislo), e, n, z, cisty(kod))
}

/// LandXML: body `CgPoint` (obsah „sever východ [výška]“).
pub fn landxml(text: &str) -> Result<String, String> {
    let mut out = format!("{ZAHLAVI_EN}\n");
    let mut n = 0;
    for (a, obsah) in prvky(text, "CgPoint") {
        let c: Vec<&str> = obsah.split_whitespace().collect();
        if c.len() < 2 {
            continue;
        }
        let jmeno = atribut(a, "name").or_else(|| atribut(a, "oID")).unwrap_or_default();
        let kod = atribut(a, "code").or_else(|| atribut(a, "desc")).unwrap_or_default();
        out.push_str(&radek(&jmeno, c[1], c[0], c.get(2).copied().unwrap_or("0"), &kod));
        n += 1;
    }
    if n == 0 {
        return Err(crate::preklad::tr("v souboru LandXML nejsou body CgPoint").into());
    }
    Ok(out)
}

/// Trimble JobXML: redukované souřadnice bodů (`Reductions/Point/Grid`),
/// surová měření se nepočítají.
pub fn jobxml(text: &str) -> Result<String, String> {
    let redukce = prvky(text, "Reductions").into_iter().map(|(_, o)| o).collect::<Vec<_>>().join("\n");
    let zdroj = if redukce.is_empty() { text.to_string() } else { redukce };
    let mut out = format!("{ZAHLAVI_EN}\n");
    let mut n = 0;
    for (_, p) in prvky(&zdroj, "Point") {
        let Some(grid) = prvky(p, "Grid").first().map(|g| g.1) else { continue };
        let (Some(sever), Some(vychod)) = (prvni(grid, "North"), prvni(grid, "East")) else { continue };
        if sever.is_empty() || vychod.is_empty() {
            continue;
        }
        let z = prvni(grid, "Elevation").filter(|z| !z.is_empty()).unwrap_or_else(|| "0".into());
        out.push_str(&radek(&prvni(p, "Name").unwrap_or_default(), &vychod, &sever, &z, &prvni(p, "Code").unwrap_or_default()));
        n += 1;
    }
    if n == 0 {
        return Err(crate::preklad::tr("v souboru JobXML nejsou redukované souřadnice bodů").into());
    }
    Ok(out)
}

// ── Leica GSI ─────────────────────────────────────────────────────────────────

fn je_gsi(text: &str) -> bool {
    text.lines().filter(|l| !l.trim().is_empty()).take(5).all(|l| {
        let w = l.trim_start_matches('*');
        w.len() > 7 && w[..2].chars().all(|c| c.is_ascii_digit()) && w.split_whitespace().all(|s| s.len() >= 15)
    }) && text.lines().any(|l| !l.trim().is_empty())
}

/// Hodnota slova GSI v metrech podle jednotky (6. znak informace).
fn gsi_delka(info_jednotka: char, hodnota: &str) -> Option<f64> {
    let (znamenko, cisla) = match hodnota.split_at_checked(1)? {
        ("-", c) => (-1.0, c),
        ("+", c) => (1.0, c),
        _ => (1.0, hodnota),
    };
    let v: f64 = cisla.trim_start_matches('0').parse().unwrap_or(0.0);
    let delitel = match info_jednotka {
        '0' => 1_000.0,
        '6' => 10_000.0,
        '8' => 100_000.0,
        '1' => 1_000.0 / 0.3048,
        '7' => 10_000.0 / 0.3048,
        _ => return None,
    };
    Some(znamenko * v / delitel)
}

/// Leica GSI-8 i GSI-16: slova 11 (číslo), 71 (kód), 81/82/83 (E, N, výška).
pub fn gsi(text: &str) -> Result<String, String> {
    let mut out = format!("{ZAHLAVI_EN}\n");
    let mut n = 0;
    for l in text.lines() {
        let (mut cislo, mut kod, mut e, mut s, mut z) = (String::new(), String::new(), None, None, None);
        for slovo in l.trim_start_matches('*').split_whitespace() {
            let slovo = slovo.trim_start_matches('*');
            if slovo.len() < 8 {
                continue;
            }
            let wi = &slovo[..2];
            let jednotka = slovo.chars().nth(5).unwrap_or('0');
            let hodnota = &slovo[6..];
            match wi {
                "11" => cislo = hodnota.trim_start_matches(['+', '-']).trim_start_matches('0').to_string(),
                "71" => kod = hodnota.trim_start_matches(['+', '-']).trim_start_matches('0').to_string(),
                "81" => e = gsi_delka(jednotka, hodnota),
                "82" => s = gsi_delka(jednotka, hodnota),
                "83" => z = gsi_delka(jednotka, hodnota),
                _ => {}
            }
        }
        if let (Some(e), Some(s)) = (e, s) {
            if cislo.is_empty() {
                cislo = "0".into();
            }
            out.push_str(&radek(&cislo, &format!("{e:.4}"), &format!("{s:.4}"), &format!("{:.4}", z.unwrap_or(0.0)), &kod));
            n += 1;
        }
    }
    if n == 0 {
        return Err(crate::preklad::tr("v souboru GSI nejsou souřadnice bodů (slova 81, 82)").into());
    }
    Ok(out)
}

// ── Excel ────────────────────────────────────────────────────────────────────

/// Listy sešitu a vybraný list jako text s tabulátory (prázdné řádky se
/// vynechají, čísla s desetinnou tečkou).
pub fn excel(bajty: &[u8], list: usize) -> Result<(Vec<String>, String), String> {
    use calamine::{open_workbook_auto_from_rs, Data, Reader};
    let mut sesit = open_workbook_auto_from_rs(std::io::Cursor::new(bajty.to_vec())).map_err(|e| trf!("sešit nejde otevřít: {}", e))?;
    let listy = sesit.sheet_names().to_vec();
    let jmeno = listy.get(list).or(listy.first()).cloned().ok_or(crate::preklad::tr("sešit nemá žádný list"))?;
    let oblast = sesit.worksheet_range(&jmeno).map_err(|e| trf!("list {} nejde přečíst: {}", jmeno, e))?;
    let mut out = String::new();
    for r in oblast.rows() {
        let bunky: Vec<String> = r
            .iter()
            .map(|c| match c {
                Data::Empty => String::new(),
                Data::Float(f) => format!("{f}"),
                Data::Int(i) => i.to_string(),
                Data::String(s) => s.replace(['\t', '\r', '\n'], " ").trim().to_string(),
                Data::Bool(b) => b.to_string(),
                jine => jine.to_string(),
            })
            .collect();
        if bunky.iter().all(|b| b.is_empty()) {
            continue;
        }
        let posledni = bunky.iter().rposition(|b| !b.is_empty()).unwrap_or(0);
        out.push_str(&bunky[..=posledni].join("\t"));
        out.push('\n');
    }
    Ok((listy, out))
}

#[cfg(test)]
mod testy {
    use super::*;
    use crate::body::Oddelovac;
    use crate::rozpoznani::rozpoznej;

    #[test]
    fn landxml_zapis_a_cteni() {
        use crate::body::Bod;
        use crate::povrch::{Definice, Povrch};
        let body: Vec<Bod> = [(0.0, 0.0, 1.0), (10.0, 0.0, 2.0), (0.0, 10.0, 3.0), (10.0, 10.0, 4.0)]
            .iter()
            .enumerate()
            .map(|(i, &(x, y, z))| Bod { cislo: (i + 1).to_string(), x: x - 745_000.0, y: y - 1_043_000.0, z, popis: "T".into() })
            .collect();
        let p = Povrch::z_definice(&Definice { body: body.clone(), ..Default::default() }, 0.001).unwrap();
        let xml = crate::landxml::povrch(&p, "P", "", &body, "1");
        let t = landxml(&xml).unwrap();
        let k = &rozpoznej(&t)[0];
        assert_eq!((k.pocet, k.hlavicka, k.oddelovac), (4, 1, Oddelovac::Tabulator));
        let v = crate::body::nacti_body(&t, &k.format_bodu());
        assert_eq!(v.body[1].x, -744_990.0);
        assert_eq!(v.body[1].y, -1_043_000.0);
        assert_eq!(v.body[1].popis, "T");
    }

    #[test]
    fn landxml_poradi_sever_vychod() {
        let x = r#"<LandXML><CgPoints><CgPoint name="7" code="OB">-1043520.45 -745210.12 312.48</CgPoint><CgPoint oID="8">-1043521 -745211</CgPoint></CgPoints></LandXML>"#;
        let t = landxml(x).unwrap();
        assert_eq!(t.lines().nth(1), Some("7\t-745210.12\t-1043520.45\t312.48\tOB"));
        assert_eq!(t.lines().nth(2), Some("8\t-745211\t-1043521\t0\t"));
        assert!(landxml("<LandXML></LandXML>").is_err());
    }

    #[test]
    fn jobxml_jen_redukovane() {
        let x = r#"<JOBFile><FieldBook><PointRecord><Name>1</Name><Grid><North>9</North><East>9</East></Grid></PointRecord></FieldBook>
<Reductions><Point><Name>101</Name><Code>T</Code><Grid><North>1043520.450</North><East>745210.120</East><Elevation>312.480</Elevation></Grid></Point>
<Point><Name>102</Name><Code/><Grid><North>1043518.020</North><East>745215.800</East><Elevation></Elevation></Grid></Point></Reductions></JOBFile>"#;
        let t = jobxml(x).unwrap();
        let r: Vec<&str> = t.lines().collect();
        assert_eq!(r.len(), 3);
        assert_eq!(r[1], "101\t745210.120\t1043520.450\t312.480\tT");
        assert_eq!(r[2], "102\t745215.800\t1043518.020\t0\t");
        // kladné S-JTSK z přijímače → rozpozná se jako Y, X
        let k = &rozpoznej(&t)[0];
        assert!(k.sjtsk(), "{k:?}");
    }

    #[test]
    fn gsi8_a_gsi16() {
        let g8 = "110001+00004001 71....+0000000T 81..00+74521012 82..00+04352045 83..00+00312480\n";
        let t = gsi(g8).unwrap();
        assert_eq!(t.lines().nth(1), Some("4001\t74521.0120\t4352.0450\t312.4800\tT"));
        let g16 = "*110001+0000000000004002 81..00+0000000745215800 82..00+0000001043518020 83..00+0000000000312610\n";
        let t = gsi(g16).unwrap();
        assert_eq!(t.lines().nth(1), Some("4002\t745215.8000\t1043518.0200\t312.6100\t"));
        assert!(je_gsi(g16) && je_gsi(g8));
        assert!(!je_gsi("1;2;3;4\n"));
    }

    #[test]
    fn gsi_jednotky_mm_desetiny_setiny() {
        assert_eq!(gsi_delka('0', "+00001234"), Some(1.234));
        assert_eq!(gsi_delka('6', "-00012345"), Some(-1.2345));
        assert_eq!(gsi_delka('8', "+00123456"), Some(1.23456));
        assert_eq!(gsi_delka('2', "+00001234"), None);
    }

    #[test]
    fn nacti_podle_obsahu() {
        let z = nacti("body.txt", "1 2 3 4\n".as_bytes(), None, 0).unwrap();
        assert_eq!((z.druh, z.kodovani), (DruhZdroje::Text, Some(Kodovani::Utf8)));
        let z = nacti("x.xml", br#"<?xml?><LandXML><CgPoints><CgPoint name="1">1 2 3</CgPoint></CgPoints></LandXML>"#, None, 0).unwrap();
        assert_eq!(z.druh, DruhZdroje::LandXml);
        let z = nacti("mereni.txt", "*110001+0000000000000001 81..00+0000000000001000 82..00+0000000000002000\n".as_bytes(), None, 0).unwrap();
        assert_eq!(z.druh, DruhZdroje::Gsi);
    }
}

#[cfg(test)]
mod testy_excel {
    use super::*;
    use std::io::Write;

    /// Minimální sešit .xlsx (dva listy, vložené řetězce a čísla).
    fn sesit() -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut pis = |jmeno: &str, obsah: &str| {
            z.start_file(jmeno, o).unwrap();
            z.write_all(obsah.as_bytes()).unwrap();
        };
        pis("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#);
        pis("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#);
        pis("xl/workbook.xml", r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Poznámky" sheetId="1" r:id="rId1"/><sheet name="Body" sheetId="2" r:id="rId2"/></sheets></workbook>"#);
        pis("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#);
        pis("xl/worksheets/sheet1.xml", r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>zakázka</t></is></c></row></sheetData></worksheet>"#);
        pis("xl/worksheets/sheet2.xml", r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Číslo</t></is></c><c r="B1" t="inlineStr"><is><t>Y</t></is></c><c r="C1" t="inlineStr"><is><t>X</t></is></c><c r="D1" t="inlineStr"><is><t>Z</t></is></c><c r="E1" t="inlineStr"><is><t>Kód</t></is></c></row>
<row r="2"><c r="A2"><v>4001</v></c><c r="B2"><v>745210.12</v></c><c r="C2"><v>1043520.45</v></c><c r="D2"><v>312.48</v></c><c r="E2" t="inlineStr"><is><t>T terén</t></is></c></row>
<row r="4"><c r="A4"><v>4002</v></c><c r="B4"><v>745215.8</v></c><c r="C4"><v>1043518.02</v></c><c r="D4"><v>312.61</v></c></row>
</sheetData></worksheet>"#);
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn xlsx_list_a_oblast() {
        let b = sesit();
        let (listy, t) = excel(&b, 1).unwrap();
        assert_eq!(listy, vec!["Poznámky".to_string(), "Body".to_string()]);
        let r: Vec<&str> = t.lines().collect();
        assert_eq!(r, vec!["Číslo\tY\tX\tZ\tKód", "4001\t745210.12\t1043520.45\t312.48\tT terén", "4002\t745215.8\t1043518.02\t312.61"]);
        let z = nacti("body.xlsx", &b, None, 1).unwrap();
        assert_eq!((z.druh, z.list, z.listy.len()), (DruhZdroje::Excel, 1, 2));
    }

    #[test]
    fn xlsx_cisla_bez_carky() {
        let (_, t) = excel(&sesit(), 1).unwrap();
        let k = &crate::rozpoznani::rozpoznej(&t)[0];
        assert_eq!((k.format.as_str(), k.hlavicka, k.pocet, k.chyby), ("PYXZD", 1, 2, 0));
        assert!(k.sjtsk());
    }
}
