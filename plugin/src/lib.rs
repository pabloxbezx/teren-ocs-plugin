//! Plugin „Terén“ pro Open CAD Studio.
//!
//! Záložka Terén: COGO body a skupiny bodů, import a export, pojmenované
//! povrchy (TIN + vrstevnice) jako jeden objekt, povinné spojnice, hranice,
//! díry, objekty výkresu, styly, úpravy povrchu, Prospektor (vlastní okno),
//! automatický přepočet, kontrola zaměření.
//! Výpočty dělá knihovna `teren`; plugin čte a zapisuje výkres.

mod cogo;
mod import;
mod interaktivni;
mod nastaveni;
mod okno;
mod panel;
mod povrchy;
mod spravce;
mod styly;
mod uchopy;
mod vykres;

use teren::preklad::tr;
use teren::trf;
use std::sync::{Mutex, OnceLock};

use ocs_plugin_api::host::{BuiltinPlugin, EntityType, Handle, HostApi, HostNotification};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

use teren::body::Oddelovac;
use teren::povrch::Povrch;

use interaktivni::DruhEditace;
use povrchy::ZaznamPovrchu;
use vykres::{PresHost, Vykres};

static MANIFEST: PluginManifest = PluginManifest {
    id: "cz.fluvio.teren",
    name: "Terén",
    version: env!("CARGO_PKG_VERSION"),
    description: "Survey points, TIN surfaces, breaklines, boundaries, contours, 3D viewer, LandXML export (Czech / English)",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 60,
    xdata_apps: &[povrchy::APP_BOD, povrchy::APP_POVRCH],
    command_prefixes: &["TER_"],
};

const MERITKA: [u32; 6] = [100, 200, 250, 500, 1000, 2000];

/// Názvy objektů pluginu ve Vlastnostech (API v10).
fn tridy() -> &'static ocs_plugin_api::manifest::EntityClasses {
    static T: OnceLock<ocs_plugin_api::manifest::EntityClasses> = OnceLock::new();
    T.get_or_init(|| {
        let labels: Vec<(&'static str, &'static str)> = [
            ("BOD", "Bod zaměření"),
            ("ZNACKA", "Značka bodu"),
            ("CISLO", "Číslo bodu"),
            ("VYSKA", "Výška bodu"),
            ("POPIS", "Popis bodu"),
            ("POVRCH", "Povrch TIN"),
        ]
        .into_iter()
        .map(|(k, v)| (k, tr(v)))
        .collect();
        ocs_plugin_api::manifest::EntityClasses {
            app_id: povrchy::APP_BOD,
            class_prefix: povrchy::TRIDA,
            labels: Box::leak(labels.into_boxed_slice()),
        }
    })
}

