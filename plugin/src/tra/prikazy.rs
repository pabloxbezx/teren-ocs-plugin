//! Interaktivní příkazy: výběr objektu kliknutím, kreslení trasy vrcholy.
//!
//! Interaktivní příkaz nemá přístup k výkresu – sbírá kliknutí a na konci
//! předá výsledek příkazem (`CommandStep::Dispatch`), který už výkres má.

use std::collections::HashMap;
use std::sync::Mutex;

use ocs_plugin_api::dialog::{DialogDef, DialogResponse, DialogValue};

use ocs_plugin_api::host::{CommandStep, Handle, InteractiveCommand, PreviewWire};
use trasy::geometrie::{geometrie, Prvek, Vrchol};
use crate::lang::tr;
use trasy::trf;

/// Vybere jeden objekt kliknutím a spustí `prikaz <handle>`.
pub struct VyberObjektu {
    pub vyzva: &'static str,
    pub prikaz: &'static str,
}

impl InteractiveCommand for VyberObjektu {
    fn prompt(&self) -> String {
        crate::lang::tr_tab(self.vyzva).to_string()
    }
    fn needs_object_pick(&self) -> bool {
        true
    }
    fn on_object_pick(&mut self, h: Handle, pt: [f64; 3]) -> CommandStep {
        CommandStep::Dispatch(format!("{} {:X} {:.4} {:.4}", self.prikaz, h.value(), pt[0], pt[1]))
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
}

/// Klik do výkresu → spustí `prikaz x y` (nástroje vrcholů editoru).
pub struct VyberBoduJeden {
    pub vyzva: &'static str,
    pub prikaz: &'static str,
}

impl InteractiveCommand for VyberBoduJeden {
    fn prompt(&self) -> String {
        crate::lang::tr_tab(self.vyzva).to_string()
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        CommandStep::Dispatch(format!("{} {:.4} {:.4}", self.prikaz, pt[0], pt[1]))
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
}

/// Výběr více bodů kliknutím: Enter = provést příkaz se všemi body
/// (`PRIKAZ x1 y1 x2 y2 …`), Esc = zrušit. Kliknutá místa se označí křížkem.
pub struct VyberBodu {
    pub vyzva: &'static str,
    pub prikaz: &'static str,
    pub body: Vec<[f64; 2]>,
    /// Velikost značky (ve výkrese).
    pub znacka: f64,
}

impl InteractiveCommand for VyberBodu {
    fn prompt(&self) -> String {
        trf!("{} – vybráno {} (Enter = provést, Esc = zrušit)", "{} – selected {} (Enter = apply, Esc = cancel)", crate::lang::tr_tab(self.vyzva), self.body.len())
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        self.body.push([pt[0], pt[1]]);
        CommandStep::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<CommandStep> {
        self.body.pop().map(|_| CommandStep::NeedPoint)
    }
    fn on_enter(&mut self) -> CommandStep {
        if self.body.is_empty() {
            return CommandStep::Cancel;
        }
        let b: Vec<String> = self.body.iter().map(|p| format!("{:.4} {:.4}", p[0], p[1])).collect();
        CommandStep::Dispatch(format!("{} {}", self.prikaz, b.join(" ")))
    }
    fn on_cursor_move(&mut self, _pt: [f64; 3]) -> Vec<PreviewWire> {
        let d = self.znacka;
        self.body
            .iter()
            .flat_map(|p| {
                [
                    PreviewWire::line([p[0] - d, p[1] - d, 0.0], [p[0] + d, p[1] + d, 0.0], Some([1.0, 0.8, 0.2, 1.0])),
                    PreviewWire::line([p[0] - d, p[1] + d, 0.0], [p[0] + d, p[1] - d, 0.0], Some([1.0, 0.8, 0.2, 1.0])),
                ]
            })
            .collect()
    }
}

/// Výsledek kreslení: vrcholy a id prodlužované trasy (převezme je
/// `TRA_KRESLIT_HOTOVO`).
pub static NAKRESLENO: Mutex<Option<(Vec<Vrchol>, Option<String>)>> = Mutex::new(None);

/// Kreslení trasy klikáním vrcholů tečnového polygonu. Každý vrchol dostane
/// poloměr, který je v panelu editoru v okamžiku kliknutí.
pub struct KresleniTrasy {
    /// Vrcholy (x, y, R).
    pub body: Vec<[f64; 3]>,
    /// Prodlužovaná trasa (id) a kolik vrcholů měla – ty se nevracejí.
    pub trasa: Option<(String, usize)>,
}

impl KresleniTrasy {
    pub fn nova() -> Self {
        Self { body: Vec::new(), trasa: None }
    }
    /// Pokračování z konce existující trasy; poslední vrchol se stane
    /// vnitřním a dostane poloměr z panelu.
    pub fn prodlouzeni(id: String, v: &[Vrchol]) -> Self {
        let mut body: Vec<[f64; 3]> = v.iter().map(|x| [x.x, x.y, x.r]).collect();
        if let Some(b) = body.last_mut() {
            b[2] = crate::tra::editor::polomer();
        }
        let n = body.len();
        Self { body, trasa: Some((id, n)) }
    }
    fn minimum(&self) -> usize {
        self.trasa.as_ref().map_or(0, |t| t.1)
    }
    fn vrcholy(&self, dalsi: Option<[f64; 2]>) -> Vec<Vrchol> {
        let mut b = self.body.clone();
        b.extend(dalsi.map(|p| [p[0], p[1], 0.0]));
        let n = b.len();
        b.iter().enumerate().map(|(i, p)| Vrchol::new(p[0], p[1], if i == 0 || i + 1 == n { 0.0 } else { p[2] })).collect()
    }
    fn vratit_pokud_chce(&mut self) {
        if crate::tra::editor::vezmi_vratit() && self.body.len() > self.minimum() {
            self.body.pop();
        }
    }
}

impl InteractiveCommand for KresleniTrasy {
    fn prompt(&self) -> String {
        let r = crate::tra::editor::polomer();
        if self.body.is_empty() {
            tr("Trasa: klikněte začátek trasy", "Alignment: click the start point").to_string()
        } else {
            trf!("Trasa: klikněte vrchol č. {} (R = {} m z panelu; Enter = hotovo, Esc = zrušit)", "Alignment: click vertex no. {} (R = {} m from the panel; Enter = finish, Esc = cancel)", self.body.len() + 1, r)
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        self.vratit_pokud_chce();
        let p = [pt[0], pt[1]];
        if self.body.last().is_some_and(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-6) {
            return CommandStep::NeedPoint;
        }
        self.body.push([p[0], p[1], crate::tra::editor::polomer()]);
        CommandStep::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<CommandStep> {
        // vrcholy prodlužované trasy se nevracejí
        (self.body.len() > self.minimum()).then(|| {
            self.body.pop();
            CommandStep::NeedPoint
        })
    }
    fn on_enter(&mut self) -> CommandStep {
        self.vratit_pokud_chce();
        if self.body.len() < 2 || self.body.len() <= self.minimum() {
            return CommandStep::Cancel;
        }
        let id = self.trasa.as_ref().map(|t| t.0.clone());
        *NAKRESLENO.lock().unwrap_or_else(|e| e.into_inner()) = Some((self.vrcholy(None), id));
        CommandStep::Dispatch("TRA_KRESLIT_HOTOVO".into())
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        self.vratit_pokud_chce();
        if self.body.is_empty() {
            return Vec::new();
        }
        // poslední kliknutý vrchol se stane vnitřním s poloměrem z panelu
        if self.body.len() > self.minimum() {
            if let Some(b) = self.body.last_mut() {
                b[2] = crate::tra::editor::polomer();
            }
        }
        let mut v = self.vrcholy(Some([pt[0], pt[1]]));
        // náhled s poloměry zmenšenými tak, aby se vešly (stejně jako po Enter)
        trasy::geometrie::prizpusob_polomery(&mut v);
        let polygon = PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [0.6, 0.6, 0.6, 1.0]);
        let mut out = vec![polygon];
        match geometrie(&v, 0.0) {
            Ok(g) => {
                for (_, p) in &g.prvky {
                    match *p {
                        Prvek::Primka { a, b } => out.push(PreviewWire::line([a[0], a[1], 0.0], [b[0], b[1], 0.0], None)),
                        Prvek::Oblouk { stred, r, uhel_a, sweep, .. } => {
                            out.push(crate::tra::uchopy::oblouk_nahledu(stred, r, uhel_a, sweep, None))
                        }
                        Prvek::Prechodnice { .. } => out.push(crate::tra::uchopy::prechodnice_nahledu(p, None)),
                    }
                }
            }
            Err(_) => out[0] = PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [1.0, 0.2, 0.2, 1.0]),
        }
        out
    }
}

/// Odpověď posledního dialogu (převezme ji příkaz z `DialogPrikaz::prikaz`).
pub static ODPOVED: Mutex<Option<HashMap<String, DialogValue>>> = Mutex::new(None);

pub fn vezmi_odpoved() -> Option<HashMap<String, DialogValue>> {
    ODPOVED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Dialog Open CAD Studia. Odpověď dialogu dostane jen běžící interaktivní
/// příkaz – tenhle ji uloží do [`ODPOVED`] a spustí `prikaz`, který ji použije.
pub struct DialogPrikaz {
    pub def: DialogDef,
    pub prikaz: String,
    pub hotovo: bool,
    /// Po potvrzení dialogu ještě klik do výkresu (výzva); příkaz pak dostane
    /// souřadnice `x y`. Dialog musí být první – OCS ho po kliknutí neotevře.
    pub klik: Option<&'static str>,
}

impl DialogPrikaz {
    pub fn new(def: DialogDef, prikaz: String) -> Self {
        Self { def, prikaz, hotovo: false, klik: None }
    }
    pub fn s_klikem(def: DialogDef, prikaz: String, vyzva: &'static str) -> Self {
        Self { def, prikaz, hotovo: false, klik: Some(vyzva) }
    }
}

impl InteractiveCommand for DialogPrikaz {
    fn prompt(&self) -> String {
        match (self.hotovo, self.klik) {
            (true, Some(v)) => crate::lang::tr_tab(v).to_string(),
            _ => self.def.title.clone(),
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        if self.hotovo && self.klik.is_some() {
            return CommandStep::Dispatch(format!("{} {:.4} {:.4}", self.prikaz, pt[0], pt[1]));
        }
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn modal_dialog(&self) -> Option<DialogDef> {
        (!self.hotovo).then(|| self.def.clone())
    }
    fn on_dialog_response(&mut self, r: DialogResponse) -> CommandStep {
        self.hotovo = true;
        match r {
            DialogResponse::Submitted(m) => {
                *ODPOVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(m);
                if self.klik.is_some() {
                    CommandStep::NeedPoint
                } else {
                    CommandStep::Dispatch(self.prikaz.clone())
                }
            }
            _ => CommandStep::Cancel,
        }
    }
}

/// Oblouk ve vrcholu bez dialogu: klik na vrchol, pak poloměr – napsaný do
/// příkazového řádku (OCS ho předá jako bod v té vzdálenosti od kliknutí),
/// nebo kliknutý (poloměr = vzdálenost od vrcholu). Enter = poloměr z panelu.
pub struct ObloukVeVrcholu {
    /// Trasy výkresu: vložení, vrcholy (světové), počáteční staničení.
    pub trasy: Vec<(Handle, Vec<Vrchol>, f64)>,
    /// Vybraný vrchol: (index trasy, index vrcholu, bod kliknutí).
    pub vybrany: Option<(usize, usize, [f64; 2])>,
}

impl ObloukVeVrcholu {
    fn prikaz(&self, r: f64) -> CommandStep {
        match self.vybrany {
            Some((t, i, _)) => CommandStep::Dispatch(format!("TRA_OBLOUK_NASTAV {:X} {} {:.4}", self.trasy[t].0.value(), i, r.max(0.0))),
            None => CommandStep::Cancel,
        }
    }
}

impl InteractiveCommand for ObloukVeVrcholu {
    fn prompt(&self) -> String {
        match self.vybrany {
            None => tr("Oblouk: klikněte na vrchol trasy", "Curve: click an alignment vertex").to_string(),
            Some((_, i, _)) => trf!(
                "Oblouk ve vrcholu {}: napište poloměr R a Enter (nebo klikněte – R = vzdálenost od vrcholu; jen Enter = R {} m z panelu)", "Curve at vertex {}: type radius R and Enter (or click – R = distance from the vertex; Enter alone = R {} m from the panel)",
                i + 1,
                crate::tra::editor::polomer()
            ),
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let p = [pt[0], pt[1]];
        if let Some((_, _, p1)) = self.vybrany {
            let r = (p[0] - p1[0]).hypot(p[1] - p1[1]);
            return self.prikaz(r);
        }
        let mut nej: Option<(f64, usize, usize)> = None;
        for (t, (_, v, _)) in self.trasy.iter().enumerate() {
            for i in 1..v.len().saturating_sub(1) {
                let d = (v[i].x - p[0]).hypot(v[i].y - p[1]);
                if nej.is_none_or(|n| d < n.0) {
                    nej = Some((d, t, i));
                }
            }
        }
        if let Some((_, t, i)) = nej {
            self.vybrany = Some((t, i, p));
        }
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        self.prikaz(crate::tra::editor::polomer())
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some((t, i, p1)) = self.vybrany else { return Vec::new() };
        let r = (pt[0] - p1[0]).hypot(pt[1] - p1[1]);
        let (_, v, pocatek) = &self.trasy[t];
        let mut v = v.clone();
        v[i].r = r;
        let mut out = vec![PreviewWire::line([p1[0], p1[1], 0.0], [pt[0], pt[1], 0.0], Some([0.6, 0.6, 0.6, 1.0]))];
        match geometrie(&v, *pocatek) {
            Ok(g) => {
                for (_, p) in &g.prvky {
                    match *p {
                        Prvek::Primka { a, b } => {
                            out.push(PreviewWire::line([a[0], a[1], 0.0], [b[0], b[1], 0.0], Some([1.0, 0.8, 0.2, 1.0])))
                        }
                        Prvek::Oblouk { stred, r, uhel_a, sweep, .. } => {
                            out.push(crate::tra::uchopy::oblouk_nahledu(stred, r, uhel_a, sweep, Some([1.0, 0.8, 0.2, 1.0])))
                        }
                        Prvek::Prechodnice { .. } => out.push(crate::tra::uchopy::prechodnice_nahledu(p, Some([1.0, 0.8, 0.2, 1.0]))),
                    }
                }
            }
            Err(_) => out.push(PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [1.0, 0.2, 0.2, 1.0])),
        }
        out
    }
}

/// Pohled pro kreslení nivelety: id, vložení (levý dolní roh), osy.
pub struct PohledKresleni {
    pub id: String,
    pub vlozeni: [f64; 2],
    /// od, do, srovnávací rovina, horní okraj, převýšení
    pub osy: [f64; 5],
}

impl PohledKresleni {
    fn obsahuje(&self, p: [f64; 2]) -> bool {
        let [od, do_, sr, horni, prev] = self.osy;
        let (x, y) = (p[0] - self.vlozeni[0], p[1] - self.vlozeni[1]);
        x >= -1e-6 && x <= do_ - od + 1e-6 && y >= -1e-6 && y <= (horni - sr) * prev + 1e-6
    }
    /// Bod výkresu → (staničení, výška).
    fn na_profil(&self, p: [f64; 2]) -> (f64, f64) {
        let [od, do_, sr, _, prev] = self.osy;
        ((od + p[0] - self.vlozeni[0]).clamp(od, do_), sr + (p[1] - self.vlozeni[1]) / prev)
    }
    /// (staničení, výška) → bod výkresu.
    fn na_vykres(&self, s: f64, z: f64) -> [f64; 3] {
        let [od, _, sr, _, prev] = self.osy;
        [self.vlozeni[0] + s - od, self.vlozeni[1] + (z - sr) * prev, 0.0]
    }
}

/// Výsledek kreslení nivelety: id pohledu a vrcholy.
pub static NAKRESLENA_NIVELETA: Mutex<Option<(String, Vec<trasy::niveleta::VrcholNivelety>)>> = Mutex::new(None);

/// Výchozí poloměr zakružovacích oblouků nivelety [m].
pub const R_NIVELETY: f64 = 2000.0;

/// Kreslení nivelety klikáním vrcholů přímo v pohledu podélného profilu.
pub struct KresleniNivelety {
    pub pohledy: Vec<PohledKresleni>,
    pub vybrany: Option<usize>,
    pub body: Vec<(f64, f64)>,
}

impl KresleniNivelety {
    fn vrcholy(&self, dalsi: Option<(f64, f64)>) -> Vec<trasy::niveleta::VrcholNivelety> {
        let mut b = self.body.clone();
        b.extend(dalsi);
        let n = b.len();
        b.iter()
            .enumerate()
            .map(|(i, &(s, z))| trasy::niveleta::VrcholNivelety::new(s, z, if i == 0 || i + 1 == n { 0.0 } else { R_NIVELETY }))
            .collect()
    }
}

impl InteractiveCommand for KresleniNivelety {
    fn prompt(&self) -> String {
        if self.body.is_empty() {
            tr("Niveleta: klikněte v pohledu profilu začátek nivelety", "Design profile: click the start of the design profile in a profile view").to_string()
        } else {
            trf!("Niveleta: klikněte vrchol č. {} (oblouky R = {} m; Enter = hotovo, Esc = zrušit)", "Design profile: click vertex no. {} (curves R = {} m; Enter = finish, Esc = cancel)", self.body.len() + 1, R_NIVELETY)
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let p = [pt[0], pt[1]];
        if self.vybrany.is_none() {
            self.vybrany = self.pohledy.iter().position(|v| v.obsahuje(p));
            if self.vybrany.is_none() {
                return CommandStep::NeedPoint;
            }
        }
        let Some(v) = self.vybrany.and_then(|i| self.pohledy.get(i)) else { return CommandStep::NeedPoint };
        let (s, z) = v.na_profil(p);
        if self.body.last().is_some_and(|q| s <= q.0 + 1e-3) {
            return CommandStep::NeedPoint; // vrcholy musí jít dopředu ve staničení
        }
        self.body.push((s, z));
        CommandStep::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<CommandStep> {
        self.body.pop()?;
        if self.body.is_empty() {
            self.vybrany = None;
        }
        Some(CommandStep::NeedPoint)
    }
    fn on_enter(&mut self) -> CommandStep {
        let Some(v) = self.vybrany.and_then(|i| self.pohledy.get(i)) else { return CommandStep::Cancel };
        if self.body.len() < 2 {
            return CommandStep::Cancel;
        }
        *NAKRESLENA_NIVELETA.lock().unwrap_or_else(|e| e.into_inner()) = Some((v.id.clone(), self.vrcholy(None)));
        CommandStep::Dispatch("TRA_NIVELETA_HOTOVO".into())
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some(v) = self.vybrany.and_then(|i| self.pohledy.get(i)) else { return Vec::new() };
        if self.body.is_empty() {
            return Vec::new();
        }
        let (s, z) = v.na_profil([pt[0], pt[1]]);
        let dalsi = (s > self.body.last().map_or(f64::MIN, |q| q.0) + 1e-3).then_some((s, z));
        let n = trasy::niveleta::Niveleta::nova("N", "", self.vrcholy(dalsi));
        let polygon = PreviewWire::with_color(n.vrcholy.iter().map(|x| v.na_vykres(x.s, x.z)).collect(), [0.6, 0.6, 0.6, 1.0]);
        let mut out = vec![polygon];
        if n.kontrola().is_ok() && n.vrcholy.len() >= 2 {
            out.push(PreviewWire::with_color(n.body().into_iter().map(|b| v.na_vykres(b[0], b[1])).collect(), [1.0, 0.3, 0.3, 1.0]));
        } else {
            out[0] = PreviewWire::with_color(n.vrcholy.iter().map(|x| v.na_vykres(x.s, x.z)).collect(), [1.0, 0.2, 0.2, 1.0]);
        }
        out
    }
}

/// Vrchol nivelety sklonem: klik do pohledu = staničení nového vrcholu, pak
/// sklon od vybraného vrcholu – napsaný do příkazového řádku (OCS ho předá
/// jako bod v té vzdálenosti od kliknutí; kurzor nad kliknutím = stoupání,
/// pod ním = klesání), nebo kliknutá výška. Enter = vrchol na stávající
/// niveletě (tečnovém polygonu).
pub struct VrcholSklonem {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    /// Vrchol, od kterého se sklon měří (vybraný v panelu); `None` = předchozí vrchol.
    pub od: Option<usize>,
    /// Po prvním kliknutí: pohled, staničení, vrchol „od“, bod kliknutí.
    pub klik: Option<(usize, f64, usize, [f64; 3])>,
}

impl VrcholSklonem {
    /// Výška tečnového polygonu ve staničení (mimo rozsah prodloužený krajní úsek).
    fn polygon(&self, s: f64) -> f64 {
        let v = &self.vrcholy;
        let k = v.windows(2).position(|w| s <= w[1].s).unwrap_or(v.len().saturating_sub(2));
        let (a, b) = (v[k], v[(k + 1).min(v.len() - 1)]);
        if (b.s - a.s).abs() < 1e-9 { a.z } else { a.z + (b.z - a.z) * (s - a.s) / (b.s - a.s) }
    }

    fn hotovo(&self, sklon: f64) -> CommandStep {
        let Some((_, s, od, _)) = self.klik else { return CommandStep::Cancel };
        CommandStep::Dispatch(format!("TRA_NIV_SKLON_HOTOVO {} {} {:.4} {:.7}", self.trasa, od, s, sklon))
    }
}

impl InteractiveCommand for VrcholSklonem {
    fn prompt(&self) -> String {
        match self.klik {
            None => tr("Vrchol sklonem: klikněte v pohledu profilu staničení nového vrcholu", "Vertex by grade: click the station of the new vertex in the profile view").to_string(),
            Some((_, s, od, _)) => {
                let i = (self.polygon(s) - self.vrcholy[od].z) / (s - self.vrcholy[od].s);
                trf!(
                    "Sklon od vrcholu {} [‰]: napište číslo a Enter (kurzor nad kliknutím = stoupání, pod = klesání), nebo klikněte výšku; jen Enter = {} ‰ (na niveletě)", "Grade from vertex {} [‰]: type a number and Enter (cursor above the click = rising, below = falling), or click the elevation; Enter alone = {} ‰ (on the design profile)",
                    od + 1,
                    trasy::stanice::cislo(i * 1000.0, 2, true)
                )
            }
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.klik {
            None => {
                let Some(p) = self.pohledy.iter().position(|p| p.obsahuje([pt[0], pt[1]])) else { return CommandStep::NeedPoint };
                let Some((s, _)) = self.pohledy[p].na_profil(pt) else { return CommandStep::NeedPoint };
                let od = self
                    .od
                    .filter(|&i| i < self.vrcholy.len() && (self.vrcholy[i].s - s).abs() > 1e-3)
                    .unwrap_or_else(|| self.vrcholy.iter().rposition(|v| v.s < s).unwrap_or(0));
                if (self.vrcholy[od].s - s).abs() < 1e-3 {
                    return CommandStep::NeedPoint;
                }
                self.klik = Some((p, s, od, pt));
                CommandStep::NeedPoint
            }
            Some((p, s, od, p1)) => {
                let sklon = match crate::tra::uchopy::napsana_vzdalenost(p1, pt) {
                    Some(d) => (if pt[1] >= p1[1] { d } else { -d }) / 1000.0,
                    None => {
                        let Some((_, z)) = self.pohledy[p].na_profil(pt) else { return CommandStep::NeedPoint };
                        (z - self.vrcholy[od].z) / (s - self.vrcholy[od].s)
                    }
                };
                self.hotovo(sklon)
            }
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        match self.klik {
            Some((_, s, od, _)) => self.hotovo((self.polygon(s) - self.vrcholy[od].z) / (s - self.vrcholy[od].s)),
            None => CommandStep::Cancel,
        }
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some((p, s, od, _)) = self.klik else { return Vec::new() };
        let ph = &self.pohledy[p];
        let Some((_, z)) = ph.na_profil(pt) else { return Vec::new() };
        let v = self.vrcholy[od];
        match (ph.na_vykres(v.s, v.z), ph.na_vykres(s, z)) {
            (Some(a), Some(b)) => vec![PreviewWire::line(a, b, Some([1.0, 0.8, 0.2, 1.0]))],
            _ => Vec::new(),
        }
    }
}

/// Přidat vrcholy nivelety: klikání do pohledu (výška = kliknutá), Enter = hotovo.
pub struct PridatVrcholyNivelety {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    pub pridane: Vec<(f64, f64)>,
}

impl PridatVrcholyNivelety {
    fn s_pridanymi(&self, navic: Option<(f64, f64)>) -> Vec<trasy::niveleta::VrcholNivelety> {
        let mut v = self.vrcholy.clone();
        for &(s, z) in self.pridane.iter().chain(navic.iter()) {
            let _ = trasy::niveleta::vloz_vrchol(&mut v, s, z, 0.0);
        }
        v
    }
}

impl InteractiveCommand for PridatVrcholyNivelety {
    fn prompt(&self) -> String {
        trf!("Přidat vrchol nivelety: klikněte do pohledu profilu (přidáno {}; Enter = hotovo, Esc = zrušit)", "Add design profile vertex: click in the profile view (added {}; Enter = done, Esc = cancel)", self.pridane.len())
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        if let Some(p) = self.pohledy.iter().find(|p| p.obsahuje([pt[0], pt[1]])) {
            if let Some((s, z)) = p.na_profil(pt) {
                let v = self.s_pridanymi(None);
                if !v.iter().any(|x| (x.s - s).abs() < 1e-3) {
                    self.pridane.push((s, z));
                }
            }
        }
        CommandStep::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<CommandStep> {
        self.pridane.pop().map(|_| CommandStep::NeedPoint)
    }
    fn on_enter(&mut self) -> CommandStep {
        if self.pridane.is_empty() {
            return CommandStep::Cancel;
        }
        let body: Vec<String> = self.pridane.iter().map(|(s, z)| format!("{s:.4} {z:.4}")).collect();
        CommandStep::Dispatch(format!("TRA_NIV_PRIDAT_HOTOVO {} {}", self.trasa, body.join(" ")))
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some(p) = self.pohledy.iter().find(|p| p.obsahuje([pt[0], pt[1]])) else { return Vec::new() };
        let v = self.s_pridanymi(p.na_profil(pt));
        let b: Vec<[f64; 3]> = v.iter().filter_map(|x| p.na_vykres(x.s, x.z)).collect();
        vec![PreviewWire::with_color(b, [1.0, 0.8, 0.2, 1.0])]
    }
}

/// Změnit oblouk nivelety: klik na vrchol, pak číslo na příkazovém řádku
/// (OCS ho předá jako bod v té vzdálenosti od kliknutí). Enter přepíná
/// zadání L → R → K, 0 = bez oblouku.
pub struct ZmenitObloukNivelety {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    /// Vybraný vrchol, bod kliknutí a zadání (0 = L, 1 = R, 2 = K).
    pub vybrany: Option<(usize, [f64; 3], usize)>,
}

const ZADANI_OBLOUKU: [&str; 3] = ["L", "R", "K"];

impl InteractiveCommand for ZmenitObloukNivelety {
    fn prompt(&self) -> String {
        match self.vybrany {
            None => tr("Změnit oblouk: klikněte na vrchol nivelety v pohledu profilu", "Change curve: click a design profile vertex in the profile view").to_string(),
            Some((k, _, m)) => {
                let nes = matches!(self.vrcholy[k].typ, trasy::niveleta::TypOblouku::Nesoumerny { .. });
                let dalsi = ZADANI_OBLOUKU[(m + 1) % 3];
                if nes {
                    trf!("Nesouměrný oblouk ve vrcholu {}: napište L [m] a Enter (L1 : L2 zůstane; 0 = bez oblouku)", "Unsymmetrical curve at vertex {}: type L [m] and Enter (L1 : L2 is kept; 0 = no curve)", k + 1)
                } else {
                    trf!(
                        "Oblouk ve vrcholu {}: {} = ? – napište číslo a Enter (0 = bez oblouku; jen Enter = přepnout na {})", "Curve at vertex {}: {} = ? – type a number and Enter (0 = no curve; Enter alone = switch to {})",
                        k + 1,
                        ZADANI_OBLOUKU[m],
                        dalsi
                    )
                }
            }
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.vybrany {
            None => {
                // nejbližší vnitřní vrchol (ve výkrese) v pohledu, kam se kliklo
                let Some(p) = self.pohledy.iter().find(|p| p.obsahuje([pt[0], pt[1]])) else { return CommandStep::NeedPoint };
                let n = self.vrcholy.len();
                let k = (1..n.saturating_sub(1))
                    .filter_map(|k| p.na_vykres(self.vrcholy[k].s, self.vrcholy[k].z).map(|q| (k, (q[0] - pt[0]).hypot(q[1] - pt[1]))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|x| x.0);
                let Some(k) = k else { return CommandStep::NeedPoint };
                let m = match self.vrcholy[k].zadani {
                    trasy::niveleta::Zadani::R => 1,
                    trasy::niveleta::Zadani::K => 2,
                    trasy::niveleta::Zadani::L(_) => 0,
                };
                let m = if matches!(self.vrcholy[k].typ, trasy::niveleta::TypOblouku::Nesoumerny { .. }) { 0 } else { m };
                self.vybrany = Some((k, pt, m));
                CommandStep::NeedPoint
            }
            // napsaná 0 přijde jako bod v místě kliknutí
            Some((k, p1, m)) => match crate::tra::uchopy::napsana_vzdalenost(p1, pt).or(((pt[0] - p1[0]).hypot(pt[1] - p1[1]) < 1e-9).then_some(0.0)) {
                Some(h) => CommandStep::Dispatch(format!("TRA_NIV_OBLOUK_NASTAV {} {} {} {:.4}", self.trasa, k, ZADANI_OBLOUKU[m], h)),
                None => CommandStep::NeedPoint,
            },
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        match self.vybrany {
            Some((k, p, m)) => {
                if !matches!(self.vrcholy[k].typ, trasy::niveleta::TypOblouku::Nesoumerny { .. }) {
                    self.vybrany = Some((k, p, (m + 1) % 3));
                }
                CommandStep::NeedPoint
            }
            None => CommandStep::Cancel,
        }
    }
}

/// Úsek nivelety v pohledu, kam se kliklo (index úseku k → k+1).
fn usek_u_bodu(v: &[trasy::niveleta::VrcholNivelety], s: f64) -> Option<usize> {
    if v.len() < 2 {
        return None;
    }
    Some(v.windows(2).position(|w| s <= w[1].s).unwrap_or(v.len() - 2))
}

/// Hodnota napsaná na příkazovém řádku (bod ve vzdálenosti od posledního
/// bodu); znaménko podle polohy kurzoru (nad = plus, pod = minus).
fn napsane_se_znamenkem(p1: [f64; 3], pt: [f64; 3]) -> Option<f64> {
    crate::tra::uchopy::napsana_vzdalenost(p1, pt).map(|d| if pt[1] >= p1[1] { d } else { -d })
}

/// Co nástroj úseku mění.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DruhUseku {
    Sklon,
    Delka,
    Bodem,
}

/// Nástroje úseků nivelety: klik na úsek, pak sklon / délka na příkazovém
/// řádku, nebo bod, kterým má úsek projít.
pub struct UsekNivelety {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    pub druh: DruhUseku,
    /// Pohled, úsek a bod kliknutí.
    pub usek: Option<(usize, usize, [f64; 3])>,
}

impl UsekNivelety {
    fn hotovo(&self, k: usize, co: &str, x: f64) -> CommandStep {
        CommandStep::Dispatch(format!("TRA_NIV_USEK_HOTOVO {} {} {} {:.7}", self.trasa, k, co, x))
    }
}

impl InteractiveCommand for UsekNivelety {
    fn prompt(&self) -> String {
        let v = &self.vrcholy;
        match (self.usek, self.druh) {
            (None, DruhUseku::Sklon) => tr("Změnit sklon: klikněte na úsek nivelety v pohledu profilu", "Change grade: click a design profile segment in the profile view").to_string(),
            (None, DruhUseku::Delka) => tr("Změnit délku: klikněte na úsek nivelety v pohledu profilu", "Change length: click a design profile segment in the profile view").to_string(),
            (None, DruhUseku::Bodem) => tr("Úsek bodem: klikněte na úsek nivelety v pohledu profilu", "Segment through point: click a design profile segment in the profile view").to_string(),
            (Some((_, k, _)), DruhUseku::Sklon) => trf!(
                "Sklon úseku {}–{} [‰] (teď {}): napište číslo a Enter (kurzor nad kliknutím = stoupání, pod = klesání)", "Grade of segment {}–{} [‰] (now {}): type a number and Enter (cursor above the click = rising, below = falling)",
                k + 1,
                k + 2,
                trasy::stanice::cislo((v[k + 1].z - v[k].z) / (v[k + 1].s - v[k].s) * 1000.0, 2, true)
            ),
            (Some((_, k, _)), DruhUseku::Delka) => trf!(
                "Délka úseku {}–{} [m] (teď {}): napište číslo a Enter", "Length of segment {}–{} [m] (now {}): type a number and Enter",
                k + 1,
                k + 2,
                trasy::stanice::cislo(v[k + 1].s - v[k].s, 2, true)
            ),
            (Some((_, k, _)), DruhUseku::Bodem) => trf!("Úsek {}–{}: klikněte bod, kterým má projít (začátek úseku drží)", "Segment {}–{}: click the point it should pass through (the segment start is kept)", k + 1, k + 2),
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.usek {
            None => {
                let Some(p) = self.pohledy.iter().position(|p| p.obsahuje([pt[0], pt[1]])) else { return CommandStep::NeedPoint };
                let Some((s, _)) = self.pohledy[p].na_profil(pt) else { return CommandStep::NeedPoint };
                let Some(k) = usek_u_bodu(&self.vrcholy, s) else { return CommandStep::NeedPoint };
                self.usek = Some((p, k, pt));
                CommandStep::NeedPoint
            }
            Some((p, k, p1)) => match self.druh {
                DruhUseku::Sklon => match napsane_se_znamenkem(p1, pt) {
                    Some(x) => self.hotovo(k, "S", x / 1000.0),
                    None => CommandStep::NeedPoint,
                },
                DruhUseku::Delka => match crate::tra::uchopy::napsana_vzdalenost(p1, pt) {
                    Some(x) => self.hotovo(k, "D", x),
                    None => CommandStep::NeedPoint,
                },
                DruhUseku::Bodem => {
                    let Some((s, z)) = self.pohledy[p].na_profil(pt) else { return CommandStep::NeedPoint };
                    let a = self.vrcholy[k];
                    if s <= a.s + 1e-3 {
                        return CommandStep::NeedPoint;
                    }
                    self.hotovo(k, "S", (z - a.z) / (s - a.s))
                }
            },
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some((p, k, _)) = self.usek else { return Vec::new() };
        if self.druh != DruhUseku::Bodem {
            return Vec::new();
        }
        let ph = &self.pohledy[p];
        let a = self.vrcholy[k];
        match ph.na_vykres(a.s, a.z) {
            Some(q) => vec![PreviewWire::line(q, [pt[0], pt[1], 0.0], Some([1.0, 0.8, 0.2, 1.0]))],
            None => Vec::new(),
        }
    }
}

/// Zvednout / snížit: klik na vrchol (nebo do pohledu mimo vrcholy = celá
/// niveleta), pak výška na příkazovém řádku (kurzor nad kliknutím = nahoru,
/// pod = dolů).
pub struct ZvednoutVrchol {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    /// Vybraný vrchol (`None` = celá niveleta) a bod kliknutí.
    pub vybrany: Option<(Option<usize>, [f64; 3])>,
}

impl InteractiveCommand for ZvednoutVrchol {
    fn prompt(&self) -> String {
        match self.vybrany {
            None => tr("Zvednout / snížit: klikněte na vrchol nivelety, nebo do pohledu mimo vrcholy = celá niveleta", "Raise / lower: click a design profile vertex, or in the view away from vertices = whole profile").to_string(),
            Some((Some(k), _)) => trf!(
                "Vrchol {}: o kolik zvednout [m] – napište číslo a Enter (kurzor nad kliknutím = nahoru, pod = dolů)", "Vertex {}: raise by [m] – type a number and Enter (cursor above the click = up, below = down)",
                k + 1
            ),
            Some((None, _)) => {
                tr("Celá niveleta: o kolik zvednout [m] – napište číslo a Enter (kurzor nad kliknutím = nahoru, pod = dolů)", "Whole profile: raise by [m] – type a number and Enter (cursor above the click = up, below = down)").to_string()
            }
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.vybrany {
            None => {
                let Some(p) = self.pohledy.iter().find(|p| p.obsahuje([pt[0], pt[1]])) else { return CommandStep::NeedPoint };
                // vrchol blíž než 3 % šířky pohledu, jinak celá niveleta
                let sirka = p.def.osy.map_or(10.0, |o| (o[1] - o[0]).abs()).max(1.0);
                let k = (0..self.vrcholy.len())
                    .filter_map(|k| p.na_vykres(self.vrcholy[k].s, self.vrcholy[k].z).map(|q| (k, (q[0] - pt[0]).hypot(q[1] - pt[1]))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .filter(|x| x.1 < 0.03 * sirka)
                    .map(|x| x.0);
                self.vybrany = Some((k, pt));
                CommandStep::NeedPoint
            }
            Some((k, p1)) => match napsane_se_znamenkem(p1, pt) {
                Some(dz) => {
                    let kde = k.map_or("v".to_string(), |k| k.to_string());
                    CommandStep::Dispatch(format!("TRA_NIV_ZVEDNOUT_HOTOVO {} {} {:.4}", self.trasa, kde, dz))
                }
                None => CommandStep::NeedPoint,
            },
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
}

/// Prodloužit niveletu: klik za konec (přibližné místo), pak sklon a délka
/// nového úseku na příkazovém řádku.
pub struct ProdlouzitNiveletu {
    pub trasa: String,
    pub pohledy: Vec<crate::tra::pohled::ObjektPohledu>,
    pub vrcholy: Vec<trasy::niveleta::VrcholNivelety>,
    /// Po kliknutí: staničení kliknutí, bod kliknutí a poslední zadaný bod.
    pub klik: Option<(f64, [f64; 3], [f64; 3])>,
    pub sklon: Option<f64>,
}

impl ProdlouzitNiveletu {
    fn posledni_sklon(&self) -> f64 {
        let v = &self.vrcholy;
        let n = v.len();
        if n < 2 { 0.0 } else { (v[n - 1].z - v[n - 2].z) / (v[n - 1].s - v[n - 2].s) }
    }
    fn hotovo(&self, sklon: f64, delka: f64) -> CommandStep {
        if delka <= 0.01 {
            return CommandStep::Cancel;
        }
        CommandStep::Dispatch(format!("TRA_NIV_PRODLOUZIT_HOTOVO {} {:.7} {:.4}", self.trasa, sklon, delka))
    }
}

impl InteractiveCommand for ProdlouzitNiveletu {
    fn prompt(&self) -> String {
        let konec = self.vrcholy.last().map_or(0.0, |v| v.s);
        match (self.klik, self.sklon) {
            (None, _) => tr("Prodloužit: klikněte v pohledu profilu za konec nivelety (přibližné místo nového konce)", "Extend: click in the profile view beyond the design profile end (approximate place of the new end)").to_string(),
            (Some(_), None) => trf!(
                "Sklon nového úseku [‰]: napište číslo a Enter (kurzor nad kliknutím = stoupání, pod = klesání; jen Enter = {} ‰)", "Grade of the new segment [‰]: type a number and Enter (cursor above the click = rising, below = falling; Enter alone = {} ‰)",
                trasy::stanice::cislo(self.posledni_sklon() * 1000.0, 2, true)
            ),
            (Some((s, _, _)), Some(_)) => trf!(
                "Délka nového úseku [m]: napište číslo a Enter (jen Enter = {} m podle kliknutí)", "Length of the new segment [m]: type a number and Enter (Enter alone = {} m from the click)",
                trasy::stanice::cislo((s - konec).max(0.0), 2, true)
            ),
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let konec = self.vrcholy.last().map_or(0.0, |v| v.s);
        match (self.klik, self.sklon) {
            (None, _) => {
                let Some(p) = self.pohledy.iter().find(|p| p.obsahuje([pt[0], pt[1]])) else { return CommandStep::NeedPoint };
                let Some((s, _)) = p.na_profil(pt) else { return CommandStep::NeedPoint };
                if s <= konec + 1e-3 {
                    return CommandStep::NeedPoint;
                }
                self.klik = Some((s, pt, pt));
                CommandStep::NeedPoint
            }
            (Some((s, p1, posledni)), None) => match napsane_se_znamenkem(posledni, pt).or_else(|| napsane_se_znamenkem(p1, pt)) {
                Some(x) => {
                    self.sklon = Some(x / 1000.0);
                    self.klik = Some((s, p1, pt));
                    CommandStep::NeedPoint
                }
                None => CommandStep::NeedPoint,
            },
            (Some((_, p1, posledni)), Some(sklon)) => {
                match crate::tra::uchopy::napsana_vzdalenost(posledni, pt).or_else(|| crate::tra::uchopy::napsana_vzdalenost(p1, pt)) {
                    Some(d) => self.hotovo(sklon, d),
                    None => CommandStep::NeedPoint,
                }
            }
        }
    }
    fn on_enter(&mut self) -> CommandStep {
        let konec = self.vrcholy.last().map_or(0.0, |v| v.s);
        match (self.klik, self.sklon) {
            (None, _) => CommandStep::Cancel,
            (Some(_), None) => {
                self.sklon = Some(self.posledni_sklon());
                CommandStep::NeedPoint
            }
            (Some((s, _, _)), Some(sklon)) => self.hotovo(sklon, s - konec),
        }
    }
}

/// Posunout vrchol trasy přesně: klik na vrchol, pak nové místo (klik nebo
/// napsané X,Y). Hotovo = příkaz TRA_POSUN_HOTOVO trasa vrchol x y.
pub struct PosunVrcholu {
    /// Trasy výkresu: vložení, vrcholy (světové), počáteční staničení.
    pub trasy: Vec<(Handle, Vec<Vrchol>, f64)>,
    pub vybrany: Option<(usize, usize)>,
}

impl InteractiveCommand for PosunVrcholu {
    fn prompt(&self) -> String {
        match self.vybrany {
            None => tr("Posunout vrchol: klikněte na vrchol trasy", "Move vertex: click an alignment vertex").to_string(),
            Some((_, i)) => trf!("Vrchol {}: klikněte nové místo, nebo napište souřadnice X,Y a Enter (Esc = zrušit)", "Vertex {}: click the new location, or type X,Y and Enter (Esc = cancel)", i + 1),
        }
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let p = [pt[0], pt[1]];
        if let Some((t, i)) = self.vybrany {
            return CommandStep::Dispatch(format!("TRA_POSUN_HOTOVO {:X} {} {:.6} {:.6}", self.trasy[t].0.value(), i, p[0], p[1]));
        }
        let mut nej: Option<(f64, usize, usize)> = None;
        for (t, (_, v, _)) in self.trasy.iter().enumerate() {
            for (i, x) in v.iter().enumerate() {
                let d = (x.x - p[0]).hypot(x.y - p[1]);
                if nej.is_none_or(|n| d < n.0) {
                    nej = Some((d, t, i));
                }
            }
        }
        if let Some((_, t, i)) = nej {
            self.vybrany = Some((t, i));
            crate::tra::editor::vyber_a_obnov(i);
        }
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some((t, i)) = self.vybrany else { return Vec::new() };
        let (_, v, pocatek) = &self.trasy[t];
        let mut v = v.clone();
        v[i].x = pt[0];
        v[i].y = pt[1];
        let mut out = vec![PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [0.6, 0.6, 0.6, 1.0])];
        match geometrie(&v, *pocatek) {
            Ok(g) => {
                for (_, p) in &g.prvky {
                    out.push(match *p {
                        Prvek::Primka { a, b } => PreviewWire::line([a[0], a[1], 0.0], [b[0], b[1], 0.0], Some([1.0, 0.8, 0.2, 1.0])),
                        Prvek::Oblouk { stred, r, uhel_a, sweep, .. } => {
                            crate::tra::uchopy::oblouk_nahledu(stred, r, uhel_a, sweep, Some([1.0, 0.8, 0.2, 1.0]))
                        }
                        Prvek::Prechodnice { .. } => crate::tra::uchopy::prechodnice_nahledu(p, Some([1.0, 0.8, 0.2, 1.0])),
                    });
                }
            }
            Err(_) => out[0] = PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [1.0, 0.2, 0.2, 1.0]),
        }
        out
    }
}
