//! Interaktivní příkazy – klikání do výkresu.
//!
//! Open CAD Studio během interaktivního příkazu nedovolí pluginu sahat do
//! výkresu. Příkazy proto jen sbírají vstup, ukazují náhled a výsledek
//! předají hostiteli k vložení (Commit) při ukončení. Zprávy pro uživatele
//! se zobrazují ve výzvě příkazu.

use teren::preklad::tr;
use teren::trf;
use ocs_plugin_api::host::codec::entities::{Line, Point, Polyline3D, Text};
use ocs_plugin_api::host::codec::types::Vector3;
use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogResponse, DialogRow, DialogValue};
use ocs_plugin_api::host::{CommandStep, EntityType, Handle, InteractiveCommand, PreviewWire};

use teren::body::Bod;
use teren::povrch::{Definice, Editace, Povrch};
use teren::skupiny::{SeznamCisel, SkupinaBodu, RUCNE};

use crate::cogo;
use crate::povrchy::{self, ZaznamPovrchu};
use crate::spravce::{self, Akce};

const ZLUTA: [f32; 4] = [1.0, 0.85, 0.2, 1.0];
const CERVENA: [f32; 4] = [1.0, 0.35, 0.3, 1.0];
const ZELENA: [f32; 4] = [0.3, 0.9, 0.4, 1.0];
const SEDA: [f32; 4] = [0.55, 0.75, 0.7, 0.9];

/// Velikost značek v náhledu podle rozměru povrchu.
fn velikost_znacky(p: &Povrch) -> f64 {
    let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for v in &p.vrcholy {
        minx = minx.min(v.x);
        maxx = maxx.max(v.x);
        miny = miny.min(v.y);
        maxy = maxy.max(v.y);
    }
    ((maxx - minx).max(maxy - miny) / 150.0).clamp(0.05, 5.0)
}

fn krizek(x: f64, y: f64, z: f64, r: f64, c: [f32; 4]) -> Vec<PreviewWire> {
    vec![
        PreviewWire::line([x - r, y, z], [x + r, y, z], Some(c)),
        PreviewWire::line([x, y - r, z], [x, y + r, z], Some(c)),
    ]
}

fn cislo(b: &Bod) -> &str {
    if b.cislo.is_empty() { "–" } else { &b.cislo }
}

// ── Spojnice přes body ──────────────────────────────────────────────────────

pub struct SpojnicePresBody {
    pub hladina: String,
    pub jmeno_povrchu: String,
    pub body: Vec<Bod>,
    pub vrcholy: Vec<usize>,
    pub tolerance: f64,
    pub zprava: String,
}

