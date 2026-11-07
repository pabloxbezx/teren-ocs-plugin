//! Jednotný přístup k výkresu.
//!
//! Příkazy spuštěné tlačítkem dostanou `HostApi`, interaktivní příkazy
//! (klikání do výkresu) mají jen kanál pro požadavky na hostitele.
//! Obojí se tu schová za trait `Vykres`, aby se povrch dal přestavět
//! odkudkoli stejným kódem.

use teren::preklad::tr;
use teren::trf;
use ocs_plugin_api::host::{
    Document as CadDocument, EntityType, ExtendedDataRecord, Handle, HostApi, LayerConfig, PluginRequestSender, TableOperation,
};
use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};

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
        let h = self.0.add_entities(e);
        if h.len() == n { Ok(h) } else { Err(trf!("vloženo {} z {} objektů", h.len(), n)) }
    }
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String> {
        let h = self.0.add_entity(e);
        if h == Handle::NULL { Err(tr("objekt se nepodařilo vložit").into()) } else { Ok(h) }
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

/// Běží interaktivní příkaz – kroky Zpět bez skupin (skupina by se s
/// krokem příkazu pobila).
pub static BEZ_SKUPIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Výkres přes kanál požadavků (uvnitř interaktivního příkazu).
pub struct PresKanal(pub Box<dyn PluginRequestSender>);

impl PresKanal {
    fn pozadavek(&self, r: PluginRequest) -> Result<PluginResponse, String> {
        if !matches!(r, PluginRequest::ShowDialog(_)) {
            pockej_na_konec_prikazu();
        }
        if let Some(soubor) = std::env::var_os("TEREN_LADENI") {
            let nazev = format!("{r:?}");
            let nazev: String = nazev.chars().take_while(|c| c.is_alphanumeric()).collect();
            let t = std::time::Instant::now();
            let v = self.0.request(r);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(soubor) {
                use std::io::Write;
                let _ = writeln!(
                    f,
                    "{:.3} {nazev} {} ms",
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()),
                    t.elapsed().as_millis()
                );
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

impl Vykres for PresKanal {
    fn dokument(&mut self) -> Result<CadDocument, String> {
        match self.pozadavek(PluginRequest::DocumentSnapshot)? {
            PluginResponse::Document(d) => Ok(*d),
            o => Err(format!("neočekávaná odpověď {o:?}")),
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
    fn pridej(&mut self, e: Vec<EntityType>) -> Result<Vec<Handle>, String> {
        match self.pozadavek(PluginRequest::AddEntities(e))? {
            PluginResponse::Handles(h) => Ok(h),
            o => Err(format!("neočekávaná odpověď {o:?}")),
        }
    }
    fn pridej_jednu(&mut self, e: EntityType) -> Result<Handle, String> {
        match self.pozadavek(PluginRequest::AddEntity(e))? {
            PluginResponse::Handle(h) | PluginResponse::OptHandle(Some(h)) if h != Handle::NULL => Ok(h),
            o => Err(format!("neočekávaná odpověď {o:?}")),
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
            o => Err(format!("neočekávaná odpověď {o:?}")),
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
            o => Err(format!("neočekávaná odpověď {o:?}")),
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
}

/// Počká, až skončí interaktivní příkaz Terénu (viz [`PRIKAZY`]). Pojistka:
/// když Open CAD Studio 3 s hlásí, že žádný příkaz neběží, počítadlo se
/// vynuluje (příkaz zmizel bez uvolnění).
fn pockej_na_konec_prikazu() {
    use std::sync::atomic::Ordering;
    let mut volno_od: Option<std::time::Instant> = None;
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
