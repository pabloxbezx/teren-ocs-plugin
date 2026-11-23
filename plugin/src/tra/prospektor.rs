//! Trasy, podélné profily a nivelety v Prospektoru Terénu: přehled pro strom
//! a karty a akce, které Prospektor nad nimi provádí.

use ocs_plugin_api::host::{Document as CadDocument, Handle};

use crate::lang::tr_dyn;

use crate::tra::{editor, objekt, pohled};
use crate::vykres::{PresKanal, Vykres};

/// Prospektor právě mění výkres – automatický přepočet pohledů Tras počká
/// (jinak by pohled překreslil podruhé a ve výkrese by byl dvakrát).
pub static PRACUJE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Pohled podélného profilu v přehledu.
#[derive(Clone, Debug, Default)]
pub struct PrehledPohledu {
    pub id: String,
    pub insert: Handle,
    /// Jméno povrchu terénního profilu (nebo id, když povrch ve výkrese chybí).
    pub povrch: String,
    pub povrch_chybi: bool,
    /// Rozsah staničení (od, do) z posledního kreslení.
    pub rozsah: Option<(f64, f64)>,
    pub meritko: f64,
    pub prevyseni: f64,
    pub pasu: usize,
    /// Celá definice (pro editor vzhledu a pásů).
    pub def: trasy::pohled::DefinicePohledu,
    /// Trasa nebo povrch se od posledního kreslení změnily.
    pub zastaraly: bool,
    /// Kam přiblížit: střed a poloměr.
    pub stred: [f64; 2],
    pub polomer: f64,
}

/// Niveleta (návrhový profil) v přehledu.
#[derive(Clone, Debug, Default)]
pub struct PrehledNivelety {
    pub jmeno: String,
    pub vrcholu: usize,
    pub oblouku: usize,
    /// Rozsah staničení a výšek nivelety.
    pub od: f64,
    pub do_: f64,
    pub z_min: f64,
    pub z_max: f64,
    /// Sklony úseků [‰].
    pub sklony: Vec<f64>,
}

/// Trasa v přehledu.
#[derive(Clone, Debug, Default)]
pub struct PrehledTrasy {
    pub id: String,
    pub jmeno: String,
    pub insert: Handle,
    pub styl: String,
    pub styl_id: String,
    pub vyjimek: usize,
    /// Výjimky trasy proti stylu (klíč → hodnota).
    pub vyjimky: std::collections::BTreeMap<String, String>,
    pub vrcholu: usize,
    pub oblouku: usize,
    pub min_r: Option<f64>,
    pub delka: f64,
    pub pocatek: f64,
    /// Chyba geometrie (trasa se nedá spočítat).
    pub chyba: Option<String>,
    pub nivelety: Vec<PrehledNivelety>,
    pub pohledy: Vec<PrehledPohledu>,
    pub stred: [f64; 2],
    pub polomer: f64,
}

fn obalka(body: impl Iterator<Item = [f64; 2]>) -> ([f64; 2], f64) {
    let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in body {
        a = [a[0].min(p[0]), a[1].min(p[1])];
        b = [b[0].max(p[0]), b[1].max(p[1])];
    }
    if a[0] > b[0] {
        return ([0.0, 0.0], 50.0);
    }
    ([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], ((b[0] - a[0]).max(b[1] - a[1]) / 2.0).max(10.0))
}