impl SpojnicePresBody {
    fn nejblizsi(&self, x: f64, y: f64) -> Option<usize> {
        self.body
            .iter()
            .enumerate()
            .map(|(i, b)| (i, (b.x - x).hypot(b.y - y)))
            .filter(|(_, d)| *d <= self.tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

impl InteractiveCommand for SpojnicePresBody {
    fn prompt(&self) -> String {
        let pokyn = if self.vrcholy.is_empty() {
            trf!("Spojnice povrchu „{}“ – klikněte na první bod", self.jmeno_povrchu)
        } else {
            tr("Další bod (znovu poslední bod = zpět, Enter = konec)").into()
        };
        if self.zprava.is_empty() { trf!("{}:", pokyn) } else { trf!("{} | {}:", self.zprava, pokyn) }
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(i) = self.nejblizsi(pt[0], pt[1]) else {
            self.zprava = trf!("V okolí {} m není bod zaměření", povrchy::cislo_cz(self.tolerance));
            return CommandStep::NeedPoint;
        };
        if self.vrcholy.last() == Some(&i) {
            self.vrcholy.pop();
            self.zprava = trf!("Zpět – bod {} odebrán", cislo(&self.body[i]));
            return CommandStep::NeedPoint;
        }
        let b = &self.body[i];
        self.zprava = trf!("Bod {} výška {:.3}", cislo(b), b.z);
        self.vrcholy.push(i);
        CommandStep::NeedPoint
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let mut w = Vec::new();
        let mut cara: Vec<[f64; 3]> = self.vrcholy.iter().map(|&i| [self.body[i].x, self.body[i].y, self.body[i].z]).collect();
        for p in &cara {
            w.push(PreviewWire::circle(*p, self.tolerance * 0.25, Some(CERVENA)));
        }
        match self.nejblizsi(pt[0], pt[1]).map(|i| &self.body[i]) {
            Some(b) => {
                cara.push([b.x, b.y, b.z]);
                w.push(PreviewWire::circle([b.x, b.y, b.z], self.tolerance * 0.5, Some(ZELENA)));
            }
            None => cara.push(pt),
        }
        if cara.len() >= 2 {
            w.push(PreviewWire::with_color(cara, CERVENA));
        }
        w
    }

    fn on_enter(&mut self) -> CommandStep {
        if self.vrcholy.len() < 2 {
            return CommandStep::Done;
        }
        let body: Vec<Vector3> = self.vrcholy.iter().map(|&i| Vector3::new(self.body[i].x, self.body[i].y, self.body[i].z)).collect();
        let mut pl = Polyline3D::from_points(body);
        pl.common.layer = self.hladina.clone();
        CommandStep::CommitAndEnd(EntityType::Polyline3D(pl))
    }
}

// ── Editace povrchu ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum DruhEditace {
    OtocitHranu,
    SmazatTrojuhelnik,
    PridatBod,
    OdebratBod,
    PridatHranu,
    SmazatHranu,
}

pub struct EditacePovrchu {
    pub zaznam: ZaznamPovrchu,
    pub definice: Definice,
    pub povrch: Povrch,
    pub druh: DruhEditace,
    pub nove: Vec<Editace>,
    pub zprava: String,
    /// První vrchol u přidání hrany.
    pub prvni: Option<usize>,
    /// Přidání bodu: poloha čeká na výšku z okna.
    pub ceka_vysku: Option<[f64; 2]>,
}

impl EditacePovrchu {
    pub fn new(zaznam: ZaznamPovrchu, definice: Definice, povrch: Povrch, druh: DruhEditace) -> Self {
        Self { zaznam, definice, povrch, druh, nove: Vec::new(), zprava: String::new(), prvni: None, ceka_vysku: None }
    }

    /// Použije úpravu: přepočte náhled a vrátí značku k uložení do výkresu.
    /// Povrch ve výkrese se přepočítá hned (Prospektor převezme značku).
    fn pouzij(&mut self, e: Editace) -> Option<EntityType> {
        let mut def = self.definice.clone();
        def.editace.extend(self.nove.iter().cloned());
        def.editace.push(e.clone());
        match Povrch::z_definice(&def, 0.001) {
            Ok(p) => {
                self.povrch = p;
                self.zprava = trf!("✔ {} (celkem {})", e.popis(), self.nove.len() + 1);
                let z = self.znacka(&e);
                self.nove.push(e);
                spravce::posli(Akce::Odlozena(
                    Box::new(Akce::Prepocitat(vec![self.zaznam.id.clone()])),
                    spravce::Podminka::ZnackyPovrchu(self.zaznam.id.clone()),
                ));
                Some(z)
            }
            Err(chyba) => {
                self.zprava = trf!("Nelze: {}", chyba);
                None
            }
        }
    }

    /// Objekt, který úpravu uloží do výkresu (hladina úprav povrchu).
    fn znacka(&self, e: &Editace) -> EntityType {
        let hladina = self.zaznam.hladiny().upravy;
        {
            {
                let mut ent = match e {
                    Editace::OtocitHranu(a, b) | Editace::PridatHranu(a, b) | Editace::SmazatHranu(a, b) => {
                        let za = self.povrch.vyska(a[0], a[1]).unwrap_or(0.0);
                        let zb = self.povrch.vyska(b[0], b[1]).unwrap_or(0.0);
                        EntityType::Line(Line::from_points(Vector3::new(a[0], a[1], za), Vector3::new(b[0], b[1], zb)))
                    }
                    Editace::PridatBod(p) => EntityType::Point(Point::from_coords(p[0], p[1], p[2])),
                    Editace::OdebratBod(p) | Editace::SmazatTrojuhelnik(p) => {
                        EntityType::Point(Point::from_coords(p[0], p[1], 0.0))
                    }
                };
                ent.common_mut().layer = hladina.clone();
                ent.common_mut().extended_data.add_record(povrchy::xdata_upravy(&self.zaznam.id, e));
                ent
            }
        }
    }
}

impl InteractiveCommand for EditacePovrchu {
    fn prompt(&self) -> String {
        let pokyn = match self.druh {
            DruhEditace::OtocitHranu => tr("Otočit hranu – klikněte na hranu"),
            DruhEditace::SmazatTrojuhelnik => tr("Smazat trojúhelník – klikněte do trojúhelníku"),
            DruhEditace::PridatBod => tr("Přidat bod – klikněte nebo zadejte x,y,z"),
            DruhEditace::OdebratBod => tr("Odebrat bod – klikněte na vrchol"),
            DruhEditace::PridatHranu if self.prvni.is_some() => tr("Přidat hranu – klikněte na druhý vrchol"),
            DruhEditace::PridatHranu => tr("Přidat hranu – klikněte na první vrchol"),
            DruhEditace::SmazatHranu => tr("Smazat hranu – klikněte na hranu"),
        };
        if self.zprava.is_empty() {
            trf!("{} (každý klik se hned projeví, Enter = konec):", pokyn)
        } else {
            trf!("{} · {}, Enter = konec:", self.zprava, pokyn)
        }
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let mut ulozit: Option<EntityType> = None;
        let (x, y) = (pt[0], pt[1]);
        let r = velikost_znacky(&self.povrch) * 3.0;
        match self.druh {
            DruhEditace::OtocitHranu => match self.povrch.nejblizsi_hrana(x, y) {
                Some(((a, b), d)) if d <= r * 2.0 => {
                    let mut zkouska = self.povrch.clone();
                    match zkouska.otoc_hranu(a, b) {
                        Ok(()) => {
                            let (ga, gb) = (self.povrch.globalni(a), self.povrch.globalni(b));
                            ulozit = self.pouzij(Editace::OtocitHranu([ga.0, ga.1], [gb.0, gb.1]));
                        }
                        Err(duvod) => self.zprava = trf!("Hranu nejde otočit – {}", duvod),
                    }
                }
                _ => self.zprava = trf!("Tady není hrana povrchu „{}“ – klikněte blíž k čáře trojúhelníku (hrany ukáže styl Trojúhelníky)", self.zaznam.jmeno),
            },
            DruhEditace::SmazatTrojuhelnik => {
                if self.povrch.trojuhelnik_v_bode(x, y).is_some() {
                    ulozit = self.pouzij(Editace::SmazatTrojuhelnik([x, y]));
                } else {
                    self.zprava = trf!("Tady není trojúhelník povrchu „{}“ – klikněte dovnitř trojúhelníku", self.zaznam.jmeno);
                }
            }
            DruhEditace::PridatBod => {
                // výška v okně – návrh z povrchu (nebo ze zadaného x,y,z)
                let navrh = if pt[2].abs() > 1e-9 { Some(pt[2]) } else { self.povrch.vyska(x, y) };
                self.ceka_vysku = Some([x, y]);
                let info = match navrh {
                    Some(_) => trf!("Y {:.2}   X {:.2}   (návrh výšky {})", -x, -y, tr("z povrchu")),
                    None => trf!("Y {:.2}   X {:.2}   – bod je mimo povrch", -x, -y),
                };
                let radek = |c: DialogControl| DialogRow { controls: vec![c], align_h: None, align_v: None };
                spravce::ukaz_dialog(DialogDef {
                    title: tr("Přidat bod").into(),
                    rows: vec![
                        radek(DialogControl::InfoText { text: info, align_h: None }),
                        radek(DialogControl::Number {
                            id: "z".into(),
                            label: tr("Výška Z [m]").into(),
                            default: (navrh.unwrap_or(0.0) * 1000.0).round() / 1000.0,
                            min: None,
                            max: None,
                            step: Some(0.01),
                        }),
                    ],
                    submit_label: tr("Přidat bod").into(),
                    cancel_label: tr("Zrušit").into(),
                    width: Some(340.0),
                });
            }
            DruhEditace::OdebratBod => match self.povrch.nejblizsi_vrchol(x, y, r * 2.0) {
                Some(i) => {
                    let (gx, gy, _) = self.povrch.globalni(i);
                    ulozit = self.pouzij(Editace::OdebratBod([gx, gy]));
                }
                None => self.zprava = trf!("Tady není bod povrchu „{}“ – klikněte blíž k vrcholu trojúhelníku", self.zaznam.jmeno),
            },
            DruhEditace::PridatHranu => match self.povrch.nejblizsi_vrchol(x, y, r * 2.0) {
                Some(i) => match self.prvni.take() {
                    None => {
                        self.prvni = Some(i);
                        self.zprava = tr("První vrchol vybrán – klikněte na druhý").into();
                    }
                    Some(a) if a == i => self.zprava = tr("To je tentýž vrchol – klikněte na jiný").into(),
                    Some(a) => {
                        let (ga, gb) = (self.povrch.globalni(a), self.povrch.globalni(i));
                        ulozit = self.pouzij(Editace::PridatHranu([ga.0, ga.1], [gb.0, gb.1]));
                    }
                },
                None => self.zprava = trf!("Tady není bod povrchu „{}“ – klikněte blíž k vrcholu trojúhelníku", self.zaznam.jmeno),
            },
            DruhEditace::SmazatHranu => match self.povrch.nejblizsi_hrana(x, y) {
                Some(((a, b), d)) if d <= r * 2.0 => {
                    let (ga, gb) = (self.povrch.globalni(a), self.povrch.globalni(b));
                    ulozit = self.pouzij(Editace::SmazatHranu([ga.0, ga.1], [gb.0, gb.1]));
                }
                _ => self.zprava = trf!("Tady není hrana povrchu „{}“ – klikněte blíž k čáře trojúhelníku (hrany ukáže styl Trojúhelníky)", self.zaznam.jmeno),
            },
        }
        match ulozit {
            Some(e) => CommandStep::Commit(e),
            None => CommandStep::NeedPoint,
        }
    }

    fn on_dialog_response(&mut self, odpoved: DialogResponse) -> CommandStep {
        let Some([x, y]) = self.ceka_vysku.take() else { return CommandStep::NeedPoint };
        if let DialogResponse::Submitted(h) = odpoved {
            if let Some(DialogValue::Number(z)) = h.get("z") {
                if z.is_finite() {
                    if let Some(e) = self.pouzij(Editace::PridatBod([x, y, *z])) {
                        return CommandStep::Commit(e);
                    }
                }
            }
        } else {
            self.zprava = tr("Bod nevložen").into();
        }
        CommandStep::NeedPoint
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let (x, y) = (pt[0], pt[1]);
        let r = velikost_znacky(&self.povrch);
        let p = &self.povrch;
        let g = |i: usize| {
            let (a, b, c) = p.globalni(i);
            [a, b, c]
        };
        // trojúhelníky v okolí kurzoru – živý náhled upraveného povrchu
        let dosah = r * 25.0;
        let mut w: Vec<PreviewWire> = Vec::new();
        for t in &p.trojuhelniky {
            let v = t.map(g);
            if v.iter().any(|q| (q[0] - x).hypot(q[1] - y) <= dosah) {
                w.push(PreviewWire::with_color(vec![v[0], v[1], v[2], v[0]], SEDA));
                if w.len() > 1500 {
                    break;
                }
            }
        }
        match self.druh {
            DruhEditace::OtocitHranu => {
                if let Some(((a, b), d)) = p.nejblizsi_hrana(x, y) {
                    if d <= r * 6.0 {
                        w.push(PreviewWire::line(g(a), g(b), Some(ZLUTA)));
                    }
                }
            }
            DruhEditace::SmazatTrojuhelnik => {
                if let Some(t) = p.trojuhelnik_v_bode(x, y) {
                    let [a, b, c] = p.trojuhelniky[t].map(g);
                    w.push(PreviewWire::with_color(vec![a, b, c, a], CERVENA));
                }
            }
            DruhEditace::PridatBod => {
                if let Some(z) = p.vyska(x, y) {
                    w.extend(krizek(x, y, z, r, ZELENA));
                }
            }
            DruhEditace::OdebratBod => {
                if let Some(i) = p.nejblizsi_vrchol(x, y, r * 6.0) {
                    w.push(PreviewWire::circle(g(i), r, Some(CERVENA)));
                }
            }
            DruhEditace::PridatHranu => {
                let cil = p.nejblizsi_vrchol(x, y, r * 6.0);
                if let Some(a) = self.prvni {
                    w.push(PreviewWire::circle(g(a), r, Some(ZELENA)));
                    let konec = cil.map(g).unwrap_or(pt);
                    w.push(PreviewWire::line(g(a), konec, Some(ZLUTA)));
                }
                if let Some(i) = cil {
                    w.push(PreviewWire::circle(g(i), r, Some(ZLUTA)));
                }
            }
            DruhEditace::SmazatHranu => {
                if let Some(((a, b), d)) = p.nejblizsi_hrana(x, y) {
                    if d <= r * 6.0 {
                        for t in p.trojuhelniky.iter().filter(|t| t.contains(&a) && t.contains(&b)) {
                            let [q1, q2, q3] = t.map(g);
                            w.push(PreviewWire::with_color(vec![q1, q2, q3, q1], CERVENA));
                        }
                    }
                }
            }
        }
        w
    }

    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Done
    }
}

// ── Výška v bodě ─────────────────────────────────────────────────────────────

pub struct VyskaVBode {
    pub povrch: Povrch,
    pub hladina: String,
    pub vyska_textu: f64,
    pub zprava: String,
}

impl InteractiveCommand for VyskaVBode {
    fn prompt(&self) -> String {
        if self.zprava.is_empty() {
            tr("Výška v bodě – klikněte do povrchu (Enter = konec):").into()
        } else {
            trf!("{} | další bod (Enter = konec):", self.zprava)
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.povrch.vyska(pt[0], pt[1]) {
            Some(z) => {
                self.zprava = trf!("Výška {:.3}", z);
                let mut t = Text::with_value(format!("{z:.2}"), Vector3::new(pt[0], pt[1], z)).with_height(self.vyska_textu);
                t.common.layer = self.hladina.clone();
                CommandStep::Commit(EntityType::Text(t))
            }
            None => {
                self.zprava = tr("Mimo povrch").into();
                CommandStep::NeedPoint
            }
        }
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        match self.povrch.vyska(pt[0], pt[1]) {
            Some(z) => krizek(pt[0], pt[1], z, velikost_znacky(&self.povrch), ZELENA),
            None => Vec::new(),
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Done
    }
}

// ── Výběr aktivního povrchu ─────────────────────────────────────────────────

pub struct VybratPovrch {
    /// (handle objektu, id povrchu, jméno povrchu)
    pub objekty: Vec<(Handle, String, String)>,
    pub vybrano: Option<String>,
    pub zprava: String,
}

impl InteractiveCommand for VybratPovrch {
    fn prompt(&self) -> String {
        match &self.vybrano {
            Some(j) => trf!("Aktivní povrch je „{}“ (Enter = konec):", j),
            None if !self.zprava.is_empty() => trf!("{} | klikněte na povrch:", self.zprava),
            None => tr("Vyberte povrch – klikněte na jeho čáru nebo trojúhelník:").into(),
        }
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        if self.vybrano.is_some() { CommandStep::Done } else { CommandStep::NeedPoint }
    }
    fn needs_object_pick(&self) -> bool {
        self.vybrano.is_none()
    }
    fn on_object_pick(&mut self, handle: Handle, _pt: [f64; 3]) -> CommandStep {
        match self.objekty.iter().find(|(h, _, _)| *h == handle) {
            Some((_, id, jmeno)) => {
                povrchy::nastav_aktivni(id);
                self.vybrano = Some(jmeno.clone());
            }
            None => self.zprava = tr("Objekt nepatří k žádnému povrchu").into(),
        }
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Done
    }
}

// ── Nové body ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum DruhBodu {
    /// Výška zadaná (x,y,z), předvolená, nebo z aktivního povrchu.
    Rucne,
    /// Výška z aktivního povrchu.
    NaPovrchu,
    /// Body pro nový povrch – po Enter vznikne skupina a z ní povrch.
    ProPovrch,
}

pub struct NovyBod {
    pub druh: DruhBodu,
    pub dalsi: u64,
    pub kod: String,
    /// Předvolená výška nových bodů.
    pub vyska: Option<f64>,
    pub povrch: Option<(String, Povrch)>,
    /// Styly bodů a měřítko pro grafiku nových bodů.
    pub styly: crate::styly::Rozlisovac,
    pub meritko: f64,
    /// Body vytvořené tímto příkazem.
    pub body: Vec<Bod>,
    /// U povrchu z naklikaných bodů: skupina a jméno nového povrchu.
    pub skupina: Option<SkupinaBodu>,
    pub jmeno_povrchu: String,
    pub zprava: String,
    /// Ruční bod: poloha je zadaná, čeká se na výšku.
    pub ceka_vysku: Option<[f64; 3]>,
}

impl NovyBod {
    /// Výška nabídnutá u ručního bodu (Enter): z povrchu, jinak jako předchozí bod.
    fn navrh_vysky(&self, x: f64, y: f64, z: f64) -> Option<(f64, &'static str)> {
        self.povrch
            .as_ref()
            .and_then(|(_, p)| p.vyska(x, y))
            .map(|z| (z, tr("z povrchu")))
            .or((z.abs() > 1e-9).then_some((z, tr("z místa kliknutí"))))
            .or(self.body.last().map(|b| (b.z, tr("jako předchozí bod"))))
    }