/// Ohlásí kurzorové menu (jednou za běh; při neúspěchu zkusí znovu příště).
pub(crate) static QUAD_HOTOVO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn registruj_quad(host: &mut dyn HostApi) {
    if QUAD_HOTOVO.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let r = host.register_quad_panels(MANIFEST.id, &[quad_panel()]);
    if r.first().copied().unwrap_or(false) {
        QUAD_HOTOVO.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Kurzorové menu (quad, API v15) – záložka Terén u bodů a povrchů.
pub(crate) fn quad_panel() -> ocs_plugin_api::dialog::QuadPanelDef {
    use ocs_plugin_api::dialog::{QuadActionDef, QuadItemDef, QuadPanelDef};
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;
    let p = |id: &str, label: &str, svg: &[u8]| QuadItemDef {
        control_id: id.to_ascii_lowercase(),
        label: label.into(),
        icon: OwnedIconKind::Svg(svg.to_vec()),
        action: QuadActionDef::Command(id.into()),
    };
    let mut q = QuadPanelDef::new("teren", tr("Terén")).with_applies_to(
        ["Point", "Insert", "Text", "Line", "LwPolyline", "Circle"].iter().map(|s| s.to_string()).collect(),
    );
    q.items = vec![
        p("TER_PANEL", tr("Vlastnosti bodu"), include_bytes!("../assets/ter_vlastnosti_bodu.svg")),
        p("TER_UCHOPY", tr("Posun bodů úchopem"), include_bytes!("../assets/ter_posun_bodu.svg")),
        p("TER_PRIDAT_BOD", tr("Přidat bod do povrchu"), include_bytes!("../assets/ter_pridat_bod.svg")),
        p("TER_PREPOCITAT", tr("Přepočítat povrch"), include_bytes!("../assets/ter_prepocitat.svg")),
        p("TER_PROSPEKTOR", tr("Prospektor"), include_bytes!("../assets/ter_prospektor.svg")),
    ];
    q
}

// ── Pás karet ───────────────────────────────────────────────────────────────

macro_rules! ikona {
    ($n:literal) => {
        IconKind::Svg(include_bytes!(concat!("../assets/", $n, ".svg")))
    };
}

fn tool(id: &'static str, label: &'static str, icon: IconKind) -> ToolDef {
    ToolDef { id, label, icon, event: ModuleEvent::Command(id.to_string()) }
}

struct TerenModule;

impl CadModule for TerenModule {
    fn id(&self) -> &'static str {
        "teren"
    }
    fn title(&self) -> &'static str {
        tr("Terén")
    }
    fn ribbon_groups(&self) -> &[RibbonGroup] {
        // Pořadí skupin podle práce: Prospektor (začátek) → Body → Povrch →
        // Definice → Úpravy → Výstupy. Nabídky mají výchozí položku na prvním
        // místě – OCS před první volbou kreslí na tlačítku první položku.
        static G: OnceLock<Vec<RibbonGroup>> = OnceLock::new();
        G.get_or_init(|| {
            let styl = ikona!("ter_styl");
            let sb = ikona!("ter_styl_bodu");
            let mut klik = vec![
                ("TER_POVRCH_KLIK_VYP", tr("Povrch: klik projde na body"), ikona!("ter_povrch_klik_vyp")),
                ("TER_POVRCH_KLIK_ZAP", tr("Povrch: vybírat klikem"), ikona!("ter_povrch_klik_zap")),
            ];
            if !nastaveni::akt().povrch_nevybirat {
                klik.reverse();
            }
            let klik_default = klik[0].0;
            vec![
                RibbonGroup {
                    title: tr("Prospektor"),
                    tools: vec![
                        RibbonItem::LargeTool(tool("TER_PROSPEKTOR", tr("Prospektor"), ikona!("ter_prospektor"))),
                        RibbonItem::LargeTool(ToolDef {
                            id: "TER_IMPORT",
                            label: tr("Import bodů"),
                            icon: ikona!("ter_body_import"),
                            event: ModuleEvent::PluginFileDialog {
                                command: "TER_IMPORT".into(),
                                title: tr("Import bodů").into(),
                                filter_name: tr("Body (TXT, CSV, XYZ, XLSX, XML, GSI)").into(),
                                extensions: ["txt", "csv", "xyz", "xlsx", "xls", "ods", "xml", "jxl", "gsi", "*"].iter().map(|p| p.to_string()).collect(),
                            },
                        }),
                        RibbonItem::LabeledDropdown {
                            id: "TER_DD_SABLONA",
                            label: tr("Šablona výkresu"),
                            icon: ikona!("ter_sablona"),
                            items: vec![
                                ("TER_PRIPRAVIT", tr("Připravit výkres ze šablony"), ikona!("ter_sablona")),
                                ("TER_SABLONA_ULOZ", tr("Uložit tento výkres jako šablonu"), ikona!("ter_sablona_ulozit")),
                            ],
                            default: "TER_PRIPRAVIT",
                        },
                        RibbonItem::LabeledTool(tool("TER_NAPOVEDA", tr("Nápověda"), ikona!("ter_napoveda"))),
                    ],
                },
                RibbonGroup {
                    // ne „Body“ – OCS by ho přeložil jako anglické „Body“ (Těleso)
                    title: tr("Body zaměření"),
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "TER_DD_BOD",
                            label: tr("Nový bod"),
                            icon: ikona!("ter_bod_novy"),
                            items: vec![
                                ("TER_BOD", tr("Ruční bod (klik nebo x,y,z)"), ikona!("ter_bod_novy")),
                                ("TER_BOD_POVRCH", tr("Bod na povrchu"), ikona!("ter_bod_na_povrchu")),
                            ],
                            default: "TER_BOD",
                        },
                        RibbonItem::LabeledTool(tool("TER_PANEL", tr("Vlastnosti bodu"), ikona!("ter_vlastnosti_bodu"))),
                        RibbonItem::LabeledDropdown {
                            id: "TER_DD_STYL_BODU",
                            label: tr("Styl bodu"),
                            icon: sb,
                            items: vec![
                                ("TER_SB_STANDARD", tr("Standard (číslo a výška)"), ikona!("ter_sb_standard")),
                                ("TER_SB_SKUPINA", tr("Podle skupiny / kódu"), ikona!("ter_sb_skupina")),
                                ("TER_SB_KRIZ_VYSKA", tr("Křížek s výškou"), ikona!("ter_sb_kriz_vyska")),
                                ("TER_SB_KRIZ_RADEK", tr("Křížek, číslo a výška vedle sebe"), ikona!("ter_sb_kriz_radek")),
                                ("TER_SB_JEN_ZNACKA", tr("Jen značka"), ikona!("ter_sb_jen_znacka")),
                                ("TER_SB_STROM_L", tr("Strom listnatý"), ikona!("ter_sb_strom_l")),
                                ("TER_SB_STROM_J", tr("Strom jehličnatý"), ikona!("ter_sb_strom_j")),
                                ("TER_SB_KER", tr("Keř"), ikona!("ter_sb_ker")),
                                ("TER_SB_SLOUP", tr("Sloup"), ikona!("ter_sb_sloup")),
                                ("TER_SB_HYDRANT", tr("Hydrant"), ikona!("ter_sb_hydrant")),
                                ("TER_SB_SACHTA", tr("Šachta"), ikona!("ter_sb_sachta")),
                                ("TER_SB_VPUST", tr("Vpusť"), ikona!("ter_sb_vpust")),
                                ("TER_SB_SOUPE", tr("Šoupě"), ikona!("ter_sb_soupe")),
                                ("TER_SB_SKRYT", tr("Nezobrazovat"), ikona!("ter_sb_skryt")),
                                ("TER_SB_DALSI", tr("Další styly… (Prospektor)"), sb),
                            ],
                            default: "TER_SB_STANDARD",
                        },
                        // přepínač – prázdný popisek: tlačítko ukáže název a ikonu zvoleného stavu
                        RibbonItem::LabeledDropdown {
                            id: "TER_DD_UCHOPY",
                            label: "",
                            icon: ikona!("ter_posun_bodu_vyp"),
                            items: vec![
                                ("TER_UCHOPY_VYP", tr("Posun bodů: vypnuto"), ikona!("ter_posun_bodu_vyp")),
                                ("TER_UCHOPY_ZAP", tr("Posun bodů: zapnuto"), ikona!("ter_posun_bodu")),
                            ],
                            default: "TER_UCHOPY_VYP",
                        },
                        RibbonItem::LabeledTool(tool("TER_SROVNAT", tr("Srovnat body"), ikona!("ter_srovnat_body"))),
                        RibbonItem::LabeledTool(tool("TER_POPISY_NA_MISTO", tr("Popisy na místo"), ikona!("ter_popisy_na_misto"))),
                        RibbonItem::LabeledTool(tool("TER_SKUPINA_Z_VYBERU", tr("Skupina z výběru"), ikona!("ter_skupina_bodu"))),
                    ],
                },
                RibbonGroup {
                    title: tr("Povrch"),
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "TER_DD_NOVY",
                            label: tr("Nový povrch"),
                            icon: ikona!("ter_povrch_novy"),
                            items: vec![
                                ("TER_NOVY_STAV", tr("Stávající terén (z bodů)"), ikona!("ter_povrch_novy")),
                                ("TER_NOVY_NAVRH", tr("Návrh (z vybraných 3D čar)"), ikona!("ter_povrch_navrh")),
                                ("TER_NOVY_KLIK", tr("Z naklikaných bodů"), ikona!("ter_povrch_z_bodu")),
                                ("TER_NOVY", tr("Další povrch z bodů (jiné jméno)"), ikona!("ter_povrch_novy")),
                                ("TER_PRUVODCE", tr("Průvodce novým povrchem…"), ikona!("ter_prospektor")),
                            ],
                            default: "TER_NOVY_STAV",
                        },
                        RibbonItem::LargeDropdown {
                            id: "TER_DD_STYL",
                            label: tr("Styl povrchu"),
                            icon: styl,
                            items: vec![
                                ("TER_SP_VRSTP", tr("Vrstevnice s popisem"), ikona!("ter_vrstevnice")),
                                ("TER_SP_VRST", tr("Vrstevnice"), ikona!("ter_vrstevnice")),
                                ("TER_SP_TINVRST", tr("Trojúhelníky + vrstevnice"), ikona!("ter_sp_tin_vrst")),
                                ("TER_SP_TIN", tr("Trojúhelníky"), ikona!("ter_sp_tin")),
                                ("TER_SP_OBRYS", tr("Obrys"), ikona!("ter_hranice")),
                                ("TER_SP_SPAD", tr("Spád (šipky)"), ikona!("ter_sp_spad")),
                                ("TER_SP_SKRYT", tr("Nezobrazovat"), ikona!("ter_sp_skryt")),
                                ("TER_SP_DALSI", tr("Další styly… (Prospektor)"), styl),
                            ],
                            default: "TER_SP_VRSTP",
                        },
                        RibbonItem::LabeledTool(tool("TER_PREPOCITAT", tr("Přepočítat povrch"), ikona!("ter_prepocitat"))),
                        RibbonItem::LabeledTool(tool("TER_NAJIT", tr("Ukázat v Prospektoru"), ikona!("ter_ukazat_v_prospektoru"))),
                        RibbonItem::LabeledDropdown {
                            id: "TER_DD_KLIK",
                            label: "",
                            icon: ikona!("ter_povrch_klik_vyp"),
                            items: klik,
                            default: klik_default,
                        },
                    ],
                },
                RibbonGroup {
                    title: tr("Definice povrchu"),
                    tools: vec![
                        RibbonItem::LargeTool(tool("TER_SPOJNICE", tr("Spojnice přes body"), ikona!("ter_spojnice_body"))),
                        RibbonItem::LabeledTool(tool("TER_PRIDAT_SPOJNICI", tr("Přidat spojnici"), ikona!("ter_pridat_spojnici"))),
                        RibbonItem::LabeledTool(tool("TER_HRANICE", tr("Hranice"), ikona!("ter_hranice"))),
                        RibbonItem::LabeledTool(tool("TER_DIRA", tr("Díra"), ikona!("ter_dira"))),
                    ],
                },
                RibbonGroup {
                    title: tr("Úpravy trojúhelníků"),
                    tools: vec![
                        RibbonItem::LargeTool(tool("TER_OTOCIT", tr("Otočit hranu"), ikona!("ter_otocit_hranu"))),
                        RibbonItem::LabeledTool(tool("TER_PRIDAT_HRANU", tr("Přidat hranu"), ikona!("ter_zlomova_hrana"))),
                        RibbonItem::LabeledTool(tool("TER_SMAZAT_HRANU", tr("Smazat hranu"), ikona!("ter_smazat_hranu"))),
                        RibbonItem::LabeledTool(tool("TER_SMAZAT_TROJ", tr("Smazat trojúhelník"), ikona!("ter_smazat_troj"))),
                        RibbonItem::LabeledTool(tool("TER_PRIDAT_BOD", tr("Přidat bod do povrchu"), ikona!("ter_pridat_bod"))),
                        RibbonItem::LabeledTool(tool("TER_ODEBRAT_BOD", tr("Odebrat bod z povrchu"), ikona!("ter_odebrat_bod"))),
                        RibbonItem::LabeledTool(tool("TER_ZRUSIT_UPRAVY", tr("Zrušit úpravy povrchu"), ikona!("ter_zrusit_upravy"))),
                    ],
                },
                RibbonGroup {
                    title: tr("Výstupy"),
                    tools: vec![
                        RibbonItem::LargeTool(tool("TER_EXPORT_POVRCHU", tr("Export povrchu"), ikona!("ter_export_povrchu"))),
                        RibbonItem::LargeTool(tool("TER_3D", tr("Prohlížeč 3D"), ikona!("ter_3d"))),
                        RibbonItem::LabeledTool(tool("TER_EXPORT_BODU", tr("Export bodů"), ikona!("ter_export_bodu"))),
                        RibbonItem::LabeledTool(tool("TER_VYSKA", tr("Výška v bodě"), ikona!("ter_vyska_v_bode"))),
                        RibbonItem::LabeledTool(tool("TER_KONTROLA", tr("Kontrola zaměření"), ikona!("ter_kontrola"))),
                    ],
                },
            ]
        })
    }
}

