//! Náhled importu bodů: první řádky souboru, rozpoznaný formát a první body.

use teren::preklad::tr;
use teren::trf;
use eframe::egui::{self, Color32, FontId, RichText};
use teren::body::{nacti_body, FormatBodu, Oddelovac, VysledekImportu};
use teren::rozpoznani::{nazev_oddelovace, rozpoznej, souhrn};

use super::{Okno, CERVENA, ORANZOVA, SEDA, ZELENA};
use crate::nastaveni;
use crate::spravce::{self, Akce};

const FORMATY: [(&str, &str); 6] = [
    ("PNEZD", "PNEZD – číslo, N(Y), E(X), Z, popis"),
    ("PENZD", "PENZD – číslo, E(X), N(Y), Z, popis"),
    ("PYXZD", "PYXZD – číslo, Y, X, Z, popis (S-JTSK kladné)"),
    ("NEZD", "NEZ – bez čísla"),
    ("ENZD", "ENZ – bez čísla"),
    ("PNEZ", "PNEZ – bez popisu"),
];
const ODDELOVACE: [(Oddelovac, &str); 4] = [
    (Oddelovac::Mezera, "mezera"),
    (Oddelovac::Carka, "carka"),
    (Oddelovac::Strednik, "strednik"),
    (Oddelovac::Tabulator, "tab"),
];

pub(super) struct NahledImportu {
    cesta: String,
    /// Rozepsaná cesta (pole Soubor).
    cesta_edit: String,
    text: String,
    chyba: Option<String>,
    format: String,
    oddelovac: Oddelovac,
    hlavicka: usize,
    rozpoznano: String,
    /// Rozpoznání je nejisté (zvýraznit).
    varovani: bool,
    vysledek: Option<VysledekImportu>,
    klic: (String, Oddelovac, usize),
}

impl NahledImportu {
    pub(super) fn novy(cesta: &str) -> Self {
        let mut n = Self {
            cesta: cesta.to_string(),
            cesta_edit: cesta.to_string(),
            text: String::new(),
            chyba: None,
            format: "PNEZD".into(),
            oddelovac: Oddelovac::Mezera,
            hlavicka: 0,
            rozpoznano: String::new(),
            varovani: false,
            vysledek: None,
            klic: (String::new(), Oddelovac::Mezera, usize::MAX),
        };
        if cesta.trim().is_empty() {
            n.chyba = Some(tr("Vložte cestu k souboru bodů nebo soubor přetáhněte do tohoto okna.").into());
            return n;
        }
        match crate::import::text_souboru(cesta) {
            Ok(t) => n.text = t,
            Err(e) => n.chyba = Some(e),
        }
        let pref = nastaveni::akt();
        if let Some(f) = pref.format {
            n.format = f;
            n.oddelovac = pref.oddelovac;
            n.rozpoznano = tr("podle nastavení (formát zvolený ručně)").into();
        } else if let Some(k) = rozpoznej(&n.text).first() {
            n.format = k.format.to_string();
            n.oddelovac = k.oddelovac;
            n.hlavicka = k.hlavicka;
            n.rozpoznano = trf!("rozpoznáno automaticky{}", if k.sjtsk() { "" } else { tr(" – body neleží v S-JTSK pro ČR, ověřte pořadí sloupců") });
            n.varovani = !k.sjtsk();
        } else {
            n.rozpoznano = tr("formát se nepodařilo rozpoznat – zvolte ho ručně").into();
            n.varovani = true;
        }
        n
    }

    fn prepocti(&mut self) {
        let klic = (self.format.clone(), self.oddelovac, self.hlavicka);
        if klic == self.klic {
            return;
        }
        self.klic = klic;
        self.vysledek = FormatBodu::z_retezce(&self.format, self.oddelovac).ok().map(|mut f| {
            f.preskocit_radku = self.hlavicka;
            nacti_body(&self.text, &f)
        });
    }
}

