//! Průvodce novým povrchem (Základ → Body → Hranice a spojnice → Zobrazení).

use teren::preklad::tr;
use teren::trf;
use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText};
use teren::skupiny::{kody_z_textu, VSECHNY};

use super::{interval_text, napoveda, Okno, MODRA, ORANZOVA, SEDA, ZELENA};
use crate::nastaveni;
use crate::povrchy::INTERVALY;
use crate::spravce::{self, Akce, NovyPovrch, StavSpravce};

const KROKY: [&str; 4] = ["Základ", "Body", "Hranice a spojnice", "Zobrazení"];
const HRANY: [Option<f64>; 6] = [None, Some(10.0), Some(20.0), Some(30.0), Some(50.0), Some(100.0)];

pub(super) struct Pruvodce {
    krok: usize,
    jmeno: String,
    skupiny: HashSet<String>,
    kody: String,
    max_hrana: Option<f64>,
    styl: String,
    interval: (f64, f64),
    auto: bool,
}

impl Pruvodce {
    pub(super) fn novy(stav: &StavSpravce) -> Self {
        // výchozí skupina: „Zaměření“, jinak všechny body
        let zamereni = stav
            .skupiny
            .iter()
            .find(|g| g.skupina.jmeno.eq_ignore_ascii_case("Zaměření"))
            .map_or(VSECHNY.to_string(), |g| g.skupina.id.clone());
        let mut jmeno = tr("Stávající terén").to_string();
        let mut n = 2;
        while stav.povrchy.iter().any(|p| p.jmeno.eq_ignore_ascii_case(&jmeno)) {
            jmeno = trf!("Stávající terén {}", n);
            n += 1;
        }
        Self {
            krok: 0,
            jmeno,
            skupiny: HashSet::from([zamereni]),
            kody: nastaveni::akt().vyloucene_kody.join(", "),
            max_hrana: None,
            styl: "VRSTP".into(),
            interval: (0.5, 2.5),
            auto: true,
        }
    }
}

