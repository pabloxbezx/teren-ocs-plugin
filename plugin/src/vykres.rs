//! Jednotný přístup k výkresu.
//!
//! Příkazy spuštěné tlačítkem dostanou `HostApi`, interaktivní příkazy
//! (klikání do výkresu) mají jen kanál pro požadavky na hostitele.
//! Obojí se tu schová za trait `Vykres`, aby se povrch dal přestavět
//! odkudkoli stejným kódem.

use crate::lang::tr;
use teren::trf;
use ocs_plugin_api::host::{
    Document as CadDocument, EntityType, ExtendedDataRecord, Handle, HostApi, LayerConfig, PluginRequestSender, TableOperation,
};
use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
use std::sync::atomic::{AtomicU64, Ordering as Poradi};

// ── Velikost zpráv ──────────────────────────────────────────────────────────
//
// Open CAD Studio přenese mezi pluginem a hostitelem zprávu nejvýš 64 MB.
// Větší snímek výkresu hostitel neodešle a plugin by na odpověď čekal navždy
// (pomocný proces pluginu pak do restartu OCS nereaguje). Proto:
// - vkládání objektů jde po dávkách,
// - před stažením velkého výkresu se jeho velikost odhadne ze sdílené paměti
//   a při překročení limitu se vrátí srozumitelná chyba.

/// Limit zprávy s rezervou na obálku zprávy.
pub const LIMIT_ZPRAVY: u64 = 60 * 1024 * 1024;
/// Největší dávka vkládaných objektů.
const DAVKA: u64 = 24 * 1024 * 1024;
/// Od této velikosti posledního snímku se před dalším ověřuje velikost.
const HLIDAT_OD: u64 = 32 * 1024 * 1024;
/// Velikost posledního staženého snímku výkresu (bajty, odhad).
static POSLEDNI_SNIMEK: AtomicU64 = AtomicU64::new(0);

/// Poslední stažený snímek výkresu byl velký – další stažení stojí znatelný
/// čas a může narazit na limit zprávy (tam, kde se mu dá vyhnout, nestahovat).
pub fn velky_vykres() -> bool {
    POSLEDNI_SNIMEK.load(Poradi::Relaxed) > HLIDAT_OD
}

/// Velikost objektu ve zprávě.
fn velikost(e: &EntityType) -> u64 {
    bincode::serialized_size(e).unwrap_or(0)
}

/// Rozdělí objekty do dávek, které se vejdou do jedné zprávy.
pub fn davky(e: Vec<EntityType>) -> Vec<Vec<EntityType>> {
    let mut out: Vec<Vec<EntityType>> = Vec::new();
    let mut akt: Vec<EntityType> = Vec::new();
    let (mut vel, mut celkem) = (0u64, 0u64);
    for x in e {
        let v = velikost(&x);
        if !akt.is_empty() && vel + v > DAVKA {
            out.push(std::mem::take(&mut akt));
            vel = 0;
        }
        vel += v;
        celkem += v;
        akt.push(x);
    }
    if !akt.is_empty() {
        out.push(akt);
    }
    // vložené objekty zvětší výkres – ať se před dalším snímkem ověří velikost
    POSLEDNI_SNIMEK.fetch_add(celkem, Poradi::Relaxed);
    out
}

/// Zapamatuje si velikost staženého snímku (jen u velkých výkresů – výpočet něco stojí).
fn zapamatuj_velikost(doc: &CadDocument) {
    let v = if doc.entities().count() > 15_000 { bincode::serialized_size(doc).unwrap_or(0) } else { 0 };
    POSLEDNI_SNIMEK.store(v, Poradi::Relaxed);
}

fn chyba_velikosti(v: u64) -> String {
    trf!(
        "výkres je na přenos do Terénu příliš velký (asi {} MB, Open CAD Studio přenese nejvýš 64 MB) – rozdělte body do více výkresů nebo zmenšete povrch", "the drawing is too large to transfer to Terrain (about {} MB, Open CAD Studio transfers at most 64 MB) – split the points into several drawings or make the surface smaller", // i18n:ne
        v / (1024 * 1024)
    )
}