// ── Pomocné ─────────────────────────────────────────────────────────────────

fn aktivni(v: &mut dyn Vykres) -> Option<(ZaznamPovrchu, ocs_plugin_api::host::Document)> {
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => {
            v.chyba(&trf!("Terén: {}", e));
            return None;
        }
    };
    match povrchy::aktivni(&doc) {
        Ok(z) => Some((z, doc)),
        Err(e) => {
            v.chyba(&trf!("Terén: {}", e));
            None
        }
    }
}

fn prestav_a_vypis(v: &mut dyn Vykres, z: &mut ZaznamPovrchu, popis: &str) -> Option<Povrch> {
    v.zpet(popis);
    let r = match povrchy::prestav(v, z) {
        Ok(r) => r,
        Err(e) => {
            v.chyba(&trf!("Terén: {}", e));
            return None;
        }
    };
    povrchy::vypis(v, z, &r);
    v.hotovo();
    spravce::posli(spravce::Akce::Obnovit);
    Some(r.povrch)
}

fn novy_povrch(host: &mut dyn HostApi, zaklad: &str, jmeno: &str, body: bool) {
    let vyber = host.selection();
    let mut v = PresHost(host);
    novy_povrch_v(&mut v, &vyber, zaklad, jmeno, body);
}

/// Nový povrch z bodů zaměření (`body`) nebo z vybraných 3D čar (návrh).
pub(crate) fn novy_povrch_v(v: &mut dyn vykres::Vykres, vyber: &[Handle], zaklad: &str, jmeno: &str, body: bool) {
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    if body && povrchy::body_z_vykresu(&doc).len() < 3 {
        return v.chyba(tr("Terén: ve výkrese nejsou body zaměření. Nejdřív importujte body (Import bodů)."));
    }
    let cary: Vec<_> = doc
        .model_space_entities()
        .filter(|e| vyber.contains(&e.common().handle))
        .filter(|e| matches!(e, EntityType::Polyline3D(_) | EntityType::LwPolyline(_) | EntityType::Line(_)))
        .cloned()
        .collect();
    if !body && cary.is_empty() {
        return v.info(
            tr("Terén: návrhový povrch vzniká z 3D čar (hrany, paty a koruny svahů, osy). \
             Vyberte je ve výkrese a zvolte Nový povrch → Návrh znovu."),
        );
    }
    let id = povrchy::volne_id(&doc, zaklad);
    let poradi = if id == zaklad { String::new() } else { format!(" {}", &id[zaklad.len()..]) };
    let mut z = ZaznamPovrchu::novy(id, format!("{jmeno}{poradi}"), body);
    v.zpet(tr("Nový povrch"));
    if body {
        // povrch z bodů zaměření – skupina „Zaměření“ (všechny body kromě ručně vytvořených)
        let zamereni = match cogo::ulozene_skupiny(&doc).into_iter().map(|(_, s)| s).find(|s| s.id == cogo::ZAMERENI) {
            Some(s) => s,
            None => {
                let s = cogo::skupina_zamereni();
                if let Err(e) = cogo::uloz_skupinu(v, &doc, &s) {
                    return v.chyba(&trf!("Terén: {}", e));
                }
                s
            }
        };
        z.skupiny = vec![povrchy::SkupinaVPovrchu { id: zamereni.id, zapnuto: true }];
    }
    if !body {
        let hl = z.hladiny().spojnice;
        povrchy::zajisti_hladinu(v, &doc, &hl, 1);
        for e in cary {
            v.uprav(povrchy::prirad_caru(e, &hl));
        }
    }
    match povrchy::prestav(v, &mut z) {
        Ok(r) => {
            povrchy::vypis(v, &z, &r);
            v.hotovo();
            spravce::posli(spravce::Akce::Obnovit);
        }
        Err(e) => v.chyba(&trf!("Terén: {}", e)),
    }
}

fn nastav_povrch(host: &mut dyn HostApi, popis: &str, f: impl FnOnce(&mut ZaznamPovrchu)) {
    let mut v = PresHost(host);
    let Some((mut z, _)) = aktivni(&mut v) else { return };
    f(&mut z);
    prestav_a_vypis(&mut v, &mut z, popis);
}

fn spust_editaci(host: &mut dyn HostApi, druh: DruhEditace) {
    let mut v = PresHost(host);
    let Some((z, doc)) = aktivni(&mut v) else { return };
    let def = povrchy::definice(&doc, &z).definice;
    let povrch = match Povrch::z_definice(&def, 0.001) {
        Ok(p) => p,
        Err(e) => return host.push_error(&trf!("Terén: {}", e)),
    };
    host.push_info(&trf!("Terén: úpravy povrchu „{}“ – každý klik se hned projeví ve výkrese, Enter = konec.", z.jmeno));
    spust(host, Box::new(interaktivni::EditacePovrchu::new(z, def, povrch, druh)));
}

#[derive(Clone, Copy, PartialEq)]
enum DruhCary {
    Spojnice,
    Hranice,
    Dira,
}

/// Co nástroj dělá, co potřebuje a jak se ovládá (pás karet popisky neumí).
const POPISY_NASTROJU: &[(&str, &str, &str)] = &[
    ("TER_SPOJNICE", "Spojnice přes body – nakreslí lomovou hranu (3D křivku) klikáním na body zaměření; trojúhelníky ji nepřekříží.", "klikejte na body · Backspace = zpět · Enter = konec"),
    ("TER_PRIDAT_SPOJNICI", "Přidat spojnici – vybrané 3D křivky se stanou lomovými hranami aktivního povrchu.", "nejdřív vyberte čáry, pak klikněte"),
    ("TER_HRANICE", "Hranice – vybraná uzavřená křivka ořízne aktivní povrch.", "nejdřív vyberte křivku, pak klikněte"),
    ("TER_DIRA", "Díra – uvnitř vybrané uzavřené křivky se povrch vynechá.", "nejdřív vyberte křivku, pak klikněte"),
    ("TER_OTOCIT", "Otočit hranu – prohodí společnou hranu dvou trojúhelníků aktivního povrchu.", "klikejte na hrany · Enter = konec"),
    ("TER_PRIDAT_HRANU", "Přidat hranu – vynutí hranu mezi dvěma vrcholy aktivního povrchu.", "klikněte na dva vrcholy · Enter = konec"),
    ("TER_SMAZAT_HRANU", "Smazat hranu – odstraní hranu na okraji povrchu (zúží povrch).", "klikejte na hrany · Enter = konec"),
    ("TER_SMAZAT_TROJ", "Smazat trojúhelník – vynechá trojúhelník z aktivního povrchu.", "klikejte do trojúhelníků · Enter = konec"),
    ("TER_PRIDAT_BOD", "Přidat bod – přidá do aktivního povrchu bod (výšku zadáte v okně, návrh z povrchu).", "klikejte nebo zadejte x,y,z · Enter = konec"),
    ("TER_ODEBRAT_BOD", "Odebrat bod – vynechá bod z aktivního povrchu (bod ve výkrese zůstane).", "klikejte na vrcholy · Enter = konec"),
    ("TER_VYSKA", "Výška v bodě – ukáže a popíše výšku aktivního povrchu v místě kliknutí.", "klikejte · Enter = konec"),
    ("TER_BOD", "Ruční bod – vytvoří bod zaměření s dalším volným číslem.", "klikněte nebo zadejte x,y, výšku pak zadáte v okně · Enter = konec"),
    ("TER_BOD_POVRCH", "Bod na povrchu – vytvoří bod s výškou z aktivního povrchu.", "klikejte · Enter = konec"),
    ("TER_NOVY_KLIK", "Povrch z naklikaných bodů – z bodů, na které kliknete, vznikne nová skupina a povrch.", "klikejte na body · Enter = vytvořit"),
];