    /// Dialog s výškou (a kódem) ručního bodu – otevře se po kliknutí.
    fn dialog_vysky(&self, [x, y, z]: [f64; 3]) -> DialogDef {
        let (navrh, odkud) = self.navrh_vysky(x, y, z).unwrap_or((0.0, ""));
        let info = if odkud.is_empty() { format!("Y {:.2}   X {:.2}", -x, -y) } else { trf!("Y {:.2}   X {:.2}   (návrh výšky {})", -x, -y, odkud) };
        let radek = |c: DialogControl| DialogRow { controls: vec![c], align_h: None, align_v: None };
        DialogDef {
            title: trf!("Bod {}", self.dalsi),
            rows: vec![
                radek(DialogControl::InfoText { text: info, align_h: None }),
                radek(DialogControl::Number { id: "z".into(), label: tr("Výška Z [m]").into(), default: (navrh * 1000.0).round() / 1000.0, min: None, max: None, step: Some(0.01) }),
                radek(DialogControl::Text { id: "kod".into(), label: tr("Kód / popis").into(), default: self.kod.clone() }),
            ],
            submit_label: tr("Vložit bod").into(),
            cancel_label: tr("Zrušit").into(),
            width: Some(340.0),
        }
    }

    fn vytvor(&mut self, x: f64, y: f64, z: f64, puvod: &str) -> CommandStep {
        let b = Bod { cislo: self.dalsi.to_string(), x, y, z, popis: self.kod.clone() };
        self.dalsi += 1;
        self.zprava = trf!("Bod {} výška {:.3} ({})", b.cislo, z, puvod);
        self.body.push(b.clone());
        match self.druh {
            DruhBodu::ProPovrch => CommandStep::NeedPoint,
            _ => CommandStep::CommitMany(self.objekty(&b)),
        }
    }

