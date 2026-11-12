//! Logika Prospektoru a automatického přepočtu – nezávislá na okně.
//!
//! Běží ve vlastním vlákně pluginu. Do výkresu zasahuje přes kanál požadavků
//! (`PluginRequestSender`), který Open CAD Studio obsluhuje průběžně (~10× za
//! sekundu), i když zrovna neběží žádný příkaz pluginu. Okno (nebo později
//! panel Open CAD Studia) jen čte `stav()` a posílá `Akce`.

use teren::preklad::tr;
use teren::trf;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ocs_plugin_api::host::codec::entities::{Circle, Text};
use ocs_plugin_api::host::codec::types::Vector3;
use ocs_plugin_api::dialog::DialogValue;
use ocs_plugin_api::host::{Document as CadDocument, EntityType, Handle, PluginRequestError, PluginRequestSender};
use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};

use teren::body::{zapis_body, Bod, FormatBodu, Oddelovac};
use teren::kontrola::podezrele_body;
use teren::povrch::Hlaseni;
use teren::skupiny::{SeznamCisel, SkupinaBodu, VSECHNY};

use crate::cogo::{self, BodVykresu};
use crate::nastaveni;
use crate::povrchy::{self, Polozka, SkupinaVPovrchu, StatUlozena, TypObjektu, Uprava, ZaznamPovrchu};
use crate::styly::{self, StylBodu, StylPovrchu};
use crate::vykres::{PresKanal, Vykres};

pub const H_KONTROLA: &str = "TER_KONTROLA";

// ── Data pro okno ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct SkupinaPovrchu {
    pub id: String,
    pub jmeno: String,
    pub zapnuto: bool,
    pub pocet: usize,
    /// Skupina ve výkrese neexistuje (byla smazána).
    pub chybi: bool,
}

#[derive(Clone, Debug)]
pub struct PrehledPovrchu {
    pub id: String,
    pub jmeno: String,
    pub aktivni: bool,
    pub zastaraly: bool,
    pub auto: bool,
    /// Id stylu povrchu.
    pub styl: String,
    pub interval: f64,
    pub hlavni: f64,
    pub max_hrana: Option<f64>,
    pub skupiny: Vec<SkupinaPovrchu>,
    pub vyloucene_kody: String,
    pub stat: StatUlozena,
    pub pocet_bodu: usize,
    pub vylouceno_bodu: usize,
    pub polozky: Vec<Polozka>,
    pub upravy: Vec<Uprava>,
    pub hlaseni: Option<Hlaseni>,
}

impl PrehledPovrchu {
    /// Počet upozornění z posledního přepočtu.
    pub fn upozorneni(&self) -> usize {
        self.hlaseni.as_ref().map_or(0, |h| h.krizeni_spojnic.len() + h.nepouzite_editace.len())
            + self.skupiny.iter().filter(|s| s.chybi).count()
    }
}

#[derive(Clone, Debug)]
pub struct PrehledSkupiny {
    pub skupina: SkupinaBodu,
    pub pocet: usize,
    /// Jména povrchů, které skupinu používají.
    pub povrchy: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PodezrelyBod {
    pub handle: Handle,
    pub cislo: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dz: f64,
}

/// Kam má Prospektor skočit (vazba z výkresu do stromu).
#[derive(Clone, Debug, PartialEq)]
pub enum Skok {
    Povrch(String),
    Polozka(String, Handle),
    Body(Vec<Handle>),
    Skupina(String),
    StylBodu(String),
    StylPovrchu(String),
    StylyBodu,
    StylyPovrchu,
}

#[derive(Clone, Debug, Default)]
pub struct StavSpravce {
    pub povrchy: Vec<PrehledPovrchu>,
    pub body: Vec<BodVykresu>,
    pub skupiny: Vec<PrehledSkupiny>,
    /// Soubory, ze kterých byly body importovány.
    pub zdroje: Vec<String>,
    pub dalsi_cislo: u64,
    pub kontrola: Option<Vec<PodezrelyBod>>,
    pub zprava: String,
    pub vykres: String,
    pub slozka: String,
    pub pripojeno: bool,
    pub pracuji: bool,
    pub skok: Option<(u64, Skok)>,
    pub styly_bodu: Vec<StylBodu>,
    /// Bloky výkresu použitelné jako vlastní značka bodu.
    pub bloky: Vec<String>,
    pub styly_povrchu: Vec<StylPovrchu>,
    /// Id stylů uložených ve výkrese (upravené vestavěné a vlastní).
    pub ulozene_styly: Vec<String>,
    /// Měřítko výkresu (jmenovatel).
    pub meritko: f64,
    /// Styl každého bodu (id stylu) a odkud ho bod má.
    pub styl_bodu: HashMap<Handle, (String, styly::Puvod)>,
    /// Co je právě vybrané ve výkrese (body i přes značku/popis, povrchy).
    pub vyber_body: Vec<Handle>,
    pub vyber_povrchy: Vec<String>,
    /// Počet vybraných objektů, které nejsou bod ani povrch.
    pub vyber_jine: usize,
    /// Načtené sítě povrchů pro prohlížeč 3D.
    pub site3d: Arc<Vec<Sit3D>>,
    /// Zvyšuje se při každé změně – okno podle toho překresluje.
    pub verze: u64,
}

/// Síť povrchu pro prohlížeč 3D (souřadnice vůči společnému počátku).
#[derive(Debug, Default, Clone)]
pub struct Sit3D {
    pub id: String,
    pub jmeno: String,
    pub pocatek: (f64, f64),
    pub vrcholy: Vec<[f32; 3]>,
    pub troj: Vec<[u32; 3]>,
    /// Vrcholy, které jsou body zaměření.
    pub body: Vec<u32>,
    /// Obrys povrchu.
    pub obrys: Vec<Vec<[f32; 3]>>,
    /// Povinné hrany (spojnice).
    pub spojnice: Vec<Vec<[f32; 3]>>,
    /// Vrstevnice: (hlavní, body).
    pub vrstevnice: Vec<(bool, Vec<[f32; 3]>)>,
    pub z_min: f64,
    pub z_max: f64,
}

/// Postaví síť povrchu pro prohlížeč 3D.
fn sit3d(doc: &CadDocument, z: &ZaznamPovrchu, pocatek: Option<(f64, f64)>) -> Result<Sit3D, String> {
    let def = povrchy::definice(doc, z).definice;
    let p = teren::povrch::Povrch::z_definice(&def, 0.001).map_err(|e| e.to_string())?;
    let o = pocatek.unwrap_or(p.pocatek);
    let mistni = |x: f64, y: f64, zz: f64| [(x - o.0) as f32, (y - o.1) as f32, zz as f32];
    let vrcholy: Vec<[f32; 3]> = (0..p.vrcholy.len())
        .map(|i| {
            let (x, y, zz) = p.globalni(i);
            mistni(x, y, zz)
        })
        .collect();
    let troj: Vec<[u32; 3]> = p.trojuhelniky.iter().map(|t| t.map(|i| i as u32)).collect();
    let pouzite = p.pouzite_vrcholy();
    let body = (0..p.vrcholy.len()).filter(|&i| p.vrcholy[i].bod.is_some() && pouzite[i]).map(|i| i as u32).collect();
    let obrys: Vec<Vec<[f32; 3]>> = p
        .obrys()
        .into_iter()
        .map(|o| {
            let mut b: Vec<[f32; 3]> = o.into_iter().map(|(x, y, zz)| mistni(x, y, zz)).collect();
            if let Some(&f) = b.first() {
                b.push(f);
            }
            b
        })
        .collect();
    let spojnice: Vec<Vec<[f32; 3]>> = p.povinne_hrany.iter().map(|&(a, b)| vec![vrcholy[a], vrcholy[b]]).collect();
    let st = styly::styl_povrchu(doc, &z.styl);
    let (interval, hlavni) = st.interval.unwrap_or((z.interval, z.hlavni));
    let vrstevnice = teren::vrstevnice::vrstevnice(&p, &teren::vrstevnice::NastaveniVrstevnic { interval, hlavni })
        .into_iter()
        .map(|c| {
            let mut b: Vec<[f32; 3]> = c.body.iter().map(|&(x, y)| mistni(x, y, c.vyska)).collect();
            if c.uzavrena && !b.is_empty() {
                b.push(b[0]);
            }
            (c.hlavni, b)
        })
        .collect();
    let s = p.statistika();
    Ok(Sit3D {
        id: z.id.clone(),
        jmeno: z.jmeno.clone(),
        pocatek: o,
        vrcholy,
        troj,
        body,
        obrys,
        spojnice,
        vrstevnice,
        z_min: s.z_min,
        z_max: s.z_max,
    })
}

/// Nastaví bloky všech povrchů jako „jen pro zobrazení“ (nebo zpět).
pub fn oznac_povrchy(v: &mut dyn Vykres, doc: &CadDocument, ano: bool) -> usize {
    povrchy::povrchy(doc).into_iter().filter(|z| v.jen_zobrazeni(z.insert, ano)).count()
}

/// Po akci, která objekty přestavěla (nový blok povrchu, nové popisy bodů),
/// znovu vybere tytéž body a povrchy – výběr ve výkrese i v Prospektoru zůstane.
pub fn obnov_vyber(v: &mut dyn Vykres, body: &[Handle], povrchy_id: &[String]) {
    if body.is_empty() && povrchy_id.is_empty() {
        return;
    }
    let Ok(doc) = v.dokument() else { return };
    let mut h: Vec<Handle> = cogo::body(&doc).into_iter().map(|b| b.handle).filter(|x| body.contains(x)).collect();
    h.extend(povrchy::povrchy(&doc).into_iter().filter(|z| povrchy_id.contains(&z.id)).map(|z| z.insert));
    if !h.is_empty() {
        v.vyber(h);
    }
}

#[derive(Clone, Debug)]
pub enum Zmena {
    Jmeno(String),
    Styl(String),
    Interval(f64, f64),
    Hrana(Option<f64>),
    Kody(String),
    Auto(bool),
}

#[derive(Clone, Debug)]
pub enum Akce {
    Obnovit,
    Aktivovat(String),
    Nastav(Vec<String>, Zmena),
    Prepocitat(Vec<String>),
    Smazat(String),
    OdebratPolozku(String, Handle),
    PolozkaZapnout(String, Handle, bool),
    UpravaZapnout(String, usize, bool),
    UpravaSmazat(String, usize),
    UpravySmazat(String),
    /// Skupina bodů v povrchu: `None` = odebrat, `Some(zapnuto)` = přidat / zapnout / vypnout.
    SkupinaVPovrchu(String, String, Option<bool>),
    /// Přidat objekty vybrané ve výkrese do definice povrchu.
    PridatObjekty(String, TypObjektu, bool),
    /// Přiblížit na místo (střed, poloměr) a vybrat objekty.
    Ukazat([f64; 2], f64, Vec<Handle>),
    Kontrola,
    /// Odebrat bod zaměření z aktivního povrchu (úprava „odebrat bod“).
    VyloucitBod(f64, f64),
    // body
    BodUpravit(Handle, Bod),
    BodySmazat(Vec<Handle>),
    /// Ručně odsunuté popisy vybraných bodů vrátit na výchozí místo.
    PopisyNaMisto(Vec<Handle>),
    /// Režim posunu bodů úchopem: zapnout / vypnout (None = přepnout).
    Uchopy(Option<bool>),
    /// Přiblížit a vybrat body.
    UkazatBody(Vec<Handle>),
    /// Z panelu Bod Terénu: body, číslo, výška, kód, styl (jen změněné).
    BodyPanel(Vec<Handle>, Option<String>, Option<f64>, Option<String>, Option<String>),
    /// Jen zpráva do příkazového řádku.
    Hlaseni(String),
    /// Hromadně: výška (hodnota, přičíst), kód.
    BodyHromadne(Vec<Handle>, Option<(f64, bool)>, Option<String>),
    BodyPrecislovat(Vec<Handle>, u64, u64),
    ObnovitPopisy,
    /// Vlastní styl vybraných bodů („“ = podle skupiny).
    StylBodu(Vec<Handle>, String),
    UlozStylBodu(StylBodu),
    UlozStylPovrchu(StylPovrchu),
    /// Smaže uložený styl (vestavěný se vrátí k původnímu nastavení).
    SmazStyl(String),
    SablonaUloz,
    SablonaNacti,
    /// Import bodů z okna náhledu (formát, duplicity, skupina, styl…).
    Import(Box<crate::import::NastaveniImportu>),
    /// Převést body nakreslené ve výkrese pro náhled importu:
    /// (hladina – `None` = všechny, jen výběr, tolerance párování textů; 0 = auto).
    BodyZVykresu(Option<String>, bool, f64),
    /// Měřítko výkresu (jmenovatel).
    Meritko(u32),
    /// Nový povrch jako tlačítko na pásu: (základ id, jméno, z bodů zaměření / z vybraných čar).
    NovyPovrchPas(String, String, bool),
    /// Připraví výkres ze šablony: hladiny, PDMODE, měřítko a styly.
    PripravVykres,
    SmazatKontrolu,
    /// Srovná značky a popisy s body (po přesunu, smazání, kopírování).
    SrovnatBody(bool),
    /// Načte sítě povrchů pro prohlížeč 3D.
    Nacti3D(Vec<String>),
    /// Povrchy nejdou / jdou vybrat kliknutím.
    PovrchyNevybirat(bool),
    /// Převzít výběr z výkresu (body → tabulka, objekt povrchu → strom).
    VyberZVykresu,
    // skupiny
    SkupinaUlozit(SkupinaBodu),
    SkupinaNova(Vec<Handle>),
    SkupinaSmazat(String),
    SkupinaBody(String, Vec<Handle>, bool),
    SkupinaVyberVykresu(String, bool),
    PovrchZeSkupiny(String, Option<String>, bool),
    NovyPovrch(NovyPovrch),
    /// Export povrchu do souboru: (id, cesta, LandXML = true / DXF 3D plochy = false).
    ExportPovrchu(String, String, bool),
    Export(String, String, String, Oddelovac, bool),
    /// Akce, která počká, až se ve výkrese objeví skupina (po dokončení příkazu).
    Odlozena(Box<Akce>, Podminka),
}

/// Nový povrch z průvodce.
#[derive(Clone, Debug)]
pub struct NovyPovrch {
    pub jmeno: String,
    pub skupiny: Vec<String>,
    pub vyloucene_kody: Vec<String>,
    pub max_hrana: Option<f64>,
    pub styl: String,
    pub interval: (f64, f64),
    pub auto: bool,
}

/// Na co odložená akce čeká.
#[derive(Clone, Debug, PartialEq)]
pub enum Podminka {
    /// Až skončí příkaz a ve výkrese bude skupina (povrch z naklikaných bodů).
    Skupina(String),
    /// Až budou ve výkrese značky úprav povrchu – i během příkazu (okamžité úpravy).
    ZnackyPovrchu(String),
}

enum Udalost {
    Akce(Akce),
    Zmena(u64),
    Prikaz(u64, bool),
    Vyber(u64, Vec<Handle>),
    Otevreno(u64),
}

// ── Sdílený stav ──────────────────────────────────────────────────────────────

struct Sdilene {
    stav: Mutex<StavSpravce>,
    kanaly: Mutex<HashMap<u64, Arc<dyn PluginRequestSender>>>,
    aktivni_tab: Mutex<Option<u64>>,
    tx: Mutex<Option<Sender<Udalost>>>,
    okno_otevreno: AtomicBool,
    sledovat_vyber: AtomicBool,
}

fn sdilene() -> &'static Sdilene {
    static S: OnceLock<Sdilene> = OnceLock::new();
    S.get_or_init(|| Sdilene {
        stav: Mutex::new(StavSpravce::default()),
        kanaly: Mutex::new(HashMap::new()),
        aktivni_tab: Mutex::new(None),
        tx: Mutex::new(None),
        okno_otevreno: AtomicBool::new(false),
        sledovat_vyber: AtomicBool::new(nastaveni::akt().sledovat_vyber),
    })
}