pub trait Vykres {
    /// Aktuální stav výkresu (kopie).
    fn dokument(&mut self) -> Result<CadDocument, String>;
    fn info(&mut self, zprava: &str);
    fn vystup(&mut self, zprava: &str);
    fn chyba(&mut self, zprava: &str);
    fn zpet(&mut self, nazev: &str);
    /// Otevře jeden krok Zpět pro všechny následující změny (až po `konec_kroku`).
    fn zacatek_kroku(&mut self, nazev: &str) {
        self.zpet(nazev);
    }
    fn konec_kroku(&mut self) {}
    fn hotovo(&mut self);
    /// Jako [`Vykres::hotovo`], ale bez překreslení celého výkresu – pro změny
    /// objektů v modelu (vložení, smazání, transakce), které OCS překreslí sám.
    /// Překreslení celého výkresu je potřeba jen po změně definic bloků.
    fn zmeneno(&mut self) {
        self.hotovo()
    }
    fn pridej(&mut self, e: Vec<EntityType>) -> Result<Vec<Handle>, String>;
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String>;
    fn uprav(&mut self, e: EntityType) -> bool;
    fn odeber(&mut self, h: Handle) -> bool;
    fn zapis_xdata(&mut self, h: Handle, r: ExtendedDataRecord) -> bool;
    fn tabulka(&mut self, op: TableOperation) -> Result<Handle, String>;
    fn hladina(&mut self, cfg: LayerConfig);
    /// Spustí příkaz Open CAD Studia (např. ZOOM).
    fn prikaz(&mut self, prikaz: &str) -> bool;
    /// Nastaví výběr objektů.
    fn vyber(&mut self, h: Vec<Handle>);
    /// Aktuální výběr objektů ve výkrese.
    fn vybrane(&mut self) -> Vec<Handle>;
    /// Nahradí existující objekty jedním krokem zpět.
    fn transakce(&mut self, popis: &str, e: Vec<EntityType>) -> Result<(), String>;
    /// Objekt jen pro zobrazení: nejde vybrat kliknutím ani upravit (API v9).
    fn jen_zobrazeni(&mut self, h: Handle, ano: bool) -> bool;
}

/// Nejvíc objektů, které se při přepisu bloku na místě smí mazat nebo
/// přidávat po jednom; nad tím je rychlejší blok postavit znovu.
const NA_MISTE_NEJVIC: usize = 8;

/// Zapíše záznam XDATA do objektu (nahradí záznam téže aplikace).
pub fn nastav_zaznam(e: &mut EntityType, r: ExtendedDataRecord) {
    let xd = &mut e.common_mut().extended_data;
    xd.remove_record(&r.application_name);
    xd.add_record(r);
}