    fn vyska_v(&self, pt: [f64; 3]) -> Result<(f64, &'static str), String> {
        let zadana = (pt[2].abs() > 1e-9).then_some(pt[2]);
        let z_povrchu = self.povrch.as_ref().and_then(|(_, p)| p.vyska(pt[0], pt[1]));
        match self.druh {
            DruhBodu::NaPovrchu => z_povrchu.map(|z| (z, tr("z povrchu"))).ok_or_else(|| tr("Mimo povrch").into()),
            DruhBodu::Rucne => zadana
                .map(|z| (z, tr("zadaná")))
                .or(self.vyska.map(|z| (z, tr("předvolená"))))
                .or(z_povrchu.map(|z| (z, tr("z povrchu"))))
                .ok_or_else(|| tr("Neznám výšku – zadejte x,y,z nebo předvolte výšku v Prospektoru (Body)").into()),
            DruhBodu::ProPovrch => zadana
                .map(|z| (z, tr("zadaná")))
                .or(self.vyska.map(|z| (z, tr("předvolená"))))
                .or(self.body.last().map(|b| (b.z, tr("jako předchozí bod"))))
                .ok_or_else(|| tr("Neznám výšku – zadejte x,y,z nebo předvolte výšku v Prospektoru (Body)").into()),
        }
    }