/// Při prvním spuštění nástroje v této relaci vypíše, co dělá.
fn uvod_nastroje(host: &mut dyn HostApi, cmd: &str) {
    static UKAZANE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Some(&(id, popis, ovladani)) = POPISY_NASTROJU.iter().find(|p| p.0 == cmd) else { return };
    let mut u = UKAZANE.lock().unwrap_or_else(|e| e.into_inner());
    if !u.contains(&id) {
        u.push(id);
        host.push_info(&trf!("Terén: {} ({}; nápověda: tlačítko Nápověda)", tr(popis), tr(ovladani)));
    }
}

/// Přiřadí vybrané čáry k aktivnímu povrchu (přesun na hladinu povrchu) a povrch přepočítá.
fn prirad_vyber(host: &mut dyn HostApi, druh: DruhCary) {
    let vyber = host.selection();
    let nazev = match druh {
        DruhCary::Spojnice => tr("spojnice"),
        DruhCary::Hranice => tr("hranice"),
        DruhCary::Dira => tr("díry"),
    };
    let mut v = PresHost(host);
    let Some((mut z, doc)) = aktivni(&mut v) else { return };
    if vyber.is_empty() {
        let rada = match druh {
            DruhCary::Spojnice => {
                tr("Vyberte ve výkrese 3D křivky (hrany, paty a koruny svahů) a klikněte znovu na Přidat spojnici. \
                 Spojnici přes body zaměření naklikáte nástrojem Spojnice přes body.")
            }
            DruhCary::Hranice => tr("Nakreslete nebo vyberte uzavřenou křivku kolem území a klikněte znovu na Hranice."),
            DruhCary::Dira => tr("Vyberte uzavřenou křivku kolem místa, které má v povrchu chybět, a klikněte znovu na Díra."),
        };
        return v.info(&trf!("Terén: pro {} povrchu „{}“ nejsou vybrané žádné čáry. {}", nazev, z.jmeno, rada));
    }
    let h = z.hladiny();
    let (cil, barva) = match druh {
        DruhCary::Spojnice => (h.spojnice, 1),
        DruhCary::Hranice => (h.hranice, 5),
        DruhCary::Dira => (h.diry, 5),
    };
    povrchy::zajisti_hladinu(&mut v, &doc, &cil, barva);
    v.zpet(tr("Přiřadit čáry k povrchu"));
    let (mut ok, mut jine) = (0, 0);
    for e in doc.model_space_entities().filter(|e| vyber.contains(&e.common().handle)) {
        let vhodna = match e {
            EntityType::Polyline3D(_) | EntityType::LwPolyline(_) => true,
            EntityType::Line(_) => druh == DruhCary::Spojnice,
            _ => false,
        };
        if !vhodna {
            jine += 1;
            continue;
        }
        if v.uprav(povrchy::prirad_caru(e.clone(), &cil)) {
            ok += 1;
        }
    }
    if jine > 0 {
        v.vystup(&trf!("  {} vybraných objektů nejsou vhodné čáry – vynechány", jine));
    }
    if ok > 0 {
        v.vystup(&trf!("  {} čar přidáno jako {} povrchu „{}“", ok, nazev, z.jmeno));
        prestav_a_vypis(&mut v, &mut z, tr("Přepočítat povrch"));
    }
}

/// Vrátí vybrané čáry z povrchů na původní hladiny a dotčené povrchy přepočítá.
fn odebrat_z_povrchu(host: &mut dyn HostApi) {
    let vyber = host.selection();
    let mut v = PresHost(host);
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    if vyber.is_empty() {
        return v.info(tr("Terén: vyberte spojnice, hranice nebo díry, které mají přestat patřit k povrchu, a klikněte znovu."));
    }
    let vse = povrchy::povrchy(&doc);
    let mut dotcene: Vec<ZaznamPovrchu> = Vec::new();
    v.zpet(tr("Odebrat z povrchu"));
    let mut pocet = 0;
    for e in doc.model_space_entities().filter(|e| vyber.contains(&e.common().handle)) {
        let vrstva = e.common().layer.to_uppercase();
        let Some(z) = vse.iter().find(|z| {
            let h = z.hladiny();
            [h.spojnice, h.hranice, h.diry].iter().any(|x| x.to_uppercase() == vrstva)
        }) else {
            continue;
        };
        if v.uprav(povrchy::uvolni_caru(e.clone())) {
            pocet += 1;
            if !dotcene.iter().any(|d| d.id == z.id) {
                dotcene.push(z.clone());
            }
        }
    }
    if pocet == 0 {
        return v.info(tr("Terén: ve výběru není žádná čára patřící k povrchu."));
    }
    v.vystup(&trf!("  {} čar odebráno z definice povrchů", pocet));
    for mut z in dotcene {
        prestav_a_vypis(&mut v, &mut z, tr("Přepočítat povrch"));
    }
}

fn vybrat(host: &mut dyn HostApi) {
    let vyber = host.selection();
    let mut v = PresHost(host);
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    let vse = povrchy::povrchy(&doc);
    if vse.is_empty() {
        return v.info(tr("Terén: ve výkrese není žádný povrch."));
    }
    let mut objekty = Vec::new();
    for z in &vse {
        objekty.push((z.insert, z.id.clone(), z.jmeno.clone()));
        let pref = format!("{}_", z.hladiny().povrch.to_uppercase());
        for e in doc.model_space_entities() {
            if e.common().layer.to_uppercase().starts_with(&pref) {
                objekty.push((e.common().handle, z.id.clone(), z.jmeno.clone()));
            }
        }
    }
    if let Some((_, id, jmeno)) = objekty.iter().find(|(h, _, _)| vyber.contains(h)) {
        povrchy::nastav_aktivni(id);
        spravce::skoc(spravce::Skok::Povrch(id.clone()));
        spravce::posli(spravce::Akce::Obnovit);
        return v.info(&trf!("Terén: aktivní povrch je „{}“.", jmeno));
    }
    spust(host, Box::new(interaktivni::VybratPovrch { objekty, vybrano: None, zprava: String::new() }));
}