/// Přehled všech tras výkresu s jejich niveletami a pohledy.
pub fn prehled(doc: &CadDocument) -> Vec<PrehledTrasy> {
    let povrchy = crate::povrchy::povrchy(doc);
    let pohledy = pohled::pohledy(doc);
    let mut out: Vec<PrehledTrasy> = objekt::trasy(doc)
        .into_iter()
        .map(|t| {
            let vrcholy = t.vrcholy();
            let vnitrni = if vrcholy.len() > 2 { &vrcholy[1..vrcholy.len() - 1] } else { &[][..] };
            let oblouky: Vec<f64> = vnitrni.iter().map(|v| v.r).filter(|r| *r > 0.0).collect();
            let (stred, polomer) = obalka(vrcholy.iter().map(|v| [v.x, v.y]));
            let (delka, chyba) = match t.geometrie() {
                Ok(g) => (g.delka(), None),
                Err(e) => (0.0, Some(e)),
            };
            let nivelety = t
                .def
                .nivelety
                .iter()
                .map(|n| PrehledNivelety {
                    jmeno: n.jmeno.clone(),
                    vrcholu: n.vrcholy.len(),
                    oblouku: n.vrcholy.iter().filter(|v| v.r > 0.0).count(),
                    od: n.vrcholy.first().map_or(0.0, |v| v.s),
                    do_: n.vrcholy.last().map_or(0.0, |v| v.s),
                    z_min: n.vrcholy.iter().map(|v| v.z).fold(f64::MAX, f64::min),
                    z_max: n.vrcholy.iter().map(|v| v.z).fold(f64::MIN, f64::max),
                    sklony: n.sklony().iter().map(|i| i * 1000.0).collect(),
                })
                .collect();
            let pohledy = pohledy
                .iter()
                .filter(|p| p.def.trasa == t.def.id)
                .map(|p| {
                    let z = povrchy.iter().find(|z| z.id == p.def.povrch);
                    let (stred, polomer) = match p.def.osy {
                        Some([od, do_, sr, horni, prev]) => {
                            let (w, h) = (do_ - od, (horni - sr) * prev);
                            // pásy jsou pod rámem – střed se posune trochu dolů
                            ([p.vlozeni[0] + w / 2.0, p.vlozeni[1] + h / 2.0 - h * 0.25], (w.max(h * 1.5) / 2.0).max(10.0))
                        }
                        None => (p.vlozeni, 50.0),
                    };
                    PrehledPohledu {
                        id: p.def.id.clone(),
                        insert: p.insert,
                        povrch: z.map_or_else(|| p.def.povrch.clone(), |z| z.jmeno.clone()),
                        povrch_chybi: z.is_none(),
                        rozsah: p.def.osy.map(|o| (o[0], o[1])),
                        meritko: p.def.meritko,
                        prevyseni: p.def.prevyseni,
                        pasu: p.def.pasy.len(),
                        def: p.def.clone(),
                        zastaraly: pohled::otisk(doc, &p.def) != p.def.otisk,
                        stred,
                        polomer,
                    }
                })
                .collect();
            PrehledTrasy {
                id: t.def.id.clone(),
                jmeno: t.def.jmeno.clone(),
                insert: t.insert,
                styl: objekt::jmeno_stylu(&t.def.styl),
                styl_id: t.def.styl.clone(),
                vyjimek: t.def.vyjimky.len(),
                vyjimky: t.def.vyjimky.clone(),
                vrcholu: vrcholy.len(),
                oblouku: oblouky.len(),
                min_r: oblouky.iter().copied().reduce(f64::min),
                delka,
                pocatek: t.def.pocatek,
                chyba,
                nivelety,
                pohledy,
                stred,
                polomer,
            }
        })
        .collect();
    out.sort_by(|a, b| a.jmeno.to_lowercase().cmp(&b.jmeno.to_lowercase()));
    out
}

/// Trasy a pohledy (id trasy) mezi vybranými objekty.
pub fn z_vyberu(doc: &CadDocument, vyber: &[Handle]) -> (Vec<String>, Vec<String>) {
    let trasy = objekt::z_vyberu(doc, vyber).into_iter().map(|t| t.def.id).collect();
    let pohledy = pohled::pohledy(doc).into_iter().filter(|p| vyber.contains(&p.insert)).map(|p| p.def.id).collect();
    (trasy, pohledy)
}

/// Změní jméno, počáteční staničení a styl trasy (překreslí ji i s pohledy).
pub fn nastav(v: &mut dyn Vykres, insert: Handle, jmeno: &str, pocatek: f64, styl: &str) -> Result<String, String> {
    let jmeno = jmeno.trim();
    if jmeno.is_empty() {
        return Err(tr_dyn("jméno nesmí být prázdné"));
    }
    if !pocatek.is_finite() {
        return Err(tr_dyn("neplatné počáteční staničení"));
    }
    let doc = v.dokument()?;
    let t = objekt::trasy(&doc).into_iter().find(|t| t.insert == insert).ok_or_else(|| tr_dyn("trasa už ve výkrese není"))?;
    if t.def.jmeno == jmeno && (t.def.pocatek - pocatek).abs() < 1e-9 && t.def.styl == styl {
        return Ok(String::new());
    }
    let mut def = t.def.clone();
    def.jmeno = jmeno.to_string();
    def.pocatek = pocatek;
    if objekt::styly().iter().any(|s| s.id == styl) {
        def.styl = styl.to_string();
    }
    def.vrcholy = t.vrcholy();
    v.zpet(&tr_dyn("Vlastnosti trasy"));
    let h = objekt::uloz(v, Some(&doc), Some(&t), def)?;
    v.vyber(vec![h]);
    Ok(if t.def.jmeno != jmeno {
        trasy::trf!("Trasa „{}“ přejmenována na „{}“", "Alignment “{}” renamed to “{}”", t.def.jmeno, jmeno)
    } else {
        trasy::trf!("Trasa „{}“ upravena", "Alignment “{}” updated", jmeno)
    })
}

/// Otevře panel Editor trasy s danou trasou (z pracovního vlákna Prospektoru).
pub fn otevri_editor(k: &mut PresKanal, insert: Handle) -> Result<String, String> {
    let doc = k.dokument()?;
    let t = objekt::trasy(&doc).into_iter().find(|t| t.insert == insert).ok_or_else(|| tr_dyn("trasa už ve výkrese není"))?;
    editor::nastav_aktivni(&t.def.id);
    let def = (!editor::otevren()).then(editor::definice);
    k.panel_ukaz(editor::ID, def);
    k.panel_id(editor::ID, editor::hodnoty(&doc));
    k.vyber(vec![insert]);
    Ok(trasy::trf!("Editor trasy: {}", "Alignment editor: {}", t.def.jmeno))
}