    fn velikost(&self) -> f64 {
        (self.meritko * 0.0012).max(0.05)
    }

    fn objekty(&self, b: &Bod) -> Vec<EntityType> {
        cogo::objekty_bodu(b, RUCNE, &self.styly.styl(b, RUCNE, ""), self.meritko)
    }

    fn povrch_z_bodu(&self, navic: Option<[f64; 3]>) -> Option<Povrch> {
        let mut body = self.body.clone();
        if let Some(p) = navic {
            body.push(Bod { cislo: String::new(), x: p[0], y: p[1], z: p[2], popis: String::new() });
        }
        if body.len() < 3 {
            return None;
        }
        Povrch::z_definice(&Definice { body, ..Default::default() }, 0.001).ok()
    }
}

impl InteractiveCommand for NovyBod {
    fn prompt(&self) -> String {
        if let Some([x, y, z]) = self.ceka_vysku {
            let navrh = match self.navrh_vysky(x, y, z) {
                Some((z, odkud)) => trf!("Enter = {:.2} {}", z, odkud),
                None => tr("Esc = zrušit").into(),
            };
            let z = if self.zprava.is_empty() { String::new() } else { format!("{} | ", self.zprava) };
            return trf!("{}Bod {} – napište výšku a Enter ({}):", z, self.dalsi, navrh);
        }
        let pokyn = match self.druh {
            DruhBodu::Rucne => trf!("Bod {} – klikněte nebo zadejte x,y,z", self.dalsi),
            DruhBodu::NaPovrchu => trf!("Bod {} na povrchu „{}“ – klikněte", self.dalsi, self.povrch.as_ref().map_or("", |(j, _)| j.as_str())),
            DruhBodu::ProPovrch if self.body.len() < 3 => trf!("Povrch „{}“: bod {} – klikněte nebo zadejte x,y,z (znovu poslední = zpět, nejméně 3 body)", self.jmeno_povrchu, self.dalsi),
            DruhBodu::ProPovrch => trf!("Povrch „{}“: bod {} (znovu poslední = zpět, Enter = vytvořit povrch z {} bodů)", self.jmeno_povrchu, self.dalsi, self.body.len()),
        };
        let konec = if self.druh == DruhBodu::ProPovrch { "" } else { tr(" (Enter = konec)") };
        if self.zprava.is_empty() { trf!("{}{}:", pokyn, konec) } else { trf!("{} | {}{}:", self.zprava, pokyn, konec) }
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        // Výška ručního bodu: číslo napsané do příkazového řádku Open CAD Studio
        // předá jako bod v této vzdálenosti od posledního bodu (zadání vzdálenosti),
        // takže výška = vzdálenost od místa bodu.
        if let Some([x, y, _]) = self.ceka_vysku.take() {
            let z = (pt[0] - x).hypot(pt[1] - y);
            return self.vytvor(x, y, z, tr("zadaná"));
        }
        if self.druh == DruhBodu::Rucne && self.vyska.is_none() {
            self.ceka_vysku = Some(pt);
            self.zprava.clear();
            spravce::ukaz_dialog(self.dialog_vysky(pt));
            return CommandStep::NeedPoint;
        }
        if self.druh == DruhBodu::ProPovrch {
            if let Some(b) = self.body.last() {
                if (b.x - pt[0]).hypot(b.y - pt[1]) <= self.velikost() {
                    let b = self.body.pop().unwrap();
                    self.dalsi -= 1;
                    self.zprava = trf!("Zpět – bod {} odebrán", b.cislo);
                    return CommandStep::NeedPoint;
                }
            }
        }
        let (z, puvod) = match self.vyska_v(pt) {
            Ok(v) => v,
            Err(e) => {
                self.zprava = e;
                return CommandStep::NeedPoint;
            }
        };
        self.vytvor(pt[0], pt[1], z, puvod)
    }