/// Přepíše obsah existujícího bloku na místě: nové objekty převezmou handle
/// členů bloku stejného druhu a hladiny a spolu s upraveným vložením (`vlozeni`,
/// stejný handle – poloha, XDATA) se zapíšou jednou transakcí. OCS pak samo
/// překreslí všechna vložení bloku. Zbylé členy se smažou, chybějící přidají
/// po jednom (jen pár – jinak se vrátí `false` a volající blok postaví znovu
/// postaru). `false` také když blok chybí nebo OCS transakci odmítne; v obou
/// případech se výkres nezměnil.
pub fn prepis_blok(v: &mut dyn Vykres, doc: &CadDocument, blok: &str, nove: Vec<EntityType>, vlozeni: EntityType, popis: &str) -> Result<bool, String> {
    let Some(zaznam) = doc.block_records.iter().find(|b| b.name.eq_ignore_ascii_case(blok)) else { return Ok(false) };
    let clenove: Vec<Handle> = zaznam
        .entity_handles
        .iter()
        .copied()
        .filter(|h| !matches!(doc.get_entity(*h), None | Some(EntityType::Block(_)) | Some(EntityType::BlockEnd(_))))
        .collect();
    let (mut sparovane, pridat, smazat) = crate::cogo::sparuj(doc, &clenove, nove);
    ladeni(&format!("blok {blok}: spárováno {}, přidat {}, smazat {}", sparovane.len(), pridat.len(), smazat.len())); // i18n:ne
    if pridat.len() + smazat.len() > NA_MISTE_NEJVIC {
        ladeni(&format!("blok {blok}: postaru (moc změn)")); // i18n:ne
        return Ok(false);
    }
    sparovane.push(vlozeni);
    if let Err(e) = v.transakce(popis, sparovane) {
        ladeni(&format!("blok {blok}: postaru, transakce odmítnuta: {e}")); // i18n:ne
        return Ok(false);
    }
    for h in smazat {
        v.odeber(h);
    }
    for e in pridat {
        v.tabulka(TableOperation::BlockEntityAdd { block: zaznam.name.clone(), entity: e })?;
    }
    Ok(true)
}

/// Název požadavku pro ladicí záznam – jen začátek výpisu (celý výpis
/// vkládání tisíců objektů by měl desítky MB a zkreslil by měření).
fn nazev_pozadavku(r: &PluginRequest) -> String {
    struct Zacatek(String);
    impl std::fmt::Write for Zacatek {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.push_str(s);
            if self.0.len() > 48 { Err(std::fmt::Error) } else { Ok(()) }
        }
    }
    let mut z = Zacatek(String::new());
    let _ = std::fmt::write(&mut z, format_args!("{r:?}"));
    z.0.chars().take_while(|c| c.is_alphanumeric()).collect()
}

/// Řádek do ladicího záznamu (proměnná prostředí `TEREN_LADENI` = soubor).
pub fn ladeni(zprava: &str) {
    if let Some(soubor) = std::env::var_os("TEREN_LADENI") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(soubor) {
            use std::io::Write;
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
            let _ = f.write_all(format!("{t:.3} # {zprava}\n").as_bytes());
        }
    }
}

/// Výkres přes `HostApi` (příkaz spuštěný tlačítkem).
pub struct PresHost<'a>(pub &'a mut dyn HostApi);

impl Vykres for PresHost<'_> {
    fn dokument(&mut self) -> Result<CadDocument, String> {
        // Kopie se bere přes kanál požadavků, aby odpovídala stavu po předchozích změnách.
        match self.0.plugin_request_sender() {
            Some(s) => PresKanal(s).dokument(),
            None => Ok(self.0.document().clone()),
        }
    }
    fn info(&mut self, z: &str) {
        self.0.push_info(z)
    }
    fn vystup(&mut self, z: &str) {
        self.0.push_output(z)
    }
    fn chyba(&mut self, z: &str) {
        self.0.push_error(z)
    }
    fn zpet(&mut self, n: &str) {
        // všechny změny jednoho příkazu = jeden krok Zpět (zavře ho dispatch)
        self.0.begin_undo_group(n)
    }
    fn zacatek_kroku(&mut self, n: &str) {
        self.0.begin_undo_group(n)
    }
    fn konec_kroku(&mut self) {
        self.0.end_undo_group()
    }
    fn hotovo(&mut self) {
        self.0.bump_geometry();
        self.0.set_dirty();
    }
    fn pridej(&mut self, e: Vec<EntityType>) -> Result<Vec<Handle>, String> {
        let n = e.len();
        let mut h = Vec::with_capacity(n);
        for d in davky(e) {
            h.extend(self.0.add_entities(d));
        }
        if h.len() == n { Ok(h) } else { Err(trf!("vloženo {} z {} objektů", "inserted {} of {} objects", h.len(), n)) } // i18n:ne
    }
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String> {
        let h = self.0.add_entity(e);
        if h == Handle::NULL { Err(tr("objekt se nepodařilo vložit", "the object could not be inserted").into()) } else { Ok(h) } // i18n:ne
    }
    fn uprav(&mut self, e: EntityType) -> bool {
        self.0.update_entity(e)
    }
    fn odeber(&mut self, h: Handle) -> bool {
        self.0.remove_entity(h)
    }
    fn zapis_xdata(&mut self, h: Handle, r: ExtendedDataRecord) -> bool {
        self.0.write_record(h, r)
    }
    fn tabulka(&mut self, op: TableOperation) -> Result<Handle, String> {
        self.0.table_operation(op)
    }
    fn hladina(&mut self, cfg: LayerConfig) {
        self.0.add_layer(cfg);
    }
    fn prikaz(&mut self, prikaz: &str) -> bool {
        self.0.execute_command(prikaz)
    }
    fn vyber(&mut self, h: Vec<Handle>) {
        let _ = self.0.set_selection(&h);
    }
    fn vybrane(&mut self) -> Vec<Handle> {
        self.0.selection()
    }
    fn transakce(&mut self, popis: &str, e: Vec<EntityType>) -> Result<(), String> {
        if e.is_empty() {
            return Ok(());
        }
        self.0.update_entities_transaction(popis, e)
    }
    fn jen_zobrazeni(&mut self, h: Handle, ano: bool) -> bool {
        self.0.set_reference_only(h, ano).is_ok()
    }
}