/// Smaže trasu i s jejími pohledy profilu.
pub fn smaz_trasu(v: &mut dyn Vykres, insert: Handle) -> Result<String, String> {
    let doc = v.dokument()?;
    let t = objekt::trasy(&doc).into_iter().find(|t| t.insert == insert).ok_or_else(|| tr_dyn("trasa už ve výkrese není"))?;
    let pohledy: Vec<_> = pohled::pohledy(&doc).into_iter().filter(|p| p.def.trasa == t.def.id).collect();
    v.zpet(&tr_dyn("Smazání trasy"));
    for p in &pohledy {
        smaz_pohled_obj(v, &doc, p);
    }
    objekt::smaz(v, &doc, &t);
    Ok(if pohledy.is_empty() {
        trasy::trf!("Trasa „{}“ smazána", "Alignment “{}” deleted", t.def.jmeno)
    } else {
        trasy::trf!("Trasa „{}“ smazána i s pohledy profilu ({})", "Alignment “{}” deleted together with its profile views ({})", t.def.jmeno, pohledy.len())
    })
}

fn smaz_pohled_obj(v: &mut dyn Vykres, doc: &CadDocument, p: &pohled::ObjektPohledu) {
    v.odeber(p.insert);
    if doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&p.blok)) {
        let _ = v.tabulka(ocs_plugin_api::host::TableOperation::BlockDelete { name: p.blok.clone() });
    }
}

/// Smaže pohled podélného profilu.
pub fn smaz_pohled(v: &mut dyn Vykres, insert: Handle) -> Result<String, String> {
    let doc = v.dokument()?;
    let p = pohled::pohledy(&doc).into_iter().find(|p| p.insert == insert).ok_or_else(|| tr_dyn("pohled už ve výkrese není"))?;
    v.zpet(&tr_dyn("Smazání pohledu profilu"));
    smaz_pohled_obj(v, &doc, &p);
    v.hotovo();
    Ok(trasy::trf!("Pohled profilu {} smazán", "Profile view {} deleted", p.def.id))
}

/// Překreslí pohledy (jen zadané, nebo všechny zastaralé).
pub fn prekresli(v: &mut dyn Vykres, jen: Option<Handle>) -> Result<String, String> {
    let doc = v.dokument()?;
    v.zpet(&tr_dyn("Překreslení pohledů profilu"));
    let mut n = 0;
    for p in pohled::pohledy(&doc) {
        let zastaraly = pohled::otisk(&doc, &p.def) != p.def.otisk;
        if jen.map_or(zastaraly, |h| h == p.insert) {
            pohled::uloz(v, &doc, Some(&p), &p.def, p.vlozeni)?;
            n += 1;
        }
    }
    v.hotovo();
    Ok(trasy::trf!("Překresleno pohledů profilu: {}", "Profile views redrawn: {}", n))
}

/// Uloží upravený vzhled a pásy pohledu a pohled překreslí (jeden krok Zpět).
pub fn uprav_pohled(v: &mut dyn Vykres, insert: Handle, def: &trasy::pohled::DefinicePohledu) -> Result<String, String> {
    let doc = v.dokument()?;
    let p = pohled::pohledy(&doc).into_iter().find(|p| p.insert == insert).ok_or_else(|| tr_dyn("pohled už ve výkrese není"))?;
    let mut def = def.clone();
    def.id = p.def.id.clone();
    def.trasa = p.def.trasa.clone();
    if !def.meritko.is_finite() || def.meritko < 10.0 {
        def.meritko = 1000.0;
    }
    if !def.prevyseni.is_finite() || def.prevyseni <= 0.0 {
        def.prevyseni = 10.0;
    }
    v.zpet(&tr_dyn("Úprava pohledu profilu"));
    // uloz si překreslení i „výkres změněn“ zařídí sám (na místě i postaru)
    pohled::uloz(v, &doc, Some(&p), &def, p.vlozeni)?;
    Ok(trasy::trf!("Pohled profilu {} upraven", "Profile view {} updated", def.id))
}

/// Překreslí trasy se stylem `styl` (např. po jeho uložení). S `nahrada`
/// jim zároveň styl změní (smazaný vlastní styl → základní).
pub fn prekresli_trasy_stylu(v: &mut dyn Vykres, styl: &str, nahrada: Option<&str>) -> Result<String, String> {
    let doc = v.dokument()?;
    let trasy: Vec<_> = objekt::trasy(&doc).into_iter().filter(|t| t.def.styl == styl).collect();
    for t in &trasy {
        let mut def = t.def.clone();
        if let Some(s) = nahrada {
            def.styl = s.to_string();
        }
        def.vrcholy = t.vrcholy();
        v.zpet(&tr_dyn("Styl trasy"));
        objekt::uloz(v, Some(&doc), Some(t), def)?;
    }
    Ok(if trasy.is_empty() { String::new() } else { trasy::trf!("Překresleno tras: {}", "Alignments redrawn: {}", trasy.len()) })
}