    fn on_dialog_response(&mut self, odpoved: DialogResponse) -> CommandStep {
        let Some([x, y, _]) = self.ceka_vysku.take() else { return CommandStep::NeedPoint };
        match odpoved {
            DialogResponse::Submitted(h) => {
                if let Some(DialogValue::Text(k)) = h.get("kod") {
                    self.kod = k.trim().to_string();
                }
                match h.get("z") {
                    Some(DialogValue::Number(z)) if z.is_finite() => self.vytvor(x, y, *z, tr("zadaná")),
                    _ => {
                        self.zprava = tr("Výšku napište číslem").into();
                        CommandStep::NeedPoint
                    }
                }
            }
            DialogResponse::Cancelled => {
                self.zprava = tr("Bod nevložen").into();
                CommandStep::NeedPoint
            }
        }
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let r = self.velikost();
        if let Some([x, y, z]) = self.ceka_vysku {
            let z = self.navrh_vysky(x, y, z).map_or(0.0, |v| v.0);
            return krizek(x, y, z, r, ZLUTA);
        }
        let mut w = Vec::new();
        for b in &self.body {
            w.push(PreviewWire::circle([b.x, b.y, b.z], r * 0.6, Some(ZLUTA)));
        }
        let z = self.vyska_v(pt).ok().map(|v| v.0);
        if self.druh == DruhBodu::ProPovrch {
            let kurzor = z.map(|z| [pt[0], pt[1], z]);
            if let Some(p) = self.povrch_z_bodu(kurzor) {
                for t in &p.trojuhelniky {
                    let v = t.map(|i| {
                        let (a, b, c) = p.globalni(i);
                        [a, b, c]
                    });
                    w.push(PreviewWire::with_color(vec![v[0], v[1], v[2], v[0]], SEDA));
                }
            } else {
                let mut cara: Vec<[f64; 3]> = self.body.iter().map(|b| [b.x, b.y, b.z]).collect();
                cara.push(pt);
                if cara.len() >= 2 {
                    w.push(PreviewWire::with_color(cara, SEDA));
                }
            }
        }
        match z {
            Some(z) => w.extend(krizek(pt[0], pt[1], z, r, ZELENA)),
            None => w.extend(krizek(pt[0], pt[1], 0.0, r, CERVENA)),
        }
        w
    }