/// Výkres přímo přes `HostApi` bez kanálu požadavků (uvnitř tažení vlastního
/// úchopu – pracovní vlákno tehdy čeká, kanál by čekal také). Krok Zpět
/// otevírá volající přes `push_undo`.
pub struct PresHostPrimo<'a>(pub &'a mut dyn HostApi);

impl Vykres for PresHostPrimo<'_> {
    fn dokument(&mut self) -> Result<CadDocument, String> {
        Ok(self.0.document().clone())
    }
    fn info(&mut self, z: &str) {
        self.0.push_info(z)
    }
    fn vystup(&mut self, z: &str) {
        self.0.push_output(z)
    }
    fn chyba(&mut self, z: &str) {
        self.0.push_error(z)
    }
    fn zpet(&mut self, _n: &str) {}
    fn zacatek_kroku(&mut self, _n: &str) {}
    fn konec_kroku(&mut self) {}
    fn hotovo(&mut self) {
        self.0.bump_geometry();
        self.0.set_dirty();
    }
    fn pridej(&mut self, e: Vec<EntityType>) -> Result<Vec<Handle>, String> {
        PresHost(&mut *self.0).pridej(e)
    }
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String> {
        PresHost(&mut *self.0).pridej_jednu(e)
    }
    fn uprav(&mut self, e: EntityType) -> bool {
        self.0.update_entity(e)
    }
    fn odeber(&mut self, h: Handle) -> bool {
        self.0.remove_entity(h)
    }
    fn zapis_xdata(&mut self, h: Handle, r: ExtendedDataRecord) -> bool {
        self.0.write_record(h, r)
    }
    fn tabulka(&mut self, op: TableOperation) -> Result<Handle, String> {
        self.0.table_operation(op)
    }
    fn hladina(&mut self, cfg: LayerConfig) {
        self.0.add_layer(cfg);
    }
    fn prikaz(&mut self, prikaz: &str) -> bool {
        self.0.execute_command(prikaz)
    }
    fn vyber(&mut self, h: Vec<Handle>) {
        let _ = self.0.set_selection(&h);
    }
    fn vybrane(&mut self) -> Vec<Handle> {
        self.0.selection()
    }
    fn transakce(&mut self, popis: &str, e: Vec<EntityType>) -> Result<(), String> {
        PresHost(&mut *self.0).transakce(popis, e)
    }
    fn jen_zobrazeni(&mut self, h: Handle, ano: bool) -> bool {
        self.0.set_reference_only(h, ano).is_ok()
    }
}