impl Okno {
    pub(super) fn okno_pruvodce(&mut self, ctx: &egui::Context, stav: &StavSpravce) {
        let Some(p) = self.pruvodce.as_mut() else { return };
        let mut otevreno = true;
        let mut hotovo = false;
        egui::Window::new(tr("Nový povrch"))
            .open(&mut otevreno)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([640.0, 420.0])
            .collapsible(false)
            .show(ctx, |ui| {
                // kroky
                ui.horizontal(|ui| {
                    for (i, k) in KROKY.iter().enumerate() {
                        let (znak, barva) = if i < p.krok {
                            ("✔".to_string(), ZELENA)
                        } else if i == p.krok {
                            ((i + 1).to_string(), MODRA)
                        } else {
                            ((i + 1).to_string(), SEDA)
                        };
                        let t = RichText::new(format!("{znak} {}", tr(k))).color(barva);
                        if ui.selectable_label(i == p.krok, if i == p.krok { t.strong() } else { t }).clicked() {
                            p.krok = i;
                        }
                        if i + 1 < KROKY.len() {
                            ui.label(RichText::new("›").color(SEDA));
                        }
                    }
                });
                ui.separator();

                // náhled – body vybraných skupin bez vyloučených kódů
                let kody = kody_z_textu(&p.kody);
                let filtry: Vec<_> =
                    stav.skupiny.iter().filter(|g| p.skupiny.contains(&g.skupina.id)).map(|g| g.skupina.filtr()).collect();
                let mut bodu = 0usize;
                let mut vynechano: Vec<(String, usize)> = Vec::new();
                let (mut z0, mut z1) = (f64::MAX, f64::MIN);
                for b in &stav.body {
                    if !filtry.iter().any(|f| f.obsahuje(&b.bod, &b.zdroj)) {
                        continue;
                    }
                    let kod = b.bod.kod();
                    if kody.iter().any(|k| k.eq_ignore_ascii_case(kod)) {
                        match vynechano.iter_mut().find(|v| v.0.eq_ignore_ascii_case(kod)) {
                            Some(v) => v.1 += 1,
                            None => vynechano.push((kod.to_uppercase(), 1)),
                        }
                        continue;
                    }
                    bodu += 1;
                    z0 = z0.min(b.bod.z);
                    z1 = z1.max(b.bod.z);
                }
                drop(filtry);

                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - 200.0).max(300.0));
                        ui.set_min_height(260.0);
                        match p.krok {
                            0 => {
                                ui.label(tr("Jméno povrchu"));
                                ui.add(egui::TextEdit::singleline(&mut p.jmeno).desired_width(280.0));
                                ui.add_space(8.0);
                                ui.checkbox(&mut p.auto, tr("Přepočítat sám, když se změní body, spojnice nebo hranice"));
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        tr("Povrch vznikne ze skupin bodů (další krok). Spojnice, hranice a ruční úpravy \
                                         přidáte po vytvoření – průvodce ukáže jak."),
                                    )
                                    .color(SEDA),
                                );
                            }
                            1 => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(tr("Ze kterých skupin bodů")).strong());
                                    napoveda::otaznik(ui, tr("Skupina bodů"));
                                });
                                for g in &stav.skupiny {
                                    let mut ano = p.skupiny.contains(&g.skupina.id);
                                    ui.horizontal(|ui| {
                                        if ui.checkbox(&mut ano, &g.skupina.jmeno).changed() {
                                            if ano {
                                                p.skupiny.insert(g.skupina.id.clone());
                                            } else {
                                                p.skupiny.remove(&g.skupina.id);
                                            }
                                        }
                                        ui.label(RichText::new(g.pocet.to_string()).color(SEDA));
                                    });
                                }
                                ui.add_space(6.0);
                                egui::Grid::new("pruv_body").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                                    ui.label(tr("Vyloučit kódy"));
                                    ui.add(egui::TextEdit::singleline(&mut p.kody).hint_text(tr("např. STR")).desired_width(140.0));
                                    ui.end_row();
                                    ui.label(tr("Max. délka hrany"));
                                    let t = |h: Option<f64>| h.map_or(tr("bez omezení").to_string(), |h| format!("{} m", h));
                                    egui::ComboBox::from_id_salt("pruv_hrana").selected_text(t(p.max_hrana)).show_ui(ui, |ui| {
                                        for h in HRANY {
                                            ui.selectable_value(&mut p.max_hrana, h, t(h));
                                        }
                                    });
                                    ui.end_row();
                                });
                                ui.label(
                                    RichText::new(tr("Max. délka hrany zabrání dlouhým trojúhelníkům přes místa bez bodů (okraje, zátoky)."))
                                        .small()
                                        .color(SEDA),
                                );
                                // body s „nestandardními“ kódy, které v terénu obvykle nechceme
                                let str_bodu = stav.body.iter().filter(|b| b.bod.kod().eq_ignore_ascii_case("STR")).count();
                                if str_bodu > 0 && !kody.iter().any(|k| k == "STR") {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(trf!("Kód STR (stromy) do terénu obvykle nepatří – ve výkrese je {} takových bodů. Doplňte ho do Vyloučit kódy.", str_bodu))
                                        .color(ORANZOVA),
                                    );
                                }
                            }
                            2 => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(tr("Spojnice (lomové hrany)")).strong());
                                    napoveda::otaznik(ui, tr("Spojnice (lomová hrana)"));
                                });
                                ui.label(tr("Po vytvoření je přidáte na záložce Terén: Spojnice přes body (klikáním na body zaměření) nebo Přidat spojnici (vybrané 3D křivky)."));
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(tr("Hranice a díry")).strong());
                                    napoveda::otaznik(ui, tr("Hranice / díra"));
                                });
                                ui.label(tr("Bez hranice povrch sahá k nejvzdálenějším bodům. Ořízněte ho uzavřenou křivkou: vyberte ji a klikněte Hranice (Díra vynechá vnitřek)."));
                                ui.add_space(8.0);
                                ui.label(RichText::new(tr("Obojí najdete i v Prospektoru ve stromu pod povrchem.")).color(SEDA));
                            }
                            _ => {
                                egui::Grid::new("pruv_zobr").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                                    ui.label(tr("Styl povrchu"));
                                    let jm = stav
                                        .styly_povrchu
                                        .iter()
                                        .find(|s| s.id.eq_ignore_ascii_case(&p.styl))
                                        .map_or(p.styl.clone(), |s| s.jmeno.clone());
                                    egui::ComboBox::from_id_salt("pruv_styl").selected_text(jm).width(220.0).show_ui(ui, |ui| {
                                        for s in &stav.styly_povrchu {
                                            ui.selectable_value(&mut p.styl, s.id.clone(), &s.jmeno);
                                        }
                                    });
                                    ui.end_row();
                                    let krok_stylu =
                                        stav.styly_povrchu.iter().find(|s| s.id.eq_ignore_ascii_case(&p.styl)).and_then(|s| s.interval);
                                    ui.label(tr("Vrstevnice po"));
                                    match krok_stylu {
                                        Some((i, h)) => {
                                            ui.label(trf!("{} (podle stylu)", interval_text(i, h)));
                                        }
                                        None => {
                                            egui::ComboBox::from_id_salt("pruv_int")
                                                .selected_text(interval_text(p.interval.0, p.interval.1))
                                                .width(160.0)
                                                .show_ui(ui, |ui| {
                                                    for (i, h) in INTERVALY {
                                                        ui.selectable_value(&mut p.interval, (i, h), interval_text(i, h));
                                                    }
                                                });
                                        }
                                    }
                                    ui.end_row();
                                });
                                ui.add_space(6.0);
                                ui.label(RichText::new(tr("Styl i krok jde kdykoli změnit pravým tlačítkem na povrch v Prospektoru.")).color(SEDA));
                            }
                        }
                    });
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(tr("NÁHLED")).small().strong().color(SEDA));
                        ui.label(RichText::new(format!("{bodu}")).size(26.0).strong());
                        ui.label(tr("bodů"));
                        if bodu >= 3 {
                            ui.label(RichText::new(trf!("asi {} trojúhelníků", 2 * bodu - 5)).color(SEDA));
                            ui.label(RichText::new(format!("{} – {} m n. m.", cislo(z0), cislo(z1))).color(SEDA));
                        } else {
                            ui.label(RichText::new(tr("potřeba aspoň 3 body")).color(ORANZOVA));
                        }
                        for (k, n) in &vynechano {
                            ui.label(RichText::new(trf!("vynecháno {} × {}", n, k)).small().color(SEDA));
                        }
                    });
                });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(p.krok > 0, egui::Button::new(tr("Zpět"))).clicked() {
                        p.krok -= 1;
                    }
                    if p.krok + 1 < KROKY.len() && ui.button(tr("Další")).clicked() {
                        p.krok += 1;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let lze = bodu >= 3 && !p.jmeno.trim().is_empty();
                        let tl = egui::Button::new(RichText::new(tr("Vytvořit povrch")).color(Color32::WHITE)).fill(Color32::from_rgb(44, 95, 143));
                        if ui
                            .add_enabled(lze, tl)
                            .on_disabled_hover_text(tr("zadejte jméno a vyberte skupiny s aspoň 3 body"))
                            .clicked()
                        {
                            spravce::posli(Akce::NovyPovrch(NovyPovrch {
                                jmeno: p.jmeno.trim().to_string(),
                                skupiny: stav
                                    .skupiny
                                    .iter()
                                    .filter(|g| p.skupiny.contains(&g.skupina.id))
                                    .map(|g| g.skupina.id.clone())
                                    .collect(),
                                vyloucene_kody: kody.clone(),
                                max_hrana: p.max_hrana,
                                styl: p.styl.clone(),
                                interval: p.interval,
                                auto: p.auto,
                            }));
                            hotovo = true;
                        }
                    });
                });
            });
        if !otevreno || hotovo {
            self.pruvodce = None;
        }
    }
}

fn cislo(z: f64) -> String {
    format!("{z:.2}").replace('.', ",")
}