    fn on_enter(&mut self) -> CommandStep {
        if let Some([x, y, z]) = self.ceka_vysku {
            return match self.navrh_vysky(x, y, z) {
                Some((z, odkud)) => {
                    self.ceka_vysku = None;
                    self.vytvor(x, y, z, odkud)
                }
                None => {
                    self.zprava = tr("Výšku napište číslem").into();
                    CommandStep::NeedPoint
                }
            };
        }
        if self.druh != DruhBodu::ProPovrch {
            return CommandStep::Done;
        }
        let Some(mut s) = self.skupina.take() else { return CommandStep::Done };
        if self.body.len() < 3 {
            return CommandStep::Done;
        }
        let mut cisla = SeznamCisel::default();
        cisla.pridej(&self.body.iter().map(|b| b.cislo.clone()).collect::<Vec<_>>());
        s.cisla = cisla.text();
        let n = self.body.len() as f64;
        let stred = [self.body.iter().map(|b| b.x).sum::<f64>() / n, self.body.iter().map(|b| b.y).sum::<f64>() / n, 0.0];
        let mut ent: Vec<EntityType> = self.body.iter().flat_map(|b| self.objekty(b)).collect();
        ent.push(cogo::nosic_skupiny(&s, stred));
        // povrch vytvoří Prospektor, až příkaz skončí a skupina bude ve výkrese
        spravce::posli(Akce::Odlozena(
            Box::new(Akce::PovrchZeSkupiny(s.id.clone(), Some(self.jmeno_povrchu.clone()), true)),
            spravce::Podminka::Skupina(s.id.clone()),
        ));
        CommandStep::CommitManyAndEnd(ent)
    }
}