/// Běží interaktivní příkaz – kroky Zpět bez skupin (skupina by se s
/// krokem příkazu pobila).
pub static BEZ_SKUPIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Výkres přes kanál požadavků (uvnitř interaktivního příkazu).
pub struct PresKanal(pub Box<dyn PluginRequestSender>);

impl PresKanal {
    /// Ohlásí vlastní úchopy pluginu (nahradí předchozí).
    pub fn uchopy(&mut self, g: Vec<ocs_plugin_api::host::PluginGrip>) {
        let _ = self.pozadavek(PluginRequest::SetPluginGrips { grips: g });
    }
    /// Ohlásí kurzorové menu pluginu; true = přijato.
    pub fn quad(&mut self, panel: ocs_plugin_api::dialog::QuadPanelDef) -> bool {
        matches!(
            self.pozadavek(PluginRequest::RegisterQuadPanels { panels: vec![panel] }),
            Ok(PluginResponse::QuadPanelsRegistered { ok, .. }) if ok.first().copied().unwrap_or(false)
        )
    }
    /// Nové hodnoty polí panelu Bod Terénu.
    /// Nové hodnoty polí libovolného panelu pluginu.
    pub fn panel_id(&mut self, id: &str, hodnoty: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>) {
        let _ = self.pozadavek(PluginRequest::UpdatePanel { panel_id: id.into(), values: hodnoty });
    }
    /// Zaregistruje (když `def`) a ukáže panel pluginu.
    pub fn panel_ukaz(&mut self, id: &str, def: Option<ocs_plugin_api::dialog::PanelDef>) {
        if let Some(d) = def {
            let _ = self.pozadavek(PluginRequest::RegisterPanel(d));
        }
        let _ = self.pozadavek(PluginRequest::TogglePanel { panel_id: id.into(), show: Some(true) });
    }
    pub fn panel(&mut self, hodnoty: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>) {
        let _ = self.pozadavek(PluginRequest::UpdatePanel { panel_id: crate::panel::ID.into(), values: hodnoty });
    }
    fn pozadavek(&self, r: PluginRequest) -> Result<PluginResponse, String> {
        if !matches!(r, PluginRequest::ShowDialog(_)) {
            pockej_na_konec_prikazu();
        }
        if let Some(soubor) = std::env::var_os("TEREN_LADENI") {
            let nazev = nazev_pozadavku(&r);
            let t = std::time::Instant::now();
            let v = self.0.request(r);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(soubor) {
                use std::io::Write;
                // celý řádek jedním zápisem – záznam píše víc vláken naráz
                let radek = format!(
                    "{:.3} {nazev} {} ms\n",
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()),
                    t.elapsed().as_millis()
                );
                let _ = f.write_all(radek.as_bytes());
            }
            return match v {
                Ok(PluginResponse::Error(e)) => Err(e),
                Ok(o) => Ok(o),
                Err(e) => Err(e.to_string()),
            };
        }
        match self.0.request(r) {
            Ok(PluginResponse::Error(e)) => Err(e),
            Ok(o) => Ok(o),
            Err(e) => Err(e.to_string()),
        }
    }
    fn ok(&self, r: PluginRequest) -> bool {
        matches!(
            self.pozadavek(r),
            Ok(PluginResponse::Ok)
                | Ok(PluginResponse::Bool(true))
                | Ok(PluginResponse::Handle(_))
                | Ok(PluginResponse::OptHandle(Some(_)))
                | Ok(PluginResponse::SelectionResult(Ok(())))
                | Ok(PluginResponse::EntityTransactionResult(Ok(())))
        )
    }
}