impl Okno {
    pub(super) fn okno_importu(&mut self, ctx: &egui::Context) {
        let Some(n) = self.nahled_importu.as_mut() else { return };
        n.prepocti();
        let mut otevreno = true;
        let mut hotovo = false;
        let mut nacist: Option<String> = None;
        if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
            nacist = Some(p.display().to_string());
        }
        let soubor = std::path::Path::new(&n.cesta).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Window::new(trf!("Import bodů – {}", soubor))
            .open(&mut otevreno)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([700.0, 520.0])
            .collapsible(false)
            .max_width(ctx.content_rect().width() - 24.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Soubor"));
                    let r = ui.add(egui::TextEdit::singleline(&mut n.cesta_edit).desired_width((ui.available_width() - 70.0).max(120.0)));
                    if ui.button(tr("Načíst")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        nacist = Some(n.cesta_edit.trim().trim_matches('"').to_string());
                    }
                });
                if let Some(e) = &n.chyba {
                    ui.label(RichText::new(e).color(if n.cesta.is_empty() { SEDA } else { CERVENA }));
                    return;
                }
                // soubor – prvních pár řádků
                ui.label(RichText::new(tr("SOUBOR")).small().strong().color(SEDA));
                egui::Frame::new().fill(Color32::from_gray(20)).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, r) in n.text.lines().take(6).enumerate() {
                        let barva = if i < n.hlavicka { SEDA } else { ui.visuals().text_color() };
                        ui.label(RichText::new(format!("{:>3}  {}", i + 1, r.chars().take(110).collect::<String>())).font(FontId::monospace(11.5)).color(barva));
                    }
                });
                ui.add_space(6.0);
                // formát
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Formát"));
                    let akt = FORMATY.iter().find(|f| f.0 == n.format).map_or(n.format.clone(), |f| tr(f.1).to_string());
                    egui::ComboBox::from_id_salt("imp_n_fmt").selected_text(akt).width(290.0).show_ui(ui, |ui| {
                        for (f, t) in FORMATY {
                            ui.selectable_value(&mut n.format, f.to_string(), tr(t));
                        }
                    });
                    ui.label(tr("Oddělovač"));
                    egui::ComboBox::from_id_salt("imp_n_odd").selected_text(nazev_oddelovace(n.oddelovac)).width(100.0).show_ui(ui, |ui| {
                        for (o, _) in ODDELOVACE {
                            ui.selectable_value(&mut n.oddelovac, o, nazev_oddelovace(o));
                        }
                    });
                    ui.label(tr("Přeskočit řádků"));
                    ui.add(egui::DragValue::new(&mut n.hlavicka).range(0..=50)).on_hover_text(tr("hlavička souboru"));
                });
                ui.label(RichText::new(&n.rozpoznano).small().color(if n.varovani { ORANZOVA } else { SEDA }));
                ui.add_space(6.0);

                // první body
                let Some(v) = &n.vysledek else {
                    ui.label(RichText::new(tr("Neplatný formát.")).color(CERVENA));
                    return;
                };
                ui.label(RichText::new(tr("PRVNÍ BODY")).small().strong().color(SEDA));
                egui::Grid::new("imp_n_body").striped(true).num_columns(5).spacing([18.0, 3.0]).show(ui, |ui| {
                    for t in [tr("Číslo"), "Y", "X", "Z", tr("Popis")] {
                        ui.label(RichText::new(t).strong());
                    }
                    ui.end_row();
                    for b in v.body.iter().take(8) {
                        ui.label(&b.cislo);
                        ui.label(format!("{:.2}", -b.x));
                        ui.label(format!("{:.2}", -b.y));
                        let z = RichText::new(format!("{:.2}", b.z));
                        ui.label(if teren::rozpoznani::podezrela_vyska(b.z) { z.color(ORANZOVA) } else { z });
                        ui.label(&b.popis);
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                // souhrn
                let s = souhrn(v);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(trf!("{} bodů", s.pocet)).strong().color(if s.pocet > 0 { ZELENA } else { CERVENA }));
                    if s.chyby > 0 {
                        ui.label(RichText::new(trf!("· {} řádků se nepodařilo přečíst", s.chyby)).color(ORANZOVA))
                            .on_hover_text(v.chyby.iter().take(8).map(|c| c.to_string()).collect::<Vec<_>>().join("\n"));
                    }
                    if s.mimo_cr > 0 {
                        ui.label(RichText::new(trf!("· {} mimo ČR (S-JTSK)", s.mimo_cr)).color(ORANZOVA));
                    }
                    if s.podezrele_z > 0 {
                        ui.label(RichText::new(trf!("· {} s nesmyslnou výškou", s.podezrele_z)).color(ORANZOVA));
                    }
                    if !s.duplicitni_cisla.is_empty() {
                        ui.label(RichText::new(trf!("· {} opakovaných čísel", s.duplicitni_cisla.len())).color(ORANZOVA))
                            .on_hover_text(s.duplicitni_cisla.iter().take(20).cloned().collect::<Vec<_>>().join(", "));
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let mut ukazovat = nastaveni::akt().nahled_importu;
                    if ui.checkbox(&mut ukazovat, tr("Náhled ukazovat i příště")).changed() {
                        nastaveni::zmen(|x| x.nahled_importu = ukazovat);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let tl = egui::Button::new(RichText::new(trf!("Importovat {} bodů", s.pocet)).color(Color32::WHITE))
                            .fill(Color32::from_rgb(44, 95, 143));
                        if ui.add_enabled(s.pocet > 0, tl).clicked() {
                            spravce::posli(Akce::Import(n.cesta.clone(), n.format.clone(), n.oddelovac, n.hlavicka));
                            hotovo = true;
                        }
                        if ui.button(tr("Zrušit")).clicked() {
                            hotovo = true;
                        }
                    });
                });
            });
        if let Some(c) = nacist {
            self.nahled_importu = Some(NahledImportu::novy(&c));
        } else if !otevreno || hotovo {
            self.nahled_importu = None;
        }
    }
}