fn zamek<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn stav() -> StavSpravce {
    zamek(&sdilene().stav).clone()
}

/// Má Prospektor skákat na objekty vybrané ve výkrese?
pub fn sledovat_vyber() -> bool {
    sdilene().sledovat_vyber.load(Ordering::SeqCst)
}

pub fn nastav_sledovani(a: bool) {
    sdilene().sledovat_vyber.store(a, Ordering::SeqCst);
}

/// Oznámení o změně výběru ve výkrese.
pub fn vyber_zmenen(tab: u64, handles: Vec<Handle>) {
    // posílá se i se zavřeným Prospektorem – bod se vybírá celý (bod, značka, popisy)
    spust_vlakno();
    if let Some(tx) = zamek(&sdilene().tx).as_ref() {
        let _ = tx.send(Udalost::Vyber(tab, handles));
    }
}

pub fn nastav_okno_otevreno(o: bool) {
    sdilene().okno_otevreno.store(o, Ordering::SeqCst);
}

fn uprav_stav(f: impl FnOnce(&mut StavSpravce)) {
    {
        let mut s = zamek(&sdilene().stav);
        f(&mut s);
        s.verze += 1;
    }
    crate::okno::prekresli();
}

/// Prospektor má skočit na položku (volá se z příkazů ve výkrese).
pub fn skoc(s: Skok) {
    uprav_stav(|st| {
        let n = st.skok.as_ref().map_or(1, |(n, _)| n + 1);
        st.skok = Some((n, s));
    });
}

struct SdilenyKanal(Arc<dyn PluginRequestSender>);
impl PluginRequestSender for SdilenyKanal {
    fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError> {
        self.0.request(req)
    }
}

/// Zaregistruje kanál výkresu (volá se při každém příkazu pluginu).
pub fn registruj_kanal(tab: u64, s: Box<dyn PluginRequestSender>) {
    zamek(&sdilene().kanaly).insert(tab, Arc::from(s));
    *zamek(&sdilene().aktivni_tab) = Some(tab);
    spust_vlakno();
}

fn kanal(tab: u64) -> Option<PresKanal> {
    // kanál výkresu se registruje při příkazu pluginu; výkres otevřený až po
    // načtení pluginu ho ještě nemá – požadavky jdou stejně přes jeden kanál
    let k = zamek(&sdilene().kanaly);
    k.get(&tab).or_else(|| k.values().next()).map(|a| PresKanal(Box::new(SdilenyKanal(a.clone()))))
}

/// Otevře dialog Open CAD Studia pro běžící interaktivní příkaz; odpověď přijde
/// příkazu (`on_dialog_response`). Posílá se z vlastního vlákna se zpožděním,
/// aby host nejdřív dostal odpověď na právě zpracovávanou událost příkazu.
pub fn ukaz_dialog(def: ocs_plugin_api::dialog::DialogDef) {
    let tab = *zamek(&sdilene().aktivni_tab);
    let Some(k) = kanal(tab.unwrap_or(0)) else { return };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        let _ = k.0.request(PluginRequest::ShowDialog(def));
    });
}

pub fn posli(a: Akce) {
    spust_vlakno();
    if let Some(tx) = zamek(&sdilene().tx).as_ref() {
        let _ = tx.send(Udalost::Akce(a));
    }
}

/// Oznámení od Open CAD Studia (volá `on_notification`).
pub fn zmena_vykresu(tab: u64) {
    if let Some(tx) = zamek(&sdilene().tx).as_ref() {
        let _ = tx.send(Udalost::Zmena(tab));
    }
}

/// Výkres byl otevřen – označení „jen pro zobrazení“ se v souboru neukládá.
pub fn dokument_otevren(tab: u64) {
    if let Some(tx) = zamek(&sdilene().tx).as_ref() {
        let _ = tx.send(Udalost::Otevreno(tab));
    }
}

pub fn stav_prikazu(tab: u64, bezi: bool) {
    crate::vykres::BEZ_SKUPIN.store(bezi, Ordering::SeqCst);
    if let Some(tx) = zamek(&sdilene().tx).as_ref() {
        let _ = tx.send(Udalost::Prikaz(tab, bezi));
    }
}

fn spust_vlakno() {
    let mut tx = zamek(&sdilene().tx);
    if tx.is_some() {
        return;
    }
    let (s, r) = mpsc::channel();
    *tx = Some(s);
    std::thread::Builder::new()
        .name("teren-spravce".into())
        .spawn(move || Pracovnik::default().bez(r))
        .ok();
}

// ── Pracovní vlákno ─────────────────────────────────────────────────────────

struct Odlozena {
    od: Instant,
    tab: u64,
    akce: Akce,
    podminka: Podminka,
}

#[derive(Default)]
struct Pracovnik {
    /// Běží v kartě výkresu nějaký příkaz?
    prikaz_bezi: HashMap<u64, bool>,
    /// Karty se změnou, která čeká na vyhodnocení (čas poslední změny).
    cekajici: HashMap<u64, Instant>,
    /// Má karta povrch s automatickým přepočtem?
    ma_auto: HashMap<u64, bool>,
    /// Naposledy viděné měřítko poznámek výkresu (podle karty).
    meritka: HashMap<u64, String>,
    odlozene: Vec<Odlozena>,
    /// Karty, ve kterých jsou povrchy označené „jen pro zobrazení“.
    oznacene: std::collections::HashSet<u64>,
    /// Kdy se naposledy změnil stav příkazu (pojistka, kdyby konec nepřišel).
    prikaz_od: HashMap<u64, Instant>,
    /// Kdy skončila poslední akce z Prospektoru (krátce po ní se prázdný výběr ignoruje).
    posledni_akce: Option<Instant>,
    /// Poslední výběr ve výkrese čekající na vyhodnocení.
    vyber: Option<(u64, Vec<Handle>, Instant)>,
    posledni_vyber: Vec<Handle>,
    /// Otisky nesouladů grafiky bodů, které už byly opraveny.
    opravene: HashMap<u64, (std::collections::HashSet<u64>, std::collections::HashSet<u64>)>,
    /// Už provedené posuny bodů podle grafiky (handle, dx, dy v mm) – kvůli „Zpět“.
    posunute: HashMap<u64, std::collections::HashSet<(u64, i64, i64)>>,
    /// Otisk naposledy ohlášených úchopů bodů (aby se neposílaly zbytečně).
    uchopy_hash: Option<u64>,
}