impl PresKanal {
    /// Obyčejný krok Zpět bez skupiny – pro změny za běhu interaktivního
    /// příkazu (skupina by se s krokem příkazu pobila).
    pub fn zpet_samostatne(&mut self, n: &str) {
        let _ = self.pozadavek(PluginRequest::PushUndo { label: n.into() });
    }
    /// Číselná systémová proměnná Open CAD Studia (PDMODE…).
    pub fn promenna(&self, nazev: &str) -> Option<f64> {
        match self.pozadavek(PluginRequest::GetSystemVariable { name: nazev.into() }) {
            Ok(PluginResponse::SystemVariable(Some(ocs_plugin_api::host::HostSettingValue::Number(v)))) => Some(v),
            _ => None,
        }
    }
}

impl PresKanal {
    /// Odhad velikosti snímku výkresu ze sdílené paměti OCS (bez přenosu výkresu).
    fn odhad_velikosti(&self) -> Option<u64> {
        let tab = match self.pozadavek(PluginRequest::GetTabId).ok()? {
            PluginResponse::TabId(t) => t,
            _ => return None,
        };
        let cesta = match self.pozadavek(PluginRequest::OpenDocumentViewV4 { tab_id: tab }).ok()? {
            PluginResponse::DocumentViewV4 { path, .. } => path,
            _ => return None,
        };
        let v = ocs_plugin_api::shm::SharedDocumentReader::<ocs_plugin_api::shm::DocumentViewDataV4>::open(std::path::Path::new(&cesta))
            .ok()
            .and_then(|r| r.payload().map(|p| p.entities.iter().map(|e| e.data.len() as u64 + 16).sum::<u64>()));
        // pohled zavřít – otevřený by OCS obnovoval po každé změně
        let _ = self.pozadavek(PluginRequest::CloseDocumentViewV4 { tab_id: tab });
        v
    }
}