/// Akce Prospektoru zapsaná textem (příkaz `TER_AKCE`) – pro skripty a zkoušení.
fn akce_z_textu(t: &str) -> Result<spravce::Akce, String> {
    use ocs_plugin_api::host::Handle;
    use spravce::Akce;
    let slova: Vec<&str> = t.split_whitespace().collect();
    let co = slova.first().map(|s| s.to_uppercase()).unwrap_or_default();
    let arg = |i: usize| slova.get(i).copied().ok_or_else(|| trf!("{}: chybí parametr {}", co, i));
    let zbytek = |i: usize| slova.get(i..).map(|s| s.join(" ")).unwrap_or_default();
    let h = |s: &str| u64::from_str_radix(s, 16).map(Handle::new).map_err(|_| trf!("neplatný handle {}", s));
    let hs = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(h).collect::<Result<Vec<_>, _>>();
    let ano = |s: &str| s == "1";
    Ok(match co.as_str() {
        "SKUPINA" => {
            let radky: Vec<String> = zbytek(1).split(';').map(|x| x.trim().to_string()).collect();
            Akce::SkupinaUlozit(teren::skupiny::SkupinaBodu::precti(&radky).ok_or(tr("SKUPINA: chybí id=…"))?)
        }
        "SKUPINA_NOVA" => Akce::SkupinaNova(slova.get(1).map(|x| hs(x)).transpose()?.unwrap_or_default()),
        "SKUPINA_SMAZAT" => Akce::SkupinaSmazat(arg(1)?.into()),
        "SKUPINA_BODY" => Akce::SkupinaBody(arg(1)?.into(), hs(arg(2)?)?, ano(arg(3)?)),
        "SKUPINA_VYBER" => Akce::SkupinaVyberVykresu(arg(1)?.into(), ano(arg(2)?)),
        "POVRCH_ZE_SKUPINY" => {
            let j = zbytek(2);
            Akce::PovrchZeSkupiny(arg(1)?.into(), (!j.is_empty()).then_some(j), false)
        }
        "SKUPINA_V_POVRCHU" => {
            let stav = match arg(3)? {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            };
            Akce::SkupinaVPovrchu(arg(1)?.into(), arg(2)?.into(), stav)
        }
        "OBJEKTY" => Akce::PridatObjekty(
            arg(1)?.into(),
            povrchy::TypObjektu::z_kodu(arg(2)?).ok_or(tr("typ objektu P, B, C nebo F"))?,
            ano(arg(3)?),
        ),
        "POLOZKA" => Akce::PolozkaZapnout(arg(1)?.into(), h(arg(2)?)?, ano(arg(3)?)),
        "ODEBRAT_POLOZKU" => Akce::OdebratPolozku(arg(1)?.into(), h(arg(2)?)?),
        "BOD" => {
            let doc_h = h(arg(1)?)?;
            let z: f64 = arg(3)?.replace(',', ".").parse().map_err(|_| "neplatná výška")?;
            Akce::BodUpravit(doc_h, teren::body::Bod { cislo: arg(2)?.into(), x: f64::NAN, y: f64::NAN, z, popis: zbytek(4) })
        }
        "PRECISLOVAT" => Akce::BodyPrecislovat(
            hs(arg(3)?)?,
            arg(1)?.parse().map_err(|_| "neplatné číslo")?,
            arg(2)?.parse().map_err(|_| "neplatný krok")?,
        ),
        "SMAZAT_BODY" => Akce::BodySmazat(hs(arg(1)?)?),
        "EXPORT" => Akce::Export(arg(1)?.into(), zbytek(3), arg(2)?.to_uppercase(), Oddelovac::Mezera, true),
        "POPISY" => Akce::ObnovitPopisy,
        "PREPOCITAT" => Akce::Prepocitat(slova[1..].iter().map(|s| s.to_string()).collect()),
        "NAJIT" => Akce::VyberZVykresu,
        "STYL_BODU" => Akce::StylBodu(hs(arg(1)?)?, slova.get(2).map(|s| s.to_string()).unwrap_or_default()),
        "ULOZ_STYL_BODU" | "ULOZ_STYL_POVRCHU" => {
            let radky: Vec<String> = zbytek(1).split(';').map(|x| x.trim().to_string()).collect();
            let doc_bod = co == "ULOZ_STYL_BODU";
            let id = radky.iter().find_map(|r| r.strip_prefix("id=")).unwrap_or("").to_string();
            if doc_bod {
                let mut st = styly::StylBodu::vestavene().into_iter().find(|s| s.id == id).unwrap_or_else(|| styly::StylBodu::vestavene().remove(0));
                st.id = id;
                Akce::UlozStylBodu(styly::StylBodu::uprav(st, &radky))
            } else {
                let st = styly::StylPovrchu::vestavene().into_iter().find(|s| s.id == id).unwrap_or_else(|| styly::StylPovrchu::vlastni(&id, &id));
                Akce::UlozStylPovrchu(styly::StylPovrchu::uprav(st, &radky))
            }
        }
        "SMAZ_STYL" => Akce::SmazStyl(arg(1)?.into()),
        "STYL_POVRCHU" => Akce::Nastav(vec![arg(1)?.into()], spravce::Zmena::Styl(arg(2)?.into())),
        "SABLONA_ULOZ" => Akce::SablonaUloz,
        "SABLONA_NACTI" => Akce::SablonaNacti,
        "SMAZ_KONTROLU" => Akce::SmazatKontrolu,
        "SROVNAT" => Akce::SrovnatBody(slova.get(1) == Some(&"?")),
        _ => return Err(trf!("neznámá akce {}", co)),
    })
}

/// Najde vybrané objekty v Prospektoru (bod, položku definice, povrch).
fn najit(host: &mut dyn HostApi) {
    let vyber = host.selection();
    let mut v = PresHost(host);
    if vyber.is_empty() {
        return v.info(tr("Terén: vyberte ve výkrese bod, čáru nebo trojúhelník povrchu a klikněte znovu – Prospektor na něj skočí."));
    }
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    match spravce::najdi_ve_strome(&doc, &vyber) {
        Some((skok, popis)) => {
            if let spravce::Skok::Povrch(id) | spravce::Skok::Polozka(id, _) = &skok {
                povrchy::nastav_aktivni(id);
            }
            spravce::skoc(skok);
            okno::otevri();
            spravce::posli(spravce::Akce::Obnovit);
            v.info(&trf!("Terén: {} – zobrazeno v Prospektoru.", popis));
        }
        None => v.info(tr("Terén: vybrané objekty nepatří k žádnému bodu ani povrchu.")),
    }
}

/// Styl bodu z pásu karet pro vybrané body (stačí vybrat značku či popis).
fn styl_bodu_vyberu(host: &mut dyn HostApi, id: &str) {
    if id == "DALSI" {
        spravce::skoc(spravce::Skok::StylyBodu);
        okno::otevri();
        spravce::posli(spravce::Akce::Obnovit);
        return;
    }
    let vyber = host.selection();
    let mut v = PresHost(host);
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    let body = cogo::body_z_vyberu(&doc, &vyber);
    if body.is_empty() {
        return v.info(tr("Terén: nejdřív vyberte body ve výkrese (stačí kliknout na značku nebo popis) a pak zvolte styl."));
    }
    let styl = if id == "SKUPINA" { String::new() } else { id.to_string() };
    match spravce::proved_hned(&mut v, spravce::Akce::StylBodu(body.clone(), styl)) {
        Ok(Some(t)) => v.info(&trf!("Terén: {}", t)),
        Ok(None) => {}
        Err(e) => v.chyba(&trf!("Terén: {}", e)),
    }
    spravce::obnov_vyber(&mut v, &body, &[]);
    spravce::posli(spravce::Akce::Obnovit);
}

/// Styl povrchu z pásu karet pro vybrané povrchy (jinak pro aktivní).
fn styl_povrchu_vyberu(host: &mut dyn HostApi, id: &str) {
    if id == "DALSI" {
        spravce::skoc(spravce::Skok::StylyPovrchu);
        okno::otevri();
        spravce::posli(spravce::Akce::Obnovit);
        return;
    }
    let vyber = host.selection();
    let mut v = PresHost(host);
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    let mut ids = povrchy::povrchy_z_vyberu(&doc, &vyber);
    if ids.is_empty() {
        match povrchy::aktivni(&doc) {
            Ok(z) => ids.push(z.id),
            Err(e) => return v.chyba(&trf!("Terén: {}", e)),
        }
    }
    let vybrane = !vyber.is_empty();
    match spravce::proved_hned(&mut v, spravce::Akce::Nastav(ids.clone(), spravce::Zmena::Styl(id.to_string()))) {
        Ok(Some(t)) => v.info(&trf!("Terén: {}", t)),
        Ok(None) => {}
        Err(e) => v.chyba(&trf!("Terén: {}", e)),
    }
    if vybrane {
        spravce::obnov_vyber(&mut v, &[], &ids);
    }
    spravce::posli(spravce::Akce::Obnovit);
}