/// Kurzorové menu Terénu – ohlásí se, jakmile to Open CAD Studio přijme.
fn registruj_quad(k: &mut PresKanal) {
    if !crate::QUAD_HOTOVO.load(Ordering::SeqCst) && k.quad(crate::quad_panel()) {
        crate::QUAD_HOTOVO.store(true, Ordering::SeqCst);
    }
}

/// Ohlásí úchopy bodů (režim posunu bodů úchopem); vrací jejich počet.
fn obnov_uchopy(k: &mut PresKanal, posledni: &mut Option<u64>) -> usize {
    use std::hash::{Hash, Hasher};
    let Ok(doc) = k.dokument() else { return 0 };
    let (g, navic) = crate::uchopy::uchopy(&doc);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for x in &g {
        x.grip_id.hash(&mut h);
        x.point.iter().for_each(|c| c.to_bits().hash(&mut h));
    }
    let otisk = h.finish();
    if *posledni != Some(otisk) {
        *posledni = Some(otisk);
        k.uchopy(g.clone());
        if navic > 0 {
            k.info(&trf!("Terén: úchopy jen pro prvních {} bodů (dalších {} bez úchopu).", crate::uchopy::MAX_UCHOPU, navic));
        }
    }
    g.len()
}

const KLID: Duration = Duration::from_millis(600);

type Vysledek = Result<Option<String>, String>;