impl Vykres for PresKanal {
    fn dokument(&mut self) -> Result<CadDocument, String> {
        if POSLEDNI_SNIMEK.load(Poradi::Relaxed) > HLIDAT_OD {
            if let Some(v) = self.odhad_velikosti() {
                if v > LIMIT_ZPRAVY {
                    return Err(chyba_velikosti(v));
                }
            }
        }
        match self.pozadavek(PluginRequest::DocumentSnapshot)? {
            PluginResponse::Document(d) => {
                zapamatuj_velikost(&d);
                Ok(*d)
            }
            o => Err(format!("neočekávaná odpověď {o:?}")), // i18n:ne
        }
    }
    fn info(&mut self, z: &str) {
        let _ = self.pozadavek(PluginRequest::PushInfo(z.into()));
    }
    fn vystup(&mut self, z: &str) {
        let _ = self.pozadavek(PluginRequest::PushOutput(z.into()));
    }
    fn chyba(&mut self, z: &str) {
        let _ = self.pozadavek(PluginRequest::PushError(z.into()));
    }
    fn zpet(&mut self, n: &str) {
        // z pracovního vlákna jde každý požadavek zvlášť – krok Zpět se drží
        // otevřený, dokud ho `konec_kroku` nezavře (jinak by import nebo
        // přepočet šel vracet po kouscích)
        let r = if BEZ_SKUPIN.load(std::sync::atomic::Ordering::SeqCst) {
            PluginRequest::PushUndo { label: n.into() }
        } else {
            PluginRequest::BeginUndoGroup { label: n.into() }
        };
        let _ = self.pozadavek(r);
    }
    fn zacatek_kroku(&mut self, n: &str) {
        self.zpet(n);
    }
    fn konec_kroku(&mut self) {
        let _ = self.pozadavek(PluginRequest::EndUndoGroup);
    }
    fn hotovo(&mut self) {
        let _ = self.pozadavek(PluginRequest::BumpGeometry);
        let _ = self.pozadavek(PluginRequest::SetDirty);
    }
    fn zmeneno(&mut self) {
        let _ = self.pozadavek(PluginRequest::SetDirty);
    }
    fn pridej(&mut self, e: Vec<EntityType>) -> Result<Vec<Handle>, String> {
        let mut h = Vec::with_capacity(e.len());
        for d in davky(e) {
            match self.pozadavek(PluginRequest::AddEntities(d))? {
                PluginResponse::Handles(x) => h.extend(x),
                o => return Err(format!("neočekávaná odpověď {o:?}")), // i18n:ne
            }
        }
        Ok(h)
    }
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String> {
        match self.pozadavek(PluginRequest::AddEntity(e))? {
            PluginResponse::Handle(h) | PluginResponse::OptHandle(Some(h)) if h != Handle::NULL => Ok(h),
            o => Err(format!("neočekávaná odpověď {o:?}")), // i18n:ne
        }
    }
    fn uprav(&mut self, e: EntityType) -> bool {
        self.ok(PluginRequest::UpdateEntity(e))
    }
    fn odeber(&mut self, h: Handle) -> bool {
        self.ok(PluginRequest::RemoveEntity { handle: h })
    }
    fn zapis_xdata(&mut self, h: Handle, r: ExtendedDataRecord) -> bool {
        self.ok(PluginRequest::WriteRecord { handle: h, record: r })
    }
    fn tabulka(&mut self, op: TableOperation) -> Result<Handle, String> {
        match self.pozadavek(PluginRequest::TableOperation { operation: op })? {
            PluginResponse::TableResult(r) => r,
            PluginResponse::Handle(h) => Ok(h),
            o => Err(format!("neočekávaná odpověď {o:?}")), // i18n:ne
        }
    }
    fn hladina(&mut self, cfg: LayerConfig) {
        let _ = self.pozadavek(PluginRequest::AddLayer(cfg));
    }
    fn prikaz(&mut self, prikaz: &str) -> bool {
        self.ok(PluginRequest::ExecuteCommand(prikaz.into()))
    }
    fn vyber(&mut self, h: Vec<Handle>) {
        let _ = self.pozadavek(PluginRequest::SetSelection { handles: h });
    }
    fn vybrane(&mut self) -> Vec<Handle> {
        match self.pozadavek(PluginRequest::GetSelection) {
            Ok(PluginResponse::Selection(h)) | Ok(PluginResponse::Handles(h)) => h,
            _ => Vec::new(),
        }
    }
    fn transakce(&mut self, popis: &str, e: Vec<EntityType>) -> Result<(), String> {
        if e.is_empty() {
            return Ok(());
        }
        match self.pozadavek(PluginRequest::UpdateEntitiesTransaction { label: popis.into(), entities: e })? {
            PluginResponse::EntityTransactionResult(r) => r,
            PluginResponse::Ok => Ok(()),
            o => Err(format!("neočekávaná odpověď {o:?}")), // i18n:ne
        }
    }
    fn jen_zobrazeni(&mut self, h: Handle, ano: bool) -> bool {
        matches!(
            self.pozadavek(PluginRequest::SetReferenceOnly { handle: h, reference_only: ano }),
            Ok(PluginResponse::SetReferenceOnlyResult(Ok(()))) | Ok(PluginResponse::Ok)
        )
    }
}

/// Počet běžících interaktivních příkazů Terénu. Během nich Open CAD Studio
/// obsluhuje požadavky pracovního vlákna bez výkresu (a na požadavek
/// o výkres spadne) – pracovní vlákno proto počká, až příkaz skončí.
pub static PRIKAZY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Obal interaktivního příkazu, který hlídá [`PRIKAZY`].
pub struct Hlidany(Box<dyn ocs_plugin_api::host::InteractiveCommand>);

pub fn hlidany(c: Box<dyn ocs_plugin_api::host::InteractiveCommand>) -> Box<dyn ocs_plugin_api::host::InteractiveCommand> {
    PRIKAZY.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Box::new(Hlidany(c))
}