/// Nové body: ruční, na povrchu, nebo body pro nový povrch.
fn novy_bod(host: &mut dyn HostApi, druh: interaktivni::DruhBodu) {
    use interaktivni::DruhBodu;
    let n = nastaveni::akt();
    let mut v = PresHost(host);
    if let Err(e) = cogo::priprav_vykres(&mut v) {
        return v.chyba(&trf!("Terén: {}", e));
    }
    let doc = match v.dokument() {
        Ok(d) => d,
        Err(e) => return v.chyba(&trf!("Terén: {}", e)),
    };
    let povrch = povrchy::aktivni(&doc).ok().and_then(|z| {
        let def = povrchy::definice(&doc, &z).definice;
        Povrch::z_definice(&def, 0.001).ok().map(|p| (z.jmeno.clone(), p))
    });
    if druh == DruhBodu::NaPovrchu && povrch.is_none() {
        return v.chyba(tr("Terén: bod na povrchu potřebuje aktivní povrch – vytvořte ho nebo vyberte."));
    }
    let (skupina, jmeno_povrchu) = if druh == DruhBodu::ProPovrch {
        let jmeno = {
            let vse = povrchy::povrchy(&doc);
            (1..).map(|i| trf!("Plocha {}", i)).find(|j| !vse.iter().any(|z| z.jmeno.eq_ignore_ascii_case(j))).unwrap()
        };
        let jmeno_skupiny = cogo::volne_jmeno_skupiny(&doc, &trf!("Body {}", jmeno));
        let s = teren::skupiny::SkupinaBodu {
            id: cogo::volne_id_skupiny(&doc, &jmeno_skupiny),
            jmeno: jmeno_skupiny,
            popis: trf!("naklikané body povrchu {}", jmeno),
            ..Default::default()
        };
        (Some(s), jmeno)
    } else {
        (None, String::new())
    };
    let vyska = match n.vyska_bodu_nova {
        Some(z) => trf!("předvolená výška {}", povrchy::cislo_cz(z)),
        None if druh == DruhBodu::ProPovrch => tr("výšku zadejte jako x,y,z (další body převezmou předchozí)").into(),
        None => tr("výška ze zadání x,y,z, jinak z aktivního povrchu").into(),
    };
    let kod = if n.kod_bodu.is_empty() { String::new() } else { trf!(", kód {}", n.kod_bodu) };
    let co = match druh {
        DruhBodu::Rucne => tr("nové body").to_string(),
        DruhBodu::NaPovrchu => tr("body na povrchu").to_string(),
        DruhBodu::ProPovrch => trf!("body pro povrch „{}“", jmeno_povrchu),
    };
    let dalsi = cogo::dalsi_cislo(&doc);
    v.info(&trf!("Terén: {} od čísla {}{}{} (nastavení v Prospektoru → Body).", co, dalsi, kod, if druh == DruhBodu::NaPovrchu { String::new() } else { format!(", {vyska}") }));
    spust(host, Box::new(interaktivni::NovyBod {
        druh,
        dalsi,
        kod: n.kod_bodu.clone(),
        vyska: n.vyska_bodu_nova,
        povrch,
        styly: styly::Rozlisovac::new(&doc),
        meritko: nastaveni::meritko(&doc),
        body: Vec::new(),
        skupina,
        jmeno_povrchu,
        zprava: String::new(),
        ceka_vysku: None,
    }));
}

/// Změna měřítka: přepíše popisy bodů a přepočítá povrchy s popisem vrstevnic.
fn zmen_meritko(host: &mut dyn HostApi, m: u32) {
    let mut v = PresHost(host);
    zmen_meritko_v(&mut v, m);
}

/// Změna měřítka výkresu (i z Prospektoru).
pub(crate) fn zmen_meritko_v(v: &mut dyn Vykres, m: u32) {
    nastaveni::zmen(|n| n.meritko = m as f64);
    // měřítko poznámek Open CAD Studia (stavový řádek) se nastaví také, aby
    // se obě nastavení nehádala
    let _ = v.prikaz(&format!("CANNOSCALE 1:{m}"));
    match spravce::po_zmene_meritka(v) {
        Ok(t) => v.info(&trf!("Terén: {}", t)),
        Err(e) => v.chyba(&trf!("Terén: {}", e)),
    }
    spravce::posli(spravce::Akce::Obnovit);
}

fn nastaveni_cmd(host: &mut dyn HostApi, cmd: &str) -> bool {
    if let Some(f) = cmd.strip_prefix("TER_FMT_") {
        let f = match f {
            "PNEZD" | "PENZD" | "PYXZD" | "NEZD" | "ENZD" => Some(f.to_string()),
            _ => None,
        };
        host.push_info(&match &f {
            Some(f) => trf!("Terén: formát bodů {} (oddělovač podle nabídky Oddělovač)", f),
            None => tr("Terén: formát bodů se rozpozná automaticky").into(),
        });
        nastaveni::zmen(|n| n.format = f);
        return true;
    }
    if let Some(o) = cmd.strip_prefix("TER_ODD_") {
        let (o, t) = match o {
            "CARKA" => (Oddelovac::Carka, tr("čárka")),
            "STREDNIK" => (Oddelovac::Strednik, tr("středník")),
            "TAB" => (Oddelovac::Tabulator, tr("tabulátor")),
            _ => (Oddelovac::Mezera, "mezera"),
        };
        nastaveni::zmen(|n| n.oddelovac = o);
        host.push_info(&trf!("Terén: oddělovač {} (platí pro ručně zvolený formát)", t));
        return true;
    }
    // starší názvy příkazů stylu povrchu
    let cmd = match cmd.strip_prefix("TER_STYL_").filter(|s| ["VRSTP", "VRST", "TINVRST", "TIN", "OBRYS", "SPAD"].contains(s)) {
        Some(s) => format!("TER_SP_{s}"),
        None => cmd.to_string(),
    };
    if let Some(s) = cmd.strip_prefix("TER_SP_") {
        styl_povrchu_vyberu(host, s);
        return true;
    }
    if let Some(s) = cmd.strip_prefix("TER_SB_") {
        styl_bodu_vyberu(host, s);
        return true;
    }
    if let Some(i) = cmd.strip_prefix("TER_INT_") {
        let (iv, hl) = match i {
            "010" => (0.1, 0.5),
            "020" => (0.2, 1.0),
            "025" => (0.25, 1.0),
            "100" => (1.0, 5.0),
            "200" => (2.0, 10.0),
            _ => (0.5, 2.5),
        };
        nastav_povrch(host, tr("Interval vrstevnic"), |z| {
            z.interval = iv;
            z.hlavni = hl;
        });
        return true;
    }
    if let Some(m) = cmd.strip_prefix("TER_HRANA_") {
        let m: f64 = m.parse().unwrap_or(0.0);
        nastav_povrch(host, tr("Max. hrana"), |z| z.max_hrana = (m > 0.0).then_some(m));
        return true;
    }
    if let Some(m) = cmd.strip_prefix("TER_MERITKO_").and_then(|m| m.parse::<u32>().ok()) {
        if MERITKA.contains(&m) {
            zmen_meritko(host, m);
            return true;
        }
    }
    false
}

// ── Vstupní bod pluginu ─────────────────────────────────────────────────────

struct TerenPlugin;

impl BuiltinPlugin for TerenPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }
    fn entity_classes(&self) -> Option<&'static ocs_plugin_api::manifest::EntityClasses> {
        Some(tridy())
    }
    fn on_custom_grip_event(
        &mut self,
        host: &mut dyn HostApi,
        event: ocs_plugin_api::host::CustomGripEvent,
    ) -> ocs_plugin_api::host::CustomGripResponse {
        uchopy::udalost(host, event)
    }
    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(TerenModule)
    }
    fn on_load(&mut self, host: &mut dyn HostApi) {
        if let Some(s) = host.plugin_request_sender() {
            spravce::registruj_kanal(host.tab_id(), s);
        }
        registruj_quad(host);
    }
    fn on_notification(&mut self, _command_id: Option<u64>, n: HostNotification) {
        match n {
            HostNotification::DrawingChanged { tab_id, .. } | HostNotification::DocumentChangedV4 { tab_id, .. } => {
                spravce::zmena_vykresu(tab_id)
            }
            HostNotification::CommandStateChanged { tab_id, command } => spravce::stav_prikazu(tab_id, command.is_some()),
            HostNotification::SelectionChangedV4 { tab_id, handles } => spravce::vyber_zmenen(tab_id, handles),
            HostNotification::DocumentOpened { tab_id } => spravce::dokument_otevren(tab_id),
            HostNotification::PanelAction { panel_id, control_id, action, value } if panel_id == panel::ID => {
                panel::akce(&control_id, &action, value)
            }
            _ => {}
        }
    }
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        // PDMODE se nastavuje příkazem, který by krok Zpět ukončil – proto předem
        let c = cmd.trim().to_ascii_uppercase();
        if ["TER_IMPORT", "TER_PRIPRAVIT", "TER_BOD", "TER_NOVY", "TER_SB_"].iter().any(|p| c.starts_with(p)) {
            if let Some(ocs_plugin_api::host::HostSettingValue::Number(m)) = host.system_variable("PDMODE") {
                if (m - 1.0).abs() > 1e-9 {
                    host.execute_command("PDMODE 1");
                }
            }
        }
        if ["TER_IMPORT", "TER_PRIPRAVIT", "TER_BOD", "TER_NOVY", "TER_SB_", "TER_PROSPEKTOR"].iter().any(|p| c.starts_with(p)) {
            let mut v = vykres::PresHost(host);
            if let Ok(doc) = v.dokument() {
                cogo::presun_nosice(&mut v, &doc);
            }
        }
        let r = self.proved_prikaz(host, cmd);
        host.end_undo_group();
        r
    }
}