impl Pracovnik {
    fn bez(mut self, rx: Receiver<Udalost>) {
        loop {
            let cekat = if self.odlozene.is_empty() { 250 } else { 40 };
            match rx.recv_timeout(Duration::from_millis(cekat)) {
                Ok(Udalost::Akce(Akce::Odlozena(a, podminka))) => {
                    if let Some(tab) = Self::aktivni_tab() {
                        if !self.odlozene.iter().any(|o| o.tab == tab && o.podminka == podminka) {
                            self.odlozene.push(Odlozena { od: Instant::now(), tab, akce: *a, podminka });
                        }
                    }
                }
                Ok(Udalost::Akce(a)) => self.akce(a),
                Ok(Udalost::Zmena(tab)) => {
                    self.cekajici.insert(tab, Instant::now());
                }
                Ok(Udalost::Otevreno(tab)) => {
                    self.oznacene.remove(&tab);
                    self.cekajici.insert(tab, Instant::now());
                }
                Ok(Udalost::Vyber(tab, h)) => {
                    self.vyber = Some((tab, h, Instant::now()));
                }
                Ok(Udalost::Prikaz(tab, bezi)) => {
                    self.prikaz_bezi.insert(tab, bezi);
                    self.prikaz_od.insert(tab, Instant::now());
                    if !bezi {
                        self.cekajici.entry(tab).or_insert_with(Instant::now);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.vyhodnot_odlozene();
            self.vyhodnot_vyber();
            self.vyhodnot_zmeny();
        }
    }

    fn aktivni_tab() -> Option<u64> {
        *zamek(&sdilene().aktivni_tab)
    }

    /// Běží v kartě příkaz? Pojistka: když Open CAD Studio konec příkazu
    /// neohlásí, po 30 s se bere, že už neběží.
    fn bezi(&self, tab: u64) -> bool {
        self.prikaz_bezi.get(&tab).copied().unwrap_or(false)
            && self.prikaz_od.get(&tab).is_none_or(|t| t.elapsed() < Duration::from_secs(30))
    }

    /// Odložené akce (např. povrch z naklikaných bodů) – provedou se, až
    /// příkaz skončí a ve výkrese je skupina, na kterou čekají.
    fn vyhodnot_odlozene(&mut self) {
        if self.odlozene.is_empty() {
            return;
        }
        let ted = Instant::now();
        let mut zbyle = Vec::new();
        for o in std::mem::take(&mut self.odlozene) {
            let (limit, ceka_na_prikaz) = match o.podminka {
                Podminka::Skupina(_) => (Duration::from_secs(120), true),
                Podminka::ZnackyPovrchu(_) => (Duration::from_secs(15), false),
            };
            if ted.duration_since(o.od) > limit {
                continue;
            }
            if ted.duration_since(o.od) < Duration::from_millis(80) || (ceka_na_prikaz && self.bezi(o.tab)) {
                zbyle.push(o);
                continue;
            }
            let Some(mut k) = kanal(o.tab) else { continue };
            let Ok(d) = k.dokument() else {
                zbyle.push(o);
                continue;
            };
            match &o.podminka {
                Podminka::Skupina(id) if cogo::najdi_skupinu(&d, id).is_some() => self.akce(o.akce),
                // úprava povrchu za běhu příkazu: rychlý přepočet z téhož snímku výkresu
                Podminka::ZnackyPovrchu(id) if !povrchy::znacky_uprav(&d, id).is_empty() => {
                    let id = id.clone();
                    self.rychly_prepocet(&mut k, d, &id);
                }
                _ => zbyle.push(o),
            }
        }
        self.odlozene = zbyle;
    }

    /// Přepočet povrchu po úpravě (otočení, smazání hrany…) – co nejméně
    /// požadavků na Open CAD Studio, aby se úprava projevila hned.
    fn rychly_prepocet(&mut self, k: &mut PresKanal, doc: CadDocument, id: &str) {
        let Some(mut z) = povrchy::povrchy(&doc).into_iter().find(|z| z.id == id) else { return };
        k.zpet_samostatne(tr("Úprava povrchu"));
        match povrchy::prestav_s(k, &mut z, Some(doc)) {
            Ok(_) => k.hotovo(),
            Err(e) => k.chyba(&trf!("Terén: {}", e)),
        }
        if sdilene().okno_otevreno.load(Ordering::SeqCst) {
            self.obnov(k);
        }
    }

    /// Prospektor sleduje výběr: skočí na bod, povrch nebo položku definice.
    fn vyhodnot_vyber(&mut self) {
        let Some((tab, h, cas)) = self.vyber.take() else { return };
        if cas.elapsed() < Duration::from_millis(300) {
            self.vyber = Some((tab, h, cas));
            return;
        }
        if h == self.posledni_vyber || Self::aktivni_tab() != Some(tab) {
            return;
        }
        if h.is_empty() && self.posledni_akce.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            // výběr zmizel kvůli přestavbě objektu – panel drží dál stejný objekt
            return;
        }
        self.posledni_vyber = h.clone();
        if h.is_empty() {
            uprav_stav(|s| {
                s.vyber_body.clear();
                s.vyber_povrchy.clear();
                s.vyber_jine = 0;
            });
            if let Some(mut k) = kanal(tab) {
                k.panel(crate::panel::hodnoty(&[]));
            }
            return;
        }
        let Some(mut k) = kanal(tab) else { return };
        registruj_quad(&mut k);
        let Ok(doc) = k.dokument() else { return };
        let body = cogo::body_z_vyberu(&doc, &h);
        // panel Bod Terénu
        {
            let vse = cogo::body(&doc);
            let vybrane: Vec<&BodVykresu> = vse.iter().filter(|b| body.contains(&b.handle)).collect();
            k.panel(crate::panel::hodnoty(&vybrane));
        }
        // bod drží pohromadě: klik na značku nebo popis vybere i bod a ostatní popisy
        if !body.is_empty() && !self.bezi(tab) {
            let polohy: Vec<[f64; 2]> =
                cogo::body(&doc).iter().filter(|b| body.contains(&b.handle)).map(|b| [b.bod.x, b.bod.y]).collect();
            let mut cele = h.clone();
            for x in body.iter().copied().chain(cogo::grafika_bodu(&doc, &polohy)) {
                if !cele.contains(&x) {
                    cele.push(x);
                }
            }
            if cele.len() != h.len() {
                k.vyber(cele.clone());
                self.posledni_vyber = cele;
            }
        }
        if !sdilene().okno_otevreno.load(Ordering::SeqCst) {
            return;
        }
        let povrchy = if body.is_empty() { povrchy::povrchy_z_vyberu(&doc, &h) } else { Vec::new() };
        let jine = if body.is_empty() && povrchy.is_empty() { h.len() } else { 0 };
        // stejné objekty jako dosud (např. znovu vybrané po přestavbě povrchu)?
        let stejne = {
            let st = zamek(&sdilene().stav);
            st.vyber_body == body && st.vyber_povrchy == povrchy
        };
        let po_akci = self.posledni_akce.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
        uprav_stav(|s| {
            s.vyber_body = body;
            s.vyber_povrchy = povrchy;
            s.vyber_jine = jine;
        });
        // strom neskáče, když jde jen o znovu vybrání po vlastní akci
        // (jinak by po Uložit styl odskočil z editoru stylu)
        if sledovat_vyber() && !(stejne || po_akci) {
            if let Some((skok, _)) = najdi_ve_strome(&doc, &h) {
                skoc(skok);
            }
        }
    }

    fn vyhodnot_zmeny(&mut self) {
        let ted = Instant::now();
        let hotove: Vec<u64> = self
            .cekajici
            .iter()
            .filter(|(tab, cas)| ted.duration_since(**cas) >= KLID && !self.bezi(**tab))
            .map(|(t, _)| *t)
            .collect();
        for tab in hotove {
            self.cekajici.remove(&tab);
            let okno = sdilene().okno_otevreno.load(Ordering::SeqCst) && Self::aktivni_tab() == Some(tab);
            let Some(mut k) = kanal(tab) else { continue };
            registruj_quad(&mut k);
            self.hlidej_meritko(tab, &mut k);
            self.hlidej_body(tab, &mut k);
            if crate::uchopy::zapnuto() {
                obnov_uchopy(&mut k, &mut self.uchopy_hash);
            }
            if nastaveni::akt().povrch_nevybirat && self.oznacene.insert(tab) {
                if let Ok(doc) = k.dokument() {
                    oznac_povrchy(&mut k, &doc, true);
                }
            }
            if !okno && !self.ma_auto.get(&tab).copied().unwrap_or(true) {
                continue;
            }
            self.auto_prepocet(tab, &mut k);
            if okno {
                self.obnov(&mut k);
            }
        }
    }

    /// Změnil uživatel měřítko ve stavovém řádku? Pak přepíše popisy.
    fn hlidej_meritko(&mut self, tab: u64, k: &mut PresKanal) {
        let Ok(doc) = k.dokument() else { return };
        let ted = doc.header.current_annotation_scale.clone();
        let predtim = self.meritka.insert(tab, ted.clone());
        let zmena = predtim.is_some_and(|p| p != ted);
        if zmena && (!cogo::body(&doc).is_empty() || !povrchy::povrchy(&doc).is_empty()) {
            match po_zmene_meritka(k) {
                Ok(t) => k.info(&trf!("Terén: {}", t)),
                Err(e) => k.chyba(&trf!("Terén: {}", e)),
            }
            k.konec_kroku();
        }
    }

    /// Po přesunu, smazání nebo kopírování bodů srovná jejich značky a popisy.
    fn hlidej_body(&mut self, tab: u64, k: &mut PresKanal) {
        let Ok(doc) = k.dokument() else { return };
        if cogo::presun_nosice(k, &doc) {
            k.konec_kroku();
        }
        let m = nastaveni::meritko(&doc);
        // posunutá značka nebo popis → posunout bod (popis)
        let (posuny, otisk) = cogo::posunuta_grafika(&doc, m, nastaveni::akt().posun_jen_popis);
        if !posuny.is_empty() {
            let videno = self.posunute.entry(tab).or_default();
            if otisk.iter().all(|o| videno.contains(o)) {
                // stejný posun už jednou proveden – nejspíš „Zpět“; nechat být
                return;
            }
            if videno.len() > 20000 {
                videno.clear();
            }
            videno.extend(otisk);
            k.zpet(tr("Posun bodu"));
            match cogo::proved_posuny(k, &doc, &posuny) {
                Ok(_) => k.hotovo(),
                Err(e) => k.chyba(&trf!("Terén: posun bodu: {}", e)),
            }
            k.konec_kroku();
            return;
        }
        let n = cogo::nesoulad(&doc, m);
        if n.je_prazdny() || n.navic.len() + n.chybi.len() > 2000 {
            return;
        }
        // stejný (nebo dílčí) nesoulad už jednou opraven – nejspíš „Zpět“
        let videno = self.opravene.entry(tab).or_default();
        let uz = n.navic.iter().all(|h| videno.0.contains(&h.value())) && n.chybi.iter().all(|b| videno.1.contains(&b.handle.value()));
        if uz {
            return;
        }
        if videno.0.len() + videno.1.len() > 20000 {
            *videno = Default::default();
        }
        videno.0.extend(n.navic.iter().map(|h| h.value()));
        videno.1.extend(n.chybi.iter().map(|b| b.handle.value()));
        k.zpet(tr("Popisy bodů"));
        if let Err(e) = cogo::oprav_nesoulad(k, &doc, &n) {
            k.chyba(&trf!("Terén: popisy bodů: {}", e));
        }
        k.hotovo();
        k.konec_kroku();
    }

    /// Přepočítá zastaralé povrchy s automatickým přepočtem.
    fn auto_prepocet(&mut self, tab: u64, k: &mut PresKanal) {
        let Ok(doc) = k.dokument() else { return };
        let vse = povrchy::povrchy(&doc);
        self.ma_auto.insert(tab, vse.iter().any(|z| z.auto));
        for mut z in vse.into_iter().filter(|z| z.auto) {
            if povrchy::zastaraly(&doc, &z) {
                k.zpet(tr("Automatický přepočet povrchu"));
                match povrchy::prestav(k, &mut z) {
                    Ok(r) => {
                        povrchy::vypis(k, &z, &r);
                        k.hotovo();
                    }
                    Err(e) => k.chyba(&trf!("Terén: automatický přepočet „{}“: {}", z.jmeno, e)),
                }
                k.konec_kroku();
            }
        }
    }

    fn obnov(&mut self, k: &mut PresKanal) {
        let doc = match k.dokument() {
            Ok(d) => d,
            Err(e) => {
                uprav_stav(|s| {
                    s.pripojeno = false;
                    s.zprava = trf!("Výkres není dostupný: {}", e);
                });
                return;
            }
        };
        let p = prehled(&doc);
        if let Some(tab) = Self::aktivni_tab() {
            self.ma_auto.insert(tab, p.povrchy.iter().any(|p| p.auto));
        }
        let cesta = doc.source_path.clone();
        let ro = styly::Rozlisovac::new(&doc);
        let styl_bodu: HashMap<Handle, (String, styly::Puvod)> = p
            .body
            .iter()
            .map(|b| {
                let (st, puvod) = ro.urci(&b.bod, &b.zdroj, &b.styl);
                (b.handle, (st.id, puvod))
            })
            .collect();
        uprav_stav(|s| {
            s.styl_bodu = styl_bodu;
            s.povrchy = p.povrchy;
            s.body = p.body;
            s.skupiny = p.skupiny;
            s.zdroje = p.zdroje;
            s.dalsi_cislo = p.dalsi_cislo;
            s.styly_bodu = styly::styly_bodu(&doc);
            s.bloky = cogo::bloky_pro_znacky(&doc);
            s.styly_povrchu = styly::styly_povrchu(&doc);
            s.ulozene_styly = s
                .styly_bodu
                .iter()
                .map(|x| x.id.clone())
                .chain(s.styly_povrchu.iter().map(|x| x.id.clone()))
                .filter(|id| styly::je_ulozeny(&doc, id))
                .collect();
            s.meritko = nastaveni::meritko(&doc);
            s.vykres = cesta.clone().unwrap_or_else(|| tr("nový výkres").into());
            s.slozka = cesta
                .as_deref()
                .and_then(|c| std::path::Path::new(c).parent().map(|p| p.display().to_string()))
                .unwrap_or_default();
            s.pripojeno = true;
        });
    }

    fn akce(&mut self, a: Akce) {
        let Some(tab) = Self::aktivni_tab() else {
            uprav_stav(|s| s.zprava = tr("Spusťte nejdřív libovolný příkaz Terén ve výkrese.").into());
            return;
        };
        let Some(mut k) = kanal(tab) else { return };
        match a {
            Akce::Uchopy(zap) => {
                let zap = zap.unwrap_or(!crate::uchopy::zapnuto());
                crate::uchopy::ZAPNUTO.store(zap, Ordering::SeqCst);
                self.uchopy_hash = None;
                let n = obnov_uchopy(&mut k, &mut self.uchopy_hash);
                k.panel(HashMap::from([("uchopy".to_string(), DialogValue::Bool(zap))]));
                if zap {
                    k.info(&trf!("Terén: posun bodů úchopem zapnutý – táhněte kroužek na bodě (úchopů: {}). Vypnete tlačítkem Posun bodů.", n));
                } else {
                    k.info(tr("Terén: posun bodů úchopem vypnutý."));
                }
                return;
            }
            Akce::Hlaseni(t) => {
                k.chyba(&t);
                return;
            }
            a => self.akce_hlavni(k, a),
        }
    }

    fn akce_hlavni(&mut self, mut k: PresKanal, a: Akce) {
        uprav_stav(|s| s.pracuji = true);
        let drzet = !matches!(a, Akce::Obnovit | Akce::Nacti3D(_) | Akce::Ukazat(..) | Akce::UkazatBody(_) | Akce::VyberZVykresu | Akce::BodyZVykresu(..));
        let (body, povrchy_id) = {
            let st = zamek(&sdilene().stav);
            (st.vyber_body.clone(), st.vyber_povrchy.clone())
        };
        if drzet {
            // PDMODE se nastavuje příkazem – ten by krok Zpět ukončil, proto
            // ještě před jeho otevřením (jen jednou, když je potřeba)
            if k.promenna("PDMODE").is_some_and(|m| (m - 1.0).abs() > 1e-9) {
                k.prikaz("PDMODE 1");
            }
            k.zacatek_kroku(nazev_kroku(&a));
        }
        let vysledek = Self::proved(&mut k, a);
        if drzet {
            k.konec_kroku();
        }
        if let Err(e) = &vysledek {
            k.chyba(&trf!("Terén: {}", e));
        }
        if drzet {
            self.posledni_akce = Some(Instant::now());
            if k.vybrane().is_empty() {
                obnov_vyber(&mut k, &body, &povrchy_id);
            }
        }
        self.obnov(&mut k);
        uprav_stav(|s| {
            s.pracuji = false;
            match vysledek {
                Ok(Some(z)) => s.zprava = z,
                Ok(None) => {}
                Err(e) => s.zprava = trf!("Chyba: {}", e),
            }
        });
    }

    fn zaznam(k: &mut dyn Vykres, id: &str) -> Result<(ZaznamPovrchu, CadDocument), String> {
        let doc = k.dokument()?;
        let z = povrchy::povrchy(&doc).into_iter().find(|z| z.id == id).ok_or(tr("povrch už ve výkrese není"))?;
        Ok((z, doc))
    }

    fn prestav(k: &mut dyn Vykres, z: &mut ZaznamPovrchu, popis: &str) -> Vysledek {
        k.zpet(popis);
        let r = povrchy::prestav(k, z)?;
        povrchy::vypis(k, z, &r);
        k.hotovo();
        Ok(Some(trf!("„{}“ přepočítán ({} trojúhelníků)", z.jmeno, r.povrch.statistika().pocet_trojuhelniku)))
    }

    fn proved(k: &mut dyn Vykres, a: Akce) -> Vysledek {
        match a {
            Akce::Obnovit | Akce::Odlozena(..) => Ok(None),
            Akce::Aktivovat(id) => {
                let (z, _) = Self::zaznam(k, &id)?;
                povrchy::nastav_aktivni(&id);
                Ok(Some(trf!("Aktivní povrch: „{}“", z.jmeno)))
            }
            Akce::Nastav(idy, zmena) => {
                let mut zpravy = Vec::new();
                for id in &idy {
                    let (mut z, _) = Self::zaznam(k, id)?;
                    let prepocitat = match zmena.clone() {
                        Zmena::Jmeno(j) => {
                            let j = j.trim().to_string();
                            if j.is_empty() {
                                return Err(tr("jméno nesmí být prázdné").into());
                            }
                            z.jmeno = j;
                            false
                        }
                        Zmena::Auto(a) => {
                            z.auto = a;
                            false
                        }
                        Zmena::Styl(s) => {
                            z.styl = s;
                            true
                        }
                        Zmena::Interval(i, h) => {
                            z.interval = i;
                            z.hlavni = h;
                            true
                        }
                        Zmena::Hrana(h) => {
                            z.max_hrana = h;
                            true
                        }
                        Zmena::Kody(t) => {
                            z.vyloucene_kody = teren::skupiny::kody_z_textu(&t);
                            true
                        }
                    };
                    if prepocitat {
                        if let Some(t) = Self::prestav(k, &mut z, tr("Nastavení povrchu"))? {
                            zpravy.push(t);
                        }
                    } else {
                        k.zpet(tr("Nastavení povrchu"));
                        if !povrchy::uloz_zaznam(k, &z) {
                            return Err(tr("nastavení se nepodařilo uložit").into());
                        }
                        k.hotovo();
                        zpravy.push(trf!("„{}“ uložen", z.jmeno));
                    }
                }
                Ok(Some(zpravy.join("; ")))
            }
            Akce::Prepocitat(idy) => {
                let mut zpravy = Vec::new();
                for id in &idy {
                    let (mut z, _) = Self::zaznam(k, id)?;
                    if let Some(t) = Self::prestav(k, &mut z, tr("Přepočítat povrch"))? {
                        zpravy.push(t);
                    }
                }
                Ok(Some(zpravy.join("; ")))
            }
            Akce::Smazat(id) => {
                let (z, doc) = Self::zaznam(k, &id)?;
                k.zpet(tr("Smazat povrch"));
                povrchy::smaz(k, &doc, &z)?;
                k.hotovo();
                Ok(Some(trf!("Povrch „{}“ smazán", z.jmeno)))
            }
            Akce::OdebratPolozku(id, h) => {
                let (mut z, doc) = Self::zaznam(k, &id)?;
                k.zpet(tr("Odebrat z povrchu"));
                if let Some(e) = povrchy::odeber_objekt(&doc, h, &id) {
                    k.transakce(tr("Odebrat objekt z povrchu"), vec![e])?;
                } else {
                    let e = doc.model_space_entities().find(|e| e.common().handle == h).ok_or(tr("čára už ve výkrese není"))?;
                    if !k.uprav(povrchy::uvolni_caru(e.clone())) {
                        return Err(tr("čáru se nepodařilo upravit").into());
                    }
                }
                z.vypnute.retain(|x| *x != h);
                Self::prestav(k, &mut z, tr("Odebrat z povrchu"))
            }
            Akce::PolozkaZapnout(id, h, zap) => {
                let (mut z, _) = Self::zaznam(k, &id)?;
                z.vypnute.retain(|x| *x != h);
                if !zap {
                    z.vypnute.push(h);
                }
                Self::prestav(k, &mut z, if zap { tr("Zapnout položku povrchu") } else { tr("Vypnout položku povrchu") })
            }
            Akce::UpravaZapnout(id, i, zap) => {
                let (mut z, doc) = Self::zaznam(k, &id)?;
                Self::prevezmi_znacky(k, &doc, &mut z);
                let u = z.upravy.get_mut(i).ok_or(tr("úprava neexistuje"))?;
                u.zapnuto = zap;
                Self::prestav(k, &mut z, tr("Úprava povrchu"))
            }
            Akce::UpravaSmazat(id, i) => {
                let (mut z, doc) = Self::zaznam(k, &id)?;
                Self::prevezmi_znacky(k, &doc, &mut z);
                if i >= z.upravy.len() {
                    return Err(tr("úprava neexistuje").into());
                }
                z.upravy.remove(i);
                Self::prestav(k, &mut z, tr("Smazat úpravu povrchu"))
            }
            Akce::UpravySmazat(id) => {
                let (mut z, doc) = Self::zaznam(k, &id)?;
                Self::prevezmi_znacky(k, &doc, &mut z);
                z.upravy.clear();
                Self::prestav(k, &mut z, tr("Zrušit úpravy povrchu"))
            }
            Akce::SkupinaVPovrchu(id, skupina, stav) => {
                let (mut z, _) = Self::zaznam(k, &id)?;
                match stav {
                    None => z.skupiny.retain(|g| !g.id.eq_ignore_ascii_case(&skupina)),
                    Some(zap) => match z.skupiny.iter_mut().find(|g| g.id.eq_ignore_ascii_case(&skupina)) {
                        Some(g) => g.zapnuto = zap,
                        None => z.skupiny.push(SkupinaVPovrchu { id: skupina, zapnuto: zap }),
                    },
                }
                Self::prestav(k, &mut z, tr("Skupiny bodů povrchu"))
            }
            Akce::PridatObjekty(id, typ, hrany) => {
                let (mut z, doc) = Self::zaznam(k, &id)?;
                let vyber = k.vybrane();
                if vyber.is_empty() {
                    return Err(trf!("nejdřív ve výkrese vyberte {} pro povrch „{}“", typ.mnozne(), z.jmeno));
                }
                let p = povrchy::pridej_objekty(&doc, &vyber, &id, typ, hrany);
                let mut info = trf!("přidáno {} ({})", p.pridano, typ.mnozne());
                if p.uz_bylo > 0 {
                    info.push_str(&trf!(", {} už v povrchu bylo", p.uz_bylo));
                }
                if p.jineho_typu > 0 {
                    info.push_str(&trf!(", {} vybraných objektů je jiného typu", p.jineho_typu));
                }
                if p.obsazene > 0 {
                    info.push_str(&trf!(", {} objektů patří k povrchu jinak (spojnice…)", p.obsazene));
                }
                if p.pridano == 0 {
                    return Ok(Some(trf!("Objekty výkresu: {}", info)));
                }
                povrchy::zajisti_hladinu(k, &doc, &z.hladiny().povrch, 7);
                k.transakce(tr("Přidat objekty do povrchu"), p.upravit)?;
                let r = Self::prestav(k, &mut z, tr("Přepočítat povrch"))?;
                Ok(Some(trf!("Objekty výkresu: {}. {}", info, r.unwrap_or_default())))
            }
            Akce::UkazatBody(h) => {
                let doc = k.dokument()?;
                let body: Vec<_> = cogo::body(&doc).into_iter().filter(|b| h.contains(&b.handle)).collect();
                if body.is_empty() {
                    return Ok(None);
                }
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                for b in &body {
                    x0 = x0.min(b.bod.x);
                    x1 = x1.max(b.bod.x);
                    y0 = y0.min(b.bod.y);
                    y1 = y1.max(b.bod.y);
                }
                let r = ((x1 - x0).max(y1 - y0) / 2.0).max(5.0) * 1.4;
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let _ = k.prikaz(&format!("ZOOM W {},{} {},{}", cx - r, cy - r, cx + r, cy + r));
                Ok(None)
            }
            Akce::BodyPanel(h, cislo, z, kod, styl) => {
                let doc = k.dokument()?;
                let zmeny: Vec<cogo::ZmenaBodu> = cogo::body(&doc)
                    .into_iter()
                    .filter(|b| h.contains(&b.handle))
                    .map(|bv| {
                        let mut b = bv.bod.clone();
                        if let Some(c) = &cislo {
                            b.cislo = c.clone();
                        }
                        if let Some(z) = z {
                            b.z = z;
                        }
                        if let Some(kod) = &kod {
                            b.popis = kod.clone();
                        }
                        let st = styl.clone().unwrap_or_else(|| bv.styl.clone());
                        (bv, b, st)
                    })
                    .collect();
                let n = cogo::uprav_body(k, &doc, &zmeny, tr("Upravit body"))?;
                k.hotovo();
                Ok(Some(trf!("Upraveno bodů: {}", n)))
            }
            Akce::Uchopy(_) | Akce::Hlaseni(_) => Ok(None),
            Akce::Ukazat(stred, r, handly) => {
                let r = (r * 1.4).max(15.0);
                let _ = k.prikaz(&format!(
                    "ZOOM W {},{} {},{}",
                    stred[0] - r,
                    stred[1] - r,
                    stred[0] + r,
                    stred[1] + r
                ));
                if !handly.is_empty() {
                    k.vyber(handly);
                }
                Ok(None)
            }
            Akce::Kontrola => kontrola(k).map(Some),
            Akce::VyloucitBod(x, y) => {
                let doc = k.dokument()?;
                let mut z = povrchy::aktivni(&doc)?;
                Self::prevezmi_znacky(k, &doc, &mut z);
                z.upravy.push(Uprava { e: teren::povrch::Editace::OdebratBod([x, y]), zapnuto: true });
                Self::prestav(k, &mut z, tr("Odebrat bod z povrchu"))
            }

            // ── body ──
            Akce::BodUpravit(h, novy) => {
                let doc = k.dokument()?;
                let vse = cogo::body(&doc);
                let bv = vse.iter().find(|b| b.handle == h).ok_or(tr("bod už ve výkrese není"))?.clone();
                let mut novy = novy;
                if !novy.x.is_finite() || !novy.y.is_finite() {
                    novy.x = bv.bod.x;
                    novy.y = bv.bod.y;
                }
                let mut zprava = trf!("Bod {} upraven", novy.cislo);
                if novy.cislo != bv.bod.cislo
                    && !novy.cislo.is_empty()
                    && vse.iter().any(|b| b.handle != h && b.bod.cislo.eq_ignore_ascii_case(&novy.cislo))
                {
                    zprava.push_str(tr(" – pozor, číslo už má jiný bod"));
                }
                let styl = bv.styl.clone();
                cogo::uprav_body(k, &doc, &[(bv, novy, styl)], tr("Upravit bod"))?;
                k.hotovo();
                Ok(Some(zprava))
            }
            Akce::BodyHromadne(h, vyska, kod) => {
                let doc = k.dokument()?;
                let zmeny: Vec<cogo::ZmenaBodu> = cogo::body(&doc)
                    .into_iter()
                    .filter(|b| h.contains(&b.handle))
                    .map(|bv| {
                        let mut b = bv.bod.clone();
                        match vyska {
                            Some((z, true)) => b.z += z,
                            Some((z, false)) => b.z = z,
                            None => {}
                        }
                        if let Some(kod) = &kod {
                            b.popis = kod.clone();
                        }
                        let styl = bv.styl.clone();
                        (bv, b, styl)
                    })
                    .collect();
                let n = cogo::uprav_body(k, &doc, &zmeny, tr("Upravit body"))?;
                k.hotovo();
                Ok(Some(trf!("Upraveno bodů: {}", n)))
            }
            Akce::PopisyNaMisto(h) => {
                let doc = k.dokument()?;
                let zmeny: Vec<cogo::ZmenaBodu> = cogo::body(&doc)
                    .into_iter()
                    .filter(|b| h.contains(&b.handle) && b.posun.is_some())
                    .map(|mut bv| {
                        bv.posun = None;
                        let (b, styl) = (bv.bod.clone(), bv.styl.clone());
                        (bv, b, styl)
                    })
                    .collect();
                let n = cogo::uprav_body(k, &doc, &zmeny, tr("Popisy na místo"))?;
                k.hotovo();
                Ok(Some(trf!("Popisy vráceny na místo: {}", n)))
            }
            Akce::BodySmazat(h) => {
                let doc = k.dokument()?;
                let body: Vec<BodVykresu> = cogo::body(&doc).into_iter().filter(|b| h.contains(&b.handle)).collect();
                k.zpet(tr("Smazat body"));
                let n = cogo::smaz_body(k, &doc, &body);
                k.hotovo();
                Ok(Some(trf!("Smazáno bodů: {}", n)))
            }
            Akce::BodyPrecislovat(h, od, krok) => {
                let doc = k.dokument()?;
                let vse = cogo::body(&doc);
                let mut zmeny = Vec::new();
                for (i, handle) in h.iter().enumerate() {
                    if let Some(bv) = vse.iter().find(|b| b.handle == *handle) {
                        let mut b = bv.bod.clone();
                        b.cislo = (od + i as u64 * krok.max(1)).to_string();
                        zmeny.push((bv.clone(), b, bv.styl.clone()));
                    }
                }
                let nova: Vec<&str> = zmeny.iter().map(|(_, b, _)| b.cislo.as_str()).collect();
                let kolize = vse.iter().filter(|b| !h.contains(&b.handle) && nova.contains(&b.bod.cislo.as_str())).count();
                cogo::uprav_body(k, &doc, &zmeny, tr("Přečíslovat body"))?;
                k.hotovo();
                let mut t = trf!("Přečíslováno bodů: {} (od {}, krok {})", zmeny.len(), od, krok);
                if kolize > 0 {
                    t.push_str(&trf!(" – pozor, {} jiných bodů má stejné číslo", kolize));
                }
                Ok(Some(t))
            }
            Akce::ObnovitPopisy => {
                let doc = k.dokument()?;
                k.zpet(tr("Obnovit popisy bodů"));
                let n = cogo::obnov_grafiku(k, &doc)?;
                k.hotovo();
                Ok(Some(trf!("Popisy {} bodů vytvořeny znovu", n)))
            }
            Akce::VyberZVykresu => {
                let vyber = k.vybrane();
                if vyber.is_empty() {
                    return Ok(Some(tr("Ve výkrese není nic vybráno").into()));
                }
                let doc = k.dokument()?;
                Ok(Some(match najdi_ve_strome(&doc, &vyber) {
                    Some((s, t)) => {
                        skoc(s);
                        t
                    }
                    None => tr("Vybrané objekty nepatří k žádnému bodu ani povrchu").into(),
                }))
            }

            // ── styly ──
            Akce::StylBodu(h, styl) => {
                let doc = k.dokument()?;
                let zmeny: Vec<cogo::ZmenaBodu> = cogo::body(&doc)
                    .into_iter()
                    .filter(|b| h.contains(&b.handle))
                    .map(|b| {
                        let novy = b.bod.clone();
                        (b, novy, styl.clone())
                    })
                    .collect();
                let n = cogo::uprav_body(k, &doc, &zmeny, tr("Styl bodů"))?;
                k.hotovo();
                let jmeno = if styl.is_empty() {
                    tr("podle skupiny").to_string()
                } else {
                    styly::styly_bodu(&doc).into_iter().find(|s| s.id == styl).map_or(styl.clone(), |s| format!("„{}“", s.jmeno))
                };
                Ok(Some(trf!("Styl {} nastaven {} bodům", jmeno, n)))
            }
            Akce::UlozStylBodu(mut st) => {
                let doc = k.dokument()?;
                st.jmeno = st.jmeno.trim().to_string();
                if st.jmeno.is_empty() {
                    return Err(tr("jméno stylu nesmí být prázdné").into());
                }
                if st.id.is_empty() {
                    st.id = styly::volne_id(&doc, &st.jmeno);
                }
                k.zpet(tr("Styl bodů"));
                styly::uloz_styl_bodu(k, &doc, &st)?;
                let doc = k.dokument()?;
                let n = cogo::obnov_grafiku(k, &doc)?;
                k.hotovo();
                skoc(Skok::StylBodu(st.id.clone()));
                Ok(Some(trf!("Styl „{}“ uložen, překresleno {} bodů", st.jmeno, n)))
            }
            Akce::UlozStylPovrchu(mut st) => {
                let doc = k.dokument()?;
                st.jmeno = st.jmeno.trim().to_string();
                if st.jmeno.is_empty() {
                    return Err(tr("jméno stylu nesmí být prázdné").into());
                }
                if st.id.is_empty() {
                    st.id = styly::volne_id(&doc, &st.jmeno);
                }
                if let Some((i, h)) = st.interval {
                    if !(i > 0.0 && h >= i) {
                        return Err(tr("krok vrstevnic musí být kladný a hlavní krok aspoň jako vedlejší").into());
                    }
                }
                k.zpet(tr("Styl povrchu"));
                styly::uloz_styl_povrchu(k, &doc, &st)?;
                let mut zpravy = vec![trf!("Styl „{}“ uložen", st.jmeno)];
                for mut z in povrchy::povrchy(&doc).into_iter().filter(|z| z.styl.eq_ignore_ascii_case(&st.id)) {
                    if let Some(t) = Self::prestav(k, &mut z, tr("Styl povrchu"))? {
                        zpravy.push(t);
                    }
                }
                k.hotovo();
                skoc(Skok::StylPovrchu(st.id.clone()));
                Ok(Some(zpravy.join("; ")))
            }
            Akce::SmazStyl(id) => {
                let doc = k.dokument()?;
                let vestaveny = StylBodu::vestavene().iter().any(|s| s.id == id) || StylPovrchu::vestavene().iter().any(|s| s.id == id);
                if !vestaveny {
                    let povrch = povrchy::povrchy(&doc).into_iter().find(|z| z.styl.eq_ignore_ascii_case(&id));
                    if let Some(z) = povrch {
                        return Err(trf!("styl používá povrch „{}“", z.jmeno));
                    }
                    if let Some(g) = cogo::ulozene_skupiny(&doc).into_iter().find(|(_, g)| g.styl.eq_ignore_ascii_case(&id)) {
                        return Err(trf!("styl používá skupina „{}“", g.1.jmeno));
                    }
                    if cogo::body(&doc).iter().any(|b| b.styl.eq_ignore_ascii_case(&id)) {
                        return Err(tr("styl mají nastavený některé body").into());
                    }
                }
                k.zpet(tr("Smazat styl"));
                if !styly::smaz_styl(k, &doc, &id) {
                    return Err(tr("styl ve výkrese není uložený").into());
                }
                // překreslit, co styl používalo
                let doc = k.dokument()?;
                cogo::obnov_grafiku(k, &doc)?;
                for mut z in povrchy::povrchy(&doc).into_iter().filter(|z| z.styl.eq_ignore_ascii_case(&id)) {
                    Self::prestav(k, &mut z, tr("Styl povrchu"))?;
                }
                k.hotovo();
                Ok(Some(if vestaveny { tr("Styl vrácen k původnímu nastavení").into() } else { tr("Styl smazán").into() }))
            }
            Akce::SablonaUloz => {
                let doc = k.dokument()?;
                let (cesta, n) = styly::uloz_sablonu(&doc)?;
                Ok(Some(trf!("Šablona uložena ({} stylů): {}", n, cesta)))
            }
            Akce::SablonaNacti => {
                let doc = k.dokument()?;
                k.zpet(tr("Načíst šablonu stylů"));
                let n = styly::nacti_sablonu(k, &doc)?;
                let doc = k.dokument()?;
                cogo::obnov_grafiku(k, &doc)?;
                let mut prepocteno = 0;
                for mut z in povrchy::povrchy(&doc) {
                    if povrchy::zastaraly(&doc, &z) && Self::prestav(k, &mut z, tr("Styl povrchu")).is_ok() {
                        prepocteno += 1;
                    }
                }
                k.hotovo();
                Ok(Some(trf!("Ze šablony načteno {} stylů, přepočteno povrchů: {}", n, prepocteno)))
            }
            Akce::Import(n) => crate::import::proved_import(k, *n).map(Some),
            Akce::BodyZVykresu(hladina, vyber, tol) => crate::import::priprav_z_vykresu(k, hladina.as_deref(), vyber, tol).map(Some),
            Akce::Meritko(m) => {
                crate::zmen_meritko_v(k, m);
                Ok(Some(trf!("Měřítko 1:{}", m)))
            }
            Akce::NovyPovrchPas(zaklad, jmeno, body) => {
                let vyber = k.vybrane();
                crate::novy_povrch_v(k, &vyber, &zaklad, &jmeno, body);
                Ok(None)
            }
            Akce::PripravVykres => {
                k.zpet(tr("Připravit výkres ze šablony"));
                cogo::priprav_vykres(k)?;
                let mut co = vec![tr("hladiny Terénu").to_string(), tr("značka bodu podle stylu (PDMODE 1)").to_string()];
                if let Some(m) = styly::meritko_sablony() {
                    crate::zmen_meritko_v(k, m);
                    co.push(trf!("měřítko 1:{}", m));
                }
                if styly::sablona_existuje() {
                    let doc = k.dokument()?;
                    let n = styly::nacti_sablonu(k, &doc)?;
                    co.push(trf!("{} stylů ze šablony", n));
                    let doc = k.dokument()?;
                    if !cogo::body(&doc).is_empty() {
                        cogo::obnov_grafiku(k, &doc)?;
                    }
                }
                k.hotovo();
                k.info(&trf!("Terén: výkres připraven – {}", co.join(", ")));
                Ok(Some(trf!("Výkres připraven: {}", co.join(", "))))
            }
            Akce::SrovnatBody(jen_zjistit) => {
                let doc = k.dokument()?;
                let n = cogo::nesoulad(&doc, nastaveni::meritko(&doc));
                if n.je_prazdny() {
                    return Ok(Some(trf!("Popisy bodů jsou v pořádku (bodů {})", cogo::body(&doc).len())));
                }
                if jen_zjistit {
                    return Ok(Some(trf!("Nesoulad: grafika navíc {}, body bez grafiky {}", n.navic.len(), n.chybi.len())));
                }
                k.zpet(tr("Popisy bodů"));
                cogo::oprav_nesoulad(k, &doc, &n)?;
                k.hotovo();
                Ok(Some(trf!("Odstraněno {} objektů grafiky, dokresleno {} bodů", n.navic.len(), n.chybi.len())))
            }
            Akce::Nacti3D(idy) => {
                let doc = k.dokument()?;
                let vse = povrchy::povrchy(&doc);
                let mut site = Vec::new();
                let mut pocatek = None;
                for id in &idy {
                    if let Some(z) = vse.iter().find(|z| &z.id == id) {
                        let s = sit3d(&doc, z, pocatek)?;
                        pocatek = Some(s.pocatek);
                        site.push(s);
                    }
                }
                let n: usize = site.iter().map(|s| s.troj.len()).sum();
                uprav_stav(|s| s.site3d = Arc::new(site));
                Ok(Some(trf!("Prohlížeč: {} trojúhelníků", n)))
            }
            Akce::PovrchyNevybirat(ano) => {
                nastaveni::zmen(|n| n.povrch_nevybirat = ano);
                let doc = k.dokument()?;
                let n = oznac_povrchy(k, &doc, ano);
                Ok(Some(if ano {
                    trf!("Povrchy ({}) nejdou vybrat kliknutím – klik projde na body pod nimi", n)
                } else {
                    trf!("Povrchy ({}) jdou znovu vybrat kliknutím", n)
                }))
            }
            Akce::SmazatKontrolu => {
                let doc = k.dokument()?;
                let h: Vec<Handle> = doc
                    .model_space_entities()
                    .filter(|e| e.common().layer.eq_ignore_ascii_case(H_KONTROLA))
                    .map(|e| e.common().handle)
                    .collect();
                k.zpet(tr("Smazat značky kontroly"));
                let n = cogo::smaz_objekty(k, h);
                k.hotovo();
                uprav_stav(|s| s.kontrola = None);
                Ok(Some(trf!("Smazáno {} značek kontroly", n)))
            }

            // ── skupiny ──
            Akce::SkupinaUlozit(mut s) => {
                let doc = k.dokument()?;
                s.jmeno = s.jmeno.trim().to_string();
                if s.jmeno.is_empty() {
                    return Err(tr("jméno skupiny nesmí být prázdné").into());
                }
                if s.id.is_empty() {
                    s.id = cogo::volne_id_skupiny(&doc, &s.jmeno);
                }
                let stary_styl = cogo::najdi_skupinu(&doc, &s.id).map(|g| g.styl).unwrap_or_default();
                k.zpet(tr("Skupina bodů"));
                cogo::uloz_skupinu(k, &doc, &s)?;
                if stary_styl != s.styl {
                    let doc = k.dokument()?;
                    cogo::obnov_grafiku(k, &doc)?;
                }
                k.hotovo();
                skoc(Skok::Skupina(s.id.clone()));
                Ok(Some(trf!("Skupina „{}“ uložena", s.jmeno)))
            }
            Akce::SkupinaNova(h) => {
                let doc = k.dokument()?;
                let jmeno = cogo::volne_jmeno_skupiny(&doc, tr("Nová skupina"));
                let mut s = SkupinaBodu { id: cogo::volne_id_skupiny(&doc, &jmeno), jmeno, ..Default::default() };
                let cisla: Vec<String> =
                    cogo::body(&doc).into_iter().filter(|b| h.contains(&b.handle)).map(|b| b.bod.cislo).collect();
                let mut sez = SeznamCisel::default();
                sez.pridej(&cisla);
                s.cisla = sez.text();
                k.zpet(tr("Nová skupina bodů"));
                cogo::uloz_skupinu(k, &doc, &s)?;
                k.hotovo();
                skoc(Skok::Skupina(s.id.clone()));
                Ok(Some(trf!("Skupina „{}“ vytvořena ({} bodů)", s.jmeno, cisla.len())))
            }
            Akce::SkupinaSmazat(id) => {
                let doc = k.dokument()?;
                let pouzita: Vec<String> = povrchy::povrchy(&doc)
                    .into_iter()
                    .filter(|z| z.skupiny.iter().any(|g| g.id.eq_ignore_ascii_case(&id)))
                    .map(|z| z.jmeno)
                    .collect();
                if !pouzita.is_empty() {
                    return Err(trf!("skupinu používá povrch {} – nejdřív ji z povrchu odeberte", pouzita.join(", ")));
                }
                k.zpet(tr("Smazat skupinu bodů"));
                cogo::smaz_skupinu(k, &doc, &id)?;
                k.hotovo();
                Ok(Some(tr("Skupina smazána (body zůstaly)").into()))
            }
            Akce::SkupinaBody(id, h, pridat) => {
                let doc = k.dokument()?;
                Self::skupina_body(k, &doc, &id, &h, pridat)
            }
            Akce::SkupinaVyberVykresu(id, pridat) => {
                let vyber = k.vybrane();
                let doc = k.dokument()?;
                let body: Vec<Handle> = cogo::body(&doc).into_iter().map(|b| b.handle).filter(|h| vyber.contains(h)).collect();
                if body.is_empty() {
                    return Err(tr("ve výkrese nejsou vybrané žádné body").into());
                }
                Self::skupina_body(k, &doc, &id, &body, pridat)
            }
            Akce::PovrchZeSkupiny(skupina, jmeno, auto) => {
                let doc = k.dokument()?;
                let s = cogo::najdi_skupinu(&doc, &skupina).ok_or(tr("skupina už ve výkrese není"))?;
                let jmeno = jmeno.unwrap_or_else(|| trf!("Povrch {}", s.jmeno));
                let mut zaklad = cogo::bez_diakritiky(&jmeno);
                if zaklad.is_empty() {
                    zaklad = "POVRCH".into();
                }
                let id = povrchy::volne_id(&doc, &zaklad);
                let mut z = ZaznamPovrchu::novy(id, jmeno, false);
                z.skupiny.push(SkupinaVPovrchu { id: s.id.clone(), zapnuto: true });
                z.auto = auto;
                if s.id == VSECHNY {
                    z.vyloucene_kody = nastaveni::akt().vyloucene_kody;
                }
                let r = Self::prestav(k, &mut z, tr("Nový povrch"))?;
                skoc(Skok::Povrch(z.id.clone()));
                Ok(r)
            }
            Akce::ExportPovrchu(id, cesta, landxml) => {
                let (z, doc) = Self::zaznam(k, &id)?;
                let def = povrchy::definice(&doc, &z).definice;
                let p = teren::povrch::Povrch::z_definice(&def, 0.001).map_err(|e| e.to_string())?;
                if p.trojuhelniky.is_empty() {
                    return Err(trf!("povrch „{}“ nemá žádné trojúhelníky – nejdřív ho přepočítejte", z.jmeno));
                }
                let cesta = cesta.trim().trim_matches('"').to_string();
                if cesta.is_empty() {
                    return Err(tr("zadejte cestu k souboru").into());
                }
                let bajty = if landxml {
                    teren::landxml::povrch(&p, &z.jmeno, "", &def.body, env!("CARGO_PKG_VERSION")).into_bytes()
                } else {
                    let mut d = teren::dxf::Dxf::default();
                    d.pridej_tin(&p);
                    d.do_bajtu()
                };
                std::fs::write(&cesta, bajty).map_err(|e| trf!("soubor {} nejde zapsat: {}", cesta, e))?;
                let co = if landxml { "LandXML" } else { tr("DXF s 3D plochami") };
                k.info(&trf!("Terén: „{}“ – {} trojúhelníků uloženo do {} ({})", z.jmeno, p.trojuhelniky.len(), cesta, co));
                Ok(Some(trf!("Uloženo {} trojúhelníků do {}", p.trojuhelniky.len(), cesta)))
            }
            Akce::NovyPovrch(n) => {
                let doc = k.dokument()?;
                let mut zaklad = cogo::bez_diakritiky(&n.jmeno);
                if zaklad.is_empty() {
                    zaklad = "POVRCH".into();
                }
                let id = povrchy::volne_id(&doc, &zaklad);
                let mut z = ZaznamPovrchu::novy(id, n.jmeno.clone(), false);
                z.skupiny = n.skupiny.iter().map(|s| SkupinaVPovrchu { id: s.clone(), zapnuto: true }).collect();
                z.vyloucene_kody = n.vyloucene_kody;
                z.max_hrana = n.max_hrana;
                z.styl = n.styl;
                (z.interval, z.hlavni) = n.interval;
                z.auto = n.auto;
                let r = Self::prestav(k, &mut z, tr("Nový povrch"))?;
                povrchy::nastav_aktivni(&z.id);
                skoc(Skok::Povrch(z.id.clone()));
                Ok(r)
            }
            Akce::Export(skupina, cesta, format, odd, cp1250) => {
                let doc = k.dokument()?;
                let s = cogo::najdi_skupinu(&doc, &skupina).ok_or(tr("skupina už ve výkrese není"))?;
                let vse = cogo::body(&doc);
                let body: Vec<Bod> = cogo::body_skupin(&vse, std::slice::from_ref(&s)).into_iter().map(|b| b.bod.clone()).collect();
                if body.is_empty() {
                    return Err(tr("skupina neobsahuje žádné body").into());
                }
                let fmt = FormatBodu::z_retezce(&format, odd)?;
                let text = zapis_body(&body, &fmt);
                let bajty = if cp1250 { teren::koduj_cp1250(&text) } else { text.into_bytes() };
                let cesta = cesta.trim().trim_matches('"').to_string();
                if cesta.is_empty() {
                    return Err(tr("zadejte cestu k souboru").into());
                }
                std::fs::write(&cesta, bajty).map_err(|e| trf!("soubor {} nejde zapsat: {}", cesta, e))?;
                k.info(&trf!("Terén: export {} bodů skupiny „{}“ do {} ({})", body.len(), s.jmeno, cesta, format));
                Ok(Some(trf!("Exportováno {} bodů do {}", body.len(), cesta)))
            }
        }
    }

    fn skupina_body(k: &mut dyn Vykres, doc: &CadDocument, id: &str, h: &[Handle], pridat: bool) -> Vysledek {
        if id == VSECHNY {
            return Err(tr("skupinu Všechny body nelze měnit").into());
        }
        let mut s = cogo::najdi_skupinu(doc, id).ok_or(tr("skupina už ve výkrese není"))?;
        let cisla: Vec<String> = cogo::body(doc).into_iter().filter(|b| h.contains(&b.handle)).map(|b| b.bod.cislo).collect();
        let bez_cisla = h.len().saturating_sub(cisla.iter().filter(|c| !c.is_empty()).count());
        // ručně přidané / odebrané body mají přednost před pravidly
        let mut pridane = SeznamCisel::z_textu(&s.pridane);
        let mut odebrane = SeznamCisel::z_textu(&s.odebrane);
        if pridat {
            odebrane.odeber(&cisla);
            s.odebrane = odebrane.text();
            // bod, který už do skupiny patří podle pravidel, se nepřidává zvlášť
            let vse = cogo::body(doc);
            let f = s.filtr();
            let nove: Vec<String> = vse
                .iter()
                .filter(|b| h.contains(&b.handle) && !f.obsahuje(&b.bod, &b.zdroj))
                .map(|b| b.bod.cislo.clone())
                .collect();
            pridane.pridej(&nove);
        } else {
            pridane.odeber(&cisla);
            s.pridane = pridane.text();
            let vse = cogo::body(doc);
            let f = s.filtr();
            let porad: Vec<String> =
                vse.iter().filter(|b| h.contains(&b.handle) && f.obsahuje(&b.bod, &b.zdroj)).map(|b| b.bod.cislo.clone()).collect();
            odebrane.pridej(&porad);
        }
        s.pridane = pridane.text();
        s.odebrane = odebrane.text();
        k.zpet(tr("Skupina bodů"));
        cogo::uloz_skupinu(k, doc, &s)?;
        k.hotovo();
        let mut t = trf!("Skupina „{}“: {} {} bodů", s.jmeno, if pridat { tr("přidáno") } else { tr("odebráno") }, cisla.len());
        if bez_cisla > 0 {
            t.push_str(&trf!(" ({} bodů bez čísla nejde přidat)", bez_cisla));
        }
        Ok(Some(t))
    }

    /// Značky úprav z klikání se před změnou seznamu převezmou do definice,
    /// aby indexy v okně odpovídaly.
    fn prevezmi_znacky(k: &mut dyn Vykres, doc: &CadDocument, z: &mut ZaznamPovrchu) {
        for (h, e) in povrchy::znacky_uprav(doc, &z.id) {
            z.upravy.push(Uprava { e, zapnuto: true });
            k.odeber(h);
        }
    }
}

/// Provede akci Prospektoru hned (z příkazu `TER_AKCE`, pro zkoušení a skripty).
pub fn proved_hned(v: &mut dyn Vykres, a: Akce) -> Result<Option<String>, String> {
    Pracovnik::proved(v, a)
}

/// Přehled výkresu jako text (pro `TER_AKCE PREHLED`).
pub fn prehled_text(doc: &CadDocument) -> Vec<String> {
    let p = prehled(doc);
    let mut out = vec![trf!("bodů {}, další číslo {}, soubory: {}, měřítko 1:{}, stylů bodů {}, stylů povrchů {}", p.body.len(), p.dalsi_cislo, p.zdroje.join(", "), nastaveni::meritko(doc), styly::styly_bodu(doc).len(), styly::styly_povrchu(doc).len())];
    let ro = styly::Rozlisovac::new(doc);
    let mut pocty: std::collections::BTreeMap<String, usize> = Default::default();
    for b in &p.body {
        *pocty.entry(ro.styl_bodu(b).id).or_default() += 1;
    }
    out.push(format!("styly bodů: {pocty:?}"));
    for s in &p.skupiny {
        out.push(trf!("skupina {} „{}“: {} bodů [{}] styl {} používá: {}", s.skupina.id, s.skupina.jmeno, s.pocet, s.skupina.kriteria(), s.skupina.styl, s.povrchy.join(", ")));
    }
    for z in &p.povrchy {
        let sk: Vec<String> = z.skupiny.iter().map(|g| format!("{}{}={}", if g.zapnuto { "" } else { "-" }, g.id, g.pocet)).collect();
        let pol: Vec<String> = z
            .polozky
            .iter()
            .map(|x| format!("{:?}{}{}:{:X}", x.druh, if x.zapnuto { "" } else { "(vyp)" }, if x.hrany { "+h" } else { "" }, x.handle.value()))
            .collect();
        out.push(trf!("povrch {} „{}“ styl {}: {} trojúhelníků, bodů {} (vyl. {}), z {:.2}–{:.2}, plocha {:.1}, {}{}skupiny [{}] položky [{}] úprav {}", z.id, z.jmeno, z.styl, z.stat.trojuhelniku, z.pocet_bodu, z.vylouceno_bodu, z.stat.z_min, z.stat.z_max, z.stat.plocha_2d, if z.zastaraly { tr("ZASTARALÝ, ") } else { "" }, if z.auto { "auto, " } else { "" }, sk.join(" "), pol.join(" "), z.upravy.len()));
    }
    out
}

/// Po změně měřítka: přepíše grafiku bodů a přepočítá povrchy, jejichž styl
/// na měřítku závisí (popisy vrstevnic, šipky).
pub fn po_zmene_meritka(v: &mut dyn Vykres) -> Result<String, String> {
    let doc = v.dokument()?;
    let m = nastaveni::meritko(&doc);
    v.zpet(tr("Měřítko výkresu"));
    let n = cogo::obnov_grafiku(v, &doc)?;
    let doc = v.dokument()?;
    let mut prepocteno = 0;
    for mut z in povrchy::povrchy(&doc) {
        if povrchy::zastaraly(&doc, &z) && povrchy::prestav(v, &mut z).is_ok() {
            prepocteno += 1;
        }
    }
    v.hotovo();
    Ok(trf!("měřítko 1:{} – překresleno {} bodů{}", povrchy::cislo_cz(m), n, if prepocteno > 0 { trf!(", přepočteno povrchů: {}", prepocteno) } else { String::new() }))
}

/// K čemu ve stromu patří vybrané objekty výkresu.
pub fn najdi_ve_strome(doc: &CadDocument, vyber: &[Handle]) -> Option<(Skok, String)> {
    let vse = povrchy::povrchy(doc);
    // položka definice povrchu (čára, objekt výkresu)
    for z in &vse {
        let zdroje = povrchy::definice(doc, z);
        if let Some(p) = zdroje.polozky.iter().find(|p| vyber.contains(&p.handle)) {
            return Some((Skok::Polozka(z.id.clone(), p.handle), trf!("Položka definice povrchu „{}“", z.jmeno)));
        }
    }
    // povrch sám (blok nebo objekt na jeho hladinách)
    for z in &vse {
        let pref = format!("{}_", z.hladiny().povrch.to_uppercase());
        let patri = vyber.contains(&z.insert)
            || doc
                .model_space_entities()
                .any(|e| vyber.contains(&e.common().handle) && e.common().layer.to_uppercase().starts_with(&pref));
        if patri {
            return Some((Skok::Povrch(z.id.clone()), trf!("Povrch „{}“", z.jmeno)));
        }
    }
    let body = cogo::body_z_vyberu(doc, vyber);
    if !body.is_empty() {
        let n = body.len();
        return Some((Skok::Body(body), trf!("Vybráno bodů: {}", n)));
    }
    None
}

pub struct Prehled {
    pub povrchy: Vec<PrehledPovrchu>,
    pub body: Vec<BodVykresu>,
    pub skupiny: Vec<PrehledSkupiny>,
    pub zdroje: Vec<String>,
    pub dalsi_cislo: u64,
}

/// Přehled výkresu pro okno.
pub fn prehled(doc: &CadDocument) -> Prehled {
    let aktivni = povrchy::aktivni(doc).ok().map(|z| z.id);
    let hlaseni = zamek(povrchy::posledni_hlaseni()).clone();
    let body = cogo::body(doc);
    let skupiny = cogo::skupiny(doc);
    let zaznamy = povrchy::povrchy(doc);
    let povrchy_prehled: Vec<PrehledPovrchu> = zaznamy
        .iter()
        .cloned()
        .map(|z| {
            let zdroje = povrchy::definice(doc, &z);
            let zastaraly = povrchy::otisk(&z, &zdroje) != z.otisk || zdroje.upravy.len() != z.upravy.len();
            let skupiny_povrchu = z
                .skupiny
                .iter()
                .map(|g| {
                    let s = skupiny.iter().find(|s| s.id.eq_ignore_ascii_case(&g.id));
                    SkupinaPovrchu {
                        id: g.id.clone(),
                        jmeno: s.map_or_else(|| trf!("{} (chybí)", g.id), |s| s.jmeno.clone()),
                        zapnuto: g.zapnuto,
                        pocet: zdroje.body_skupin.iter().find(|(id, _)| id == &g.id).map_or(0, |x| x.1),
                        chybi: s.is_none(),
                    }
                })
                .collect();
            PrehledPovrchu {
                aktivni: aktivni.as_deref() == Some(z.id.as_str()),
                zastaraly,
                auto: z.auto,
                styl: z.styl.clone(),
                interval: z.interval,
                hlavni: z.hlavni,
                max_hrana: z.max_hrana,
                skupiny: skupiny_povrchu,
                vyloucene_kody: z.vyloucene_kody.join(", "),
                stat: z.stat.clone(),
                pocet_bodu: zdroje.pocet_bodu,
                vylouceno_bodu: zdroje.vylouceno_bodu,
                polozky: zdroje.polozky,
                upravy: zdroje.upravy,
                hlaseni: hlaseni.get(&z.id).cloned(),
                id: z.id,
                jmeno: z.jmeno,
            }
        })
        .collect();
    let skupiny_prehled = skupiny
        .into_iter()
        .map(|s| PrehledSkupiny {
            pocet: cogo::body_skupin(&body, std::slice::from_ref(&s)).len(),
            povrchy: zaznamy
                .iter()
                .filter(|z| z.skupiny.iter().any(|g| g.id.eq_ignore_ascii_case(&s.id)))
                .map(|z| z.jmeno.clone())
                .collect(),
            skupina: s,
        })
        .collect();
    let mut zdroje: Vec<String> = body.iter().map(|b| b.zdroj.clone()).filter(|z| !z.is_empty()).collect();
    zdroje.sort();
    zdroje.dedup();
    let dalsi_cislo = teren::skupiny::dalsi_cislo(body.iter().map(|b| b.bod.cislo.as_str()));
    Prehled { povrchy: povrchy_prehled, body, skupiny: skupiny_prehled, zdroje, dalsi_cislo }
}

/// Kontrola zaměření: najde body s podezřelou výškou a označí je ve výkrese.
pub fn kontrola(v: &mut dyn Vykres) -> Result<String, String> {
    let n = nastaveni::akt();
    let doc = v.dokument()?;
    let body = cogo::body(&doc);
    let jen_body: Vec<_> = body.iter().map(|b| b.bod.clone()).collect();
    let nalez = podezrele_body(&jen_body, n.prah_kontroly);
    // staré značky pryč
    let stare: Vec<Handle> = doc
        .model_space_entities()
        .filter(|e| e.common().layer.eq_ignore_ascii_case(H_KONTROLA))
        .map(|e| e.common().handle)
        .collect();
    v.zpet(tr("Kontrola zaměření"));
    cogo::smaz_objekty(v, stare);
    let seznam: Vec<PodezrelyBod> = nalez
        .iter()
        .map(|p| {
            let b = &body[p.index];
            PodezrelyBod { handle: b.handle, cislo: b.bod.cislo.clone(), x: b.bod.x, y: b.bod.y, z: b.bod.z, dz: p.dz }
        })
        .collect();
    if !seznam.is_empty() {
        povrchy::zajisti_hladinu(v, &doc, H_KONTROLA, 6);
        let vt = nastaveni::meritko(&doc) * 0.002;
        let r = vt * 2.5;
        let mut ent = Vec::new();
        for p in &seznam {
            let mut c = Circle::from_center_radius(Vector3::new(p.x, p.y, p.z), r);
            c.common.layer = H_KONTROLA.into();
            ent.push(EntityType::Circle(c));
            let mut t = Text::with_value(format!("{:+.2}", p.dz), Vector3::new(p.x + r * 1.1, p.y + r * 0.6, p.z))
                .with_height(vt);
            t.common.layer = H_KONTROLA.into();
            ent.push(EntityType::Text(t));
        }
        v.pridej(ent)?;
    }
    v.hotovo();
    let zprava = if seznam.is_empty() {
        trf!("Kontrola zaměření: žádný podezřelý bod (práh {} m)", povrchy::cislo_cz(n.prah_kontroly))
    } else {
        trf!("Kontrola zaměření: {} podezřelých bodů – označeny na hladině {}", seznam.len(), H_KONTROLA)
    };
    v.info(&trf!("Terén: {}", zprava));
    uprav_stav(|s| s.kontrola = Some(seznam));
    Ok(zprava)
}

/// Název kroku Zpět pro akci z Prospektoru.
fn nazev_kroku(a: &Akce) -> &'static str {
    match a {
        Akce::Import(..) => tr("Terén: import bodů"),
        Akce::Prepocitat(_) => tr("Terén: přepočet povrchu"),
        Akce::Smazat(_) => tr("Terén: smazání povrchu"),
        Akce::Nastav(..) => tr("Terén: vlastnosti povrchu"),
        Akce::BodUpravit(..) => tr("Terén: úprava bodu"),
        Akce::BodySmazat(_) => tr("Terén: smazání bodů"),
        Akce::BodyPrecislovat(..) => tr("Terén: přečíslování bodů"),
        Akce::BodyHromadne(..) => tr("Terén: úprava bodů"),
        Akce::PopisyNaMisto(_) => tr("Terén: popisy na místo"),
        Akce::BodyPanel(..) => tr("Terén: úprava bodů"),
        Akce::Uchopy(_) | Akce::UkazatBody(_) | Akce::Hlaseni(_) => tr("Terén"),
        Akce::ObnovitPopisy => tr("Terén: obnovení popisů"),
        Akce::StylBodu(..) | Akce::UlozStylBodu(..) => tr("Terén: styl bodu"),
        Akce::UlozStylPovrchu(..) => tr("Terén: styl povrchu"),
        Akce::SablonaNacti => tr("Terén: šablona výkresu"),
        Akce::Meritko(_) => tr("Terén: měřítko"),
        Akce::PripravVykres => tr("Terén: příprava výkresu"),
        Akce::NovyPovrch(_) | Akce::NovyPovrchPas(..) | Akce::PovrchZeSkupiny(..) => tr("Terén: nový povrch"),
        Akce::SrovnatBody(_) => tr("Terén: srovnání bodů"),
        _ => tr("Terén"),
    }
}