impl Drop for Hlidany {
    fn drop(&mut self) {
        PRIKAZY.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

use ocs_plugin_api::host::{CommandStep, PreviewWire};
impl ocs_plugin_api::host::InteractiveCommand for Hlidany {
    fn prompt(&self) -> String {
        self.0.prompt()
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        self.0.on_point(pt)
    }
    fn on_enter(&mut self) -> CommandStep {
        self.0.on_enter()
    }
    fn needs_object_pick(&self) -> bool {
        self.0.needs_object_pick()
    }
    fn on_object_pick(&mut self, h: Handle, pt: [f64; 3]) -> CommandStep {
        self.0.on_object_pick(h, pt)
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        self.0.on_cursor_move(pt)
    }
    fn is_selection_gathering(&self) -> bool {
        self.0.is_selection_gathering()
    }
    fn accepts_drag_selection(&self) -> bool {
        self.0.accepts_drag_selection()
    }
    fn on_selection_complete(&mut self, h: Vec<Handle>) -> CommandStep {
        self.0.on_selection_complete(h)
    }
    fn selection_rejects_locked_layers(&self) -> bool {
        self.0.selection_rejects_locked_layers()
    }
    fn modal_dialog(&self) -> Option<ocs_plugin_api::dialog::DialogDef> {
        self.0.modal_dialog()
    }
    fn on_dialog_response(&mut self, r: ocs_plugin_api::dialog::DialogResponse) -> CommandStep {
        self.0.on_dialog_response(r)
    }
    fn take_pending_panel_updates(
        &mut self,
    ) -> Vec<(String, std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>)> {
        self.0.take_pending_panel_updates()
    }
    fn on_dialog_action(&mut self, e: ocs_plugin_api::dialog::DialogEvent) -> ocs_plugin_api::dialog::DialogActionResponse {
        self.0.on_dialog_action(e)
    }
    fn on_undo_step(&mut self) -> Option<CommandStep> {
        self.0.on_undo_step()
    }
    fn input_kind(&self) -> ocs_plugin_api::host::InteractiveInputKind {
        self.0.input_kind()
    }
    fn on_text_input(&mut self, text: &str) -> CommandStep {
        self.0.on_text_input(text)
    }
}

/// Počká, až skončí interaktivní příkaz Terénu (viz [`PRIKAZY`]). Pojistka:
/// když Open CAD Studio 3 s hlásí, že žádný příkaz neběží, počítadlo se
/// vynuluje (příkaz zmizel bez uvolnění).
fn pockej_na_konec_prikazu() {
    use std::sync::atomic::Ordering;
    let mut volno_od: Option<std::time::Instant> = None;
    // tažení vlastního úchopu (nejdéle minutu – pojistka)
    let t0 = std::time::Instant::now();
    while crate::uchopy::TAHNE.load(Ordering::SeqCst) && t0.elapsed() < std::time::Duration::from_secs(60) {
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    while PRIKAZY.load(Ordering::SeqCst) > 0 {
        if BEZ_SKUPIN.load(Ordering::SeqCst) {
            volno_od = None;
        } else {
            let t = *volno_od.get_or_insert_with(std::time::Instant::now);
            if t.elapsed() > std::time::Duration::from_secs(3) {
                PRIKAZY.store(0, Ordering::SeqCst);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

#[cfg(test)]
mod testy {
    use super::*;
    use ocs_plugin_api::host::codec::entities::Text;
    use ocs_plugin_api::host::codec::types::Vector3;

    #[test]
    fn davky_se_vejdou_do_zpravy() {
        // texty po ~1 kB: 60 000 objektů ≈ 60 MB → víc dávek, žádná přes limit
        let t = "x".repeat(1000);
        let e: Vec<EntityType> = (0..60_000).map(|i| EntityType::Text(Text::with_value(format!("{i}{t}"), Vector3::new(i as f64, 0.0, 0.0)))).collect();
        let n = e.len();
        let d = davky(e);
        assert!(d.len() >= 2);
        assert_eq!(d.iter().map(Vec::len).sum::<usize>(), n);
        for x in &d {
            assert!(bincode::serialized_size(x).unwrap() <= DAVKA + 2048);
        }
        assert_eq!(davky(Vec::new()).len(), 0);
    }
}