impl TerenPlugin {
    fn proved_prikaz(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        if let Some(s) = host.plugin_request_sender() {
            spravce::registruj_kanal(host.tab_id(), s);
        }
        registruj_quad(host);
        let cmd = cmd.trim();
        let (nazev, zbytek) = match cmd.find(char::is_whitespace) {
            Some(i) => (&cmd[..i], cmd[i..].trim()),
            None => (cmd, ""),
        };
        let nazev = nazev.to_ascii_uppercase();
        uvod_nastroje(host, &nazev);
        match nazev.as_str() {
            "TER_IMPORT" => {
                if zbytek.is_empty() {
                    host.push_info(tr("Terén: pro import klikněte na tlačítko Import bodů na záložce Terén."));
                    return true;
                }
                let n = nastaveni::akt();
                let mut v = PresHost(host);
                let cesta = zbytek.trim_matches('"');
                if n.nahled_importu {
                    okno::otevri_import(cesta);
                    host.push_info(tr("Terén: náhled importu je v okně Prospektoru – zkontrolujte formát a klikněte Importovat."));
                    return true;
                }
                let fmt = n.format.as_deref().map(|f| (f, n.oddelovac, 0));
                if let Some(body) = import::priprav(&mut v, cesta, fmt) {
                    if let Err(e) = cogo::priprav_vykres(&mut v) {
                        v.chyba(&trf!("Terén: {}", e));
                        return true;
                    }
                    let soubor = import::jmeno_souboru(cesta);
                    let doc = match v.dokument() {
                        Ok(d) => d,
                        Err(e) => {
                            v.chyba(&trf!("Terén: {}", e));
                            return true;
                        }
                    };
                    let skupina = import::skupina_importu(&doc, &soubor);
                    let ro = styly::Rozlisovac::new(&doc).s_skupinou(skupina.as_ref());
                    let m = nastaveni::meritko(&doc);
                    spust(host, Box::new(import::PotvrditImport::new(&body, &ro, m, &soubor, skupina)));
                }
            }
            "TER_IMPORT_F" => {
                // z náhledu importu: TER_IMPORT_F <formát> <oddělovač> <hlavička> <cesta>
                let mut casti = zbytek.splitn(4, char::is_whitespace);
                let (Some(f), Some(o), Some(hl), Some(cesta)) = (casti.next(), casti.next(), casti.next(), casti.next()) else {
                    host.push_error(tr("Terén: TER_IMPORT_F <formát> <oddělovač> <hlavička> <cesta>"));
                    return true;
                };
                let cesta = cesta.trim().trim_matches('"').to_string();
                let o = teren::body::Oddelovac::z_nazvu(o).unwrap_or(teren::body::Oddelovac::Mezera);
                let hl: usize = hl.parse().unwrap_or(0);
                let mut v = PresHost(host);
                if let Some(body) = import::priprav(&mut v, &cesta, Some((f, o, hl))) {
                    if let Err(e) = cogo::priprav_vykres(&mut v) {
                        v.chyba(&trf!("Terén: {}", e));
                        return true;
                    }
                    let soubor = import::jmeno_souboru(&cesta);
                    let doc = match v.dokument() {
                        Ok(d) => d,
                        Err(e) => {
                            v.chyba(&trf!("Terén: {}", e));
                            return true;
                        }
                    };
                    let skupina = import::skupina_importu(&doc, &soubor);
                    let ro = styly::Rozlisovac::new(&doc).s_skupinou(skupina.as_ref());
                    let m = nastaveni::meritko(&doc);
                    spust(host, Box::new(import::PotvrditImport::new(&body, &ro, m, &soubor, skupina)));
                }
            }
            "TER_NOVY_STAV" => novy_povrch(host, "Stavajici", tr("Stávající terén"), true),
            "TER_NOVY_NAVRH" => novy_povrch(host, "Navrh", tr("Návrh"), false),
            "TER_NOVY" => novy_povrch(host, "Povrch", tr("Povrch"), true),
            "TER_PROSPEKTOR" | "TER_SPRAVCE" | "TER_SEZNAM" => {
                okno::otevri();
                host.push_info(tr("Terén: Prospektor se otevírá ve vlastním okně."));
            }
            "TER_NAJIT" | "TER_STYL_BODU" => najit(host),
            "TER_EXPORT_POVRCHU" => {
                let mut v = PresHost(host);
                if let Some((z, _)) = aktivni(&mut v) {
                    spravce::skoc(spravce::Skok::Povrch(z.id.clone()));
                    okno::otevri();
                    spravce::posli(spravce::Akce::Obnovit);
                    v.info(&trf!("Terén: export povrchu „{}“ (LandXML nebo DXF) je v Prospektoru u povrchu – Export povrchu.", z.jmeno));
                }
            }
            "TER_PRIPRAVIT" => {
                spravce::posli(spravce::Akce::PripravVykres);
            }
            "TER_PRUVODCE" => {
                okno::otevri_pruvodce();
                host.push_info(tr("Terén: průvodce novým povrchem je v okně Prospektoru."));
            }
            "TER_NAPOVEDA" => {
                okno::otevri_napovedu("teren");
                host.push_info(tr("Terén: nápověda je v okně Prospektoru (v něm také klávesa F1)."));
            }
            "TER_AKCE" => {
                let mut v = PresHost(host);
                if zbytek.eq_ignore_ascii_case("ZNACKY") {
                    // ladění: počet objektů grafiky podle polohy bodu
                    if let Ok(doc) = v.dokument() {
                        let body = cogo::body(&doc);
                        let vse = cogo::grafika_bodu(&doc, &body.iter().map(|b| [b.bod.x, b.bod.y]).collect::<Vec<_>>());
                        let znacek = doc.model_space_entities().filter(|e| e.common().layer.eq_ignore_ascii_case("TER_BODY_ZNACKA")).count();
                        v.vystup(&trf!("  značek {}, grafika přiřazená bodům {}", znacek, vse.len()));
                    }
                    return true;
                }
                if zbytek.eq_ignore_ascii_case("PREHLED") {
                    match v.dokument() {
                        Ok(doc) => {
                            for r in spravce::prehled_text(&doc) {
                                v.vystup(&format!("  {r}"));
                            }
                        }
                        Err(e) => v.chyba(&trf!("Terén: {}", e)),
                    }
                    return true;
                }
                match akce_z_textu(zbytek).and_then(|a| spravce::proved_hned(&mut v, a)) {
                    Ok(Some(t)) => v.info(&trf!("Terén: {}", t)),
                    Ok(None) => {}
                    Err(e) => v.chyba(&trf!("Terén: {}", e)),
                }
            }
            "TER_BOD" => novy_bod(host, interaktivni::DruhBodu::Rucne),
            "TER_BOD_POVRCH" => novy_bod(host, interaktivni::DruhBodu::NaPovrchu),
            "TER_NOVY_KLIK" => novy_bod(host, interaktivni::DruhBodu::ProPovrch),
            "TER_PREPOCITAT" | "TER_POVRCH" => nastav_povrch(host, tr("Přepočítat povrch"), |_| {}),
            "TER_VYBRAT" => vybrat(host),
            "TER_SMAZAT" => {
                let mut v = PresHost(host);
                if let Some((z, doc)) = aktivni(&mut v) {
                    v.zpet(tr("Smazat povrch"));
                    match povrchy::smaz(&mut v, &doc, &z) {
                        Ok(()) => {
                            v.hotovo();
                            v.info(&trf!("Terén: povrch „{}“ smazán (spojnice, hranice a body zůstaly).", z.jmeno));
                            spravce::posli(spravce::Akce::Obnovit);
                        }
                        Err(e) => v.chyba(&trf!("Terén: {}", e)),
                    }
                }
            }
            "TER_ZRUSIT_UPRAVY" => nastav_povrch(host, tr("Zrušit úpravy povrchu"), |z| z.upravy.clear()),
            "TER_KONTROLA" => {
                let mut v = PresHost(host);
                if let Err(e) = spravce::kontrola(&mut v) {
                    v.chyba(&trf!("Terén: {}", e));
                }
            }
            "TER_SPOJNICE" => {
                let mut v = PresHost(host);
                let Some((z, doc)) = aktivni(&mut v) else { return true };
                let body = povrchy::body_z_vykresu(&doc);
                if body.is_empty() {
                    v.chyba(tr("Terén: ve výkrese nejsou body zaměření, na které by šlo klikat."));
                    return true;
                }
                let h = z.hladiny();
                povrchy::zajisti_hladinu(&mut v, &doc, &h.spojnice, 1);
                v.info(&trf!("Terén: spojnice povrchu „{}“ – klikejte na body (výška z bodu), Enter = hotovo{}.", z.jmeno, if z.auto { "" } else { tr(", pak Přepočítat") }));
                spust(host, Box::new(interaktivni::SpojnicePresBody {
                    hladina: h.spojnice,
                    jmeno_povrchu: z.jmeno.clone(),
                    body,
                    vrcholy: Vec::new(),
                    tolerance: nastaveni::akt().tolerance_bodu,
                    zprava: String::new(),
                }));
            }
            "TER_PRIDAT_SPOJNICI" => prirad_vyber(host, DruhCary::Spojnice),
            "TER_HRANICE" => prirad_vyber(host, DruhCary::Hranice),
            "TER_DIRA" => prirad_vyber(host, DruhCary::Dira),
            "TER_ODEBRAT_Z_POVRCHU" => odebrat_z_povrchu(host),
            "TER_OTOCIT" => spust_editaci(host, DruhEditace::OtocitHranu),
            "TER_PRIDAT_HRANU" => spust_editaci(host, DruhEditace::PridatHranu),
            "TER_SMAZAT_HRANU" => spust_editaci(host, DruhEditace::SmazatHranu),
            "TER_SMAZAT_TROJ" => spust_editaci(host, DruhEditace::SmazatTrojuhelnik),
            "TER_PRIDAT_BOD" => spust_editaci(host, DruhEditace::PridatBod),
            "TER_ODEBRAT_BOD" => spust_editaci(host, DruhEditace::OdebratBod),
            "TER_UCHOPY_ZAP" | "TER_UCHOPY_VYP" => {
                spravce::posli(spravce::Akce::Uchopy(Some(nazev == "TER_UCHOPY_ZAP")));
            }
            "TER_POVRCH_KLIK_ZAP" | "TER_POVRCH_KLIK_VYP" => {
                spravce::posli(spravce::Akce::PovrchyNevybirat(nazev == "TER_POVRCH_KLIK_VYP"));
            }
            "TER_UCHOPY" => {
                let zap = match zbytek.to_ascii_uppercase().as_str() {
                    "ZAP" | "ON" | "1" => Some(true),
                    "VYP" | "OFF" | "0" => Some(false),
                    _ => None,
                };
                spravce::posli(spravce::Akce::Uchopy(zap));
            }
            "TER_SROVNAT" => {
                let mut v = PresHost(host);
                proved(&mut v, spravce::Akce::SrovnatBody(false));
                spravce::posli(spravce::Akce::Obnovit);
            }
            "TER_POPISY_NA_MISTO" | "TER_SKUPINA_Z_VYBERU" => {
                let vyber = host.selection();
                let mut v = PresHost(host);
                let doc = match v.dokument() {
                    Ok(d) => d,
                    Err(e) => {
                        v.chyba(&trf!("Terén: {}", e));
                        return true;
                    }
                };
                let body = cogo::body_z_vyberu(&doc, &vyber);
                if body.is_empty() {
                    v.info(tr("Terén: nejdřív vyberte body ve výkrese (stačí kliknout na značku nebo popis) a klikněte znovu."));
                    return true;
                }
                if nazev == "TER_POPISY_NA_MISTO" {
                    proved(&mut v, spravce::Akce::PopisyNaMisto(body.clone()));
                    spravce::obnov_vyber(&mut v, &body, &[]);
                } else {
                    // akce sama nastaví skok na novou skupinu
                    proved(&mut v, spravce::Akce::SkupinaNova(body));
                    okno::otevri();
                }
                spravce::posli(spravce::Akce::Obnovit);
            }
            "TER_EXPORT_BODU" => {
                spravce::skoc(spravce::Skok::Skupina(teren::skupiny::VSECHNY.to_string()));
                okno::otevri();
                spravce::posli(spravce::Akce::Obnovit);
                host.push_info(tr("Terén: export bodů je v Prospektoru u skupiny bodů (Všechny body nebo jiná skupina) – Export bodů skupiny."));
            }
            "TER_3D" => {
                let mut v = PresHost(host);
                if let Some((z, _)) = aktivni(&mut v) {
                    okno::otevri_3d(vec![z.id.clone()]);
                    v.info(&trf!("Terén: povrch „{}“ ve 3D – v okně Prospektoru.", z.jmeno));
                }
            }
            "TER_SABLONA_ULOZ" => {
                let mut v = PresHost(host);
                proved(&mut v, spravce::Akce::SablonaUloz);
            }
            "TER_PANEL" => {
                let doc = host.document().clone();
                let styly: Vec<(String, String)> = styly::styly_bodu(&doc).into_iter().map(|s| (s.id, s.jmeno)).collect();
                host.register_panel(panel::definice(&styly));
                host.toggle_panel(panel::ID, Some(true));
                let vyber = host.selection();
                let h = cogo::body_z_vyberu(&doc, &vyber);
                let vse = cogo::body(&doc);
                let vybrane: Vec<&cogo::BodVykresu> = vse.iter().filter(|b| h.contains(&b.handle)).collect();
                host.update_panel(panel::ID, panel::hodnoty(&vybrane, panel::text_povrchu(&doc)));
            }
            "TER_VYSKA" => {
                let mut v = PresHost(host);
                let Some((z, doc)) = aktivni(&mut v) else { return true };
                match Povrch::z_definice(&povrchy::definice(&doc, &z).definice, 0.001) {
                    Ok(povrch) => {
                        povrchy::zajisti_hladinu(&mut v, &doc, cogo::H_VYSKA, 7);
                        host.push_info(&trf!("Terén: výška v povrchu „{}“ – klikejte, Enter = konec.", z.jmeno));
                        spust(host, Box::new(interaktivni::VyskaVBode {
                            povrch,
                            hladina: cogo::H_VYSKA.into(),
                            vyska_textu: nastaveni::meritko(&doc) * 0.002,
                            zprava: String::new(),
                        }));
                    }
                    Err(e) => host.push_error(&trf!("Terén: {}", e)),
                }
            }
            other => return nastaveni_cmd(host, other),
        }
        true
    }
}

ocs_plugin_api::export_plugin!(TerenPlugin);

/// Provede akci Prospektoru z pásu karet a vypíše výsledek.
fn proved(v: &mut dyn Vykres, a: spravce::Akce) {
    match spravce::proved_hned(v, a) {
        Ok(Some(t)) => v.info(&trf!("Terén: {}", t)),
        Ok(None) => {}
        Err(e) => v.chyba(&trf!("Terén: {}", e)),
    }
}

/// Spustí interaktivní příkaz Terénu (hlídaný – viz `vykres::PRIKAZY`).
fn spust(host: &mut dyn HostApi, c: Box<dyn ocs_plugin_api::host::InteractiveCommand>) {
    host.start_interactive(vykres::hlidany(c));
}
