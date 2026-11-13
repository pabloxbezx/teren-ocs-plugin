//! Režim Výběr: vlastnosti bodu nebo povrchu vybraného ve výkrese,
//! řádek aktivního povrchu a postup „Jak na to“.

use teren::preklad::tr;
use teren::trf;
use super::*;
use crate::styly::Puvod;

const ZELENA_TMAVA: Color32 = Color32::from_rgb(31, 39, 35);
const ZELENA_RAM: Color32 = Color32::from_rgb(47, 74, 60);
const ZLUTA_TMAVA: Color32 = Color32::from_rgb(46, 38, 22);

/// Rámeček se zaobleným pozadím.
fn ramecek<R>(ui: &mut egui::Ui, pozadi: Color32, ram: Color32, obsah: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(pozadi)
        .stroke(egui::Stroke::new(1.0, ram))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            obsah(ui)
        })
        .inner
}

/// Štítek („chip“); zvýrazněný = aktivní.
fn stitek(ui: &mut egui::Ui, text: &str, zvyraznit: bool) {
    let (pozadi, ram, barva) = if zvyraznit {
        (Color32::from_rgb(47, 90, 70), Color32::from_rgb(78, 154, 120), Color32::WHITE)
    } else {
        (Color32::from_rgb(40, 40, 40), Color32::from_rgb(61, 61, 61), Color32::from_rgb(140, 140, 140))
    };
    egui::Frame::new()
        .fill(pozadi)
        .stroke(egui::Stroke::new(1.0, ram))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            let mut t = RichText::new(text).small().color(barva);
            if zvyraznit {
                t = t.strong();
            }
            ui.label(t);
        });
}

fn sekce(ui: &mut egui::Ui, t: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(t.to_uppercase()).small().strong().color(SEDA));
}

fn styl_podle_id<'a>(stav: &'a StavSpravce, id: &str) -> Option<&'a StylBodu> {
    stav.styly_bodu.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

/// Výběr stylu bodu s ikonami; vrací nově zvolené id ("" = podle kódu / skupiny).
pub(super) fn combo_stylu_bodu(ui: &mut egui::Ui, salt: &str, stav: &StavSpravce, aktualni: Option<&StylBodu>, text: &str) -> Option<String> {
    let mut volba = None;
    ui.horizontal(|ui| {
        if let Some(st) = aktualni {
            ikona_stylu(ui, st, 20.0);
        }
        egui::ComboBox::from_id_salt(salt).selected_text(text).width(200.0).show_ui(ui, |ui| {
            if ui.selectable_label(false, RichText::new(tr("podle skupiny / kódu")).italics()).clicked() {
                volba = Some(String::new());
            }
            for st in &stav.styly_bodu {
                ui.horizontal(|ui| {
                    ikona_stylu(ui, st, 18.0);
                    let vybran = aktualni.is_some_and(|a| a.id == st.id);
                    if ui.selectable_label(vybran, &st.jmeno).clicked() {
                        volba = Some(st.id.clone());
                    }
                });
            }
        });
    });
    volba
}

impl Okno {
    /// Řádek s aktivním povrchem a rychlým přepnutím (v obou režimech).
    /// Režim Výběr.
    pub(super) fn panel_vyberu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let body: Vec<&BodVykresu> = stav.body.iter().filter(|b| stav.vyber_body.contains(&b.handle)).collect();
        let povrchy: Vec<&PrehledPovrchu> = stav.povrchy.iter().filter(|p| stav.vyber_povrchy.contains(&p.id)).collect();
        if body.len() == 1 {
            self.vyber_bod(ui, stav, body[0]);
        } else if body.len() > 1 {
            self.vyber_vice_bodu(ui, stav, &body);
        } else if povrchy.len() == 1 {
            self.vyber_povrch(ui, stav, povrchy[0]);
        } else if povrchy.len() > 1 {
            self.vyber_vice_povrchu(ui, stav, &povrchy);
        } else {
            ui.add_space(12.0);
            ui.label(RichText::new(tr("Nic není vybráno")).size(18.0).strong());
            ui.add_space(4.0);
            if stav.vyber_jine > 0 {
                ui.label(
                    RichText::new(trf!("Vybráno {} objektů, které nepatří k bodům ani povrchům.", stav.vyber_jine)).color(SEDA),
                );
            }
            ui.label(tr("Klikněte ve výkrese na bod (značku nebo popis) nebo na povrch – tady se ukážou jeho vlastnosti a styl."));
            ui.add_space(8.0);
            zaskrtnuti_nevybirat(ui);
        }
    }

    fn vyber_bod(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, b: &BodVykresu) {
        if self.vyber_edit.as_ref().map(|e| e.0) != Some(b.handle) {
            self.vyber_edit = Some((b.handle, b.bod.cislo.clone(), b.bod.popis.clone(), b.bod.z));
        }
        let (id_stylu, puvod) = stav.styl_bodu.get(&b.handle).cloned().unwrap_or((styly::STANDARD.into(), Puvod::Standard));
        let styl = styl_podle_id(stav, &id_stylu).cloned();

        // hlavička
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if let Some(st) = &styl {
                ikona_stylu(ui, st, 44.0);
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(trf!("Bod {}", b.bod.cislo)).size(18.0).strong());
                let zdroj = if b.zdroj.is_empty() { tr("bez původu").to_string() } else { b.zdroj.clone() };
                ui.label(RichText::new(trf!("1 vybraný bod · {}", zdroj)).small().color(SEDA));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Ukázat")).clicked() {
                    ukazat_body(stav, &[b.handle]);
                }
            });
        });
        ui.separator();

        // údaje bodu
        let Some((_, cislo, popis, z)) = self.vyber_edit.as_mut() else { return };
        egui::Grid::new("vyber_bod").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("Číslo")).color(SEDA));
            ui.add(egui::TextEdit::singleline(cislo).desired_width(160.0));
            ui.end_row();
            ui.label(RichText::new(tr("Kód")).color(SEDA));
            ui.add(egui::TextEdit::singleline(popis).desired_width(160.0));
            ui.end_row();
            ui.label(RichText::new(tr("Výška Z")).color(SEDA));
            ui.add(egui::DragValue::new(z).speed(0.01).max_decimals(3).suffix(" m"));
            ui.end_row();
            ui.label(RichText::new("Y / X").color(SEDA));
            ui.label(RichText::new(format!("{:.2} / {:.2}", -b.bod.x, -b.bod.y)).monospace());
            ui.end_row();
        });
        let zmeneno = cislo.trim() != b.bod.cislo || popis.trim() != b.bod.popis || (*z - b.bod.z).abs() > 1e-9;
        let novy = Bod { cislo: cislo.trim().to_string(), z: *z, popis: popis.trim().to_string(), ..b.bod.clone() };
        if zmeneno {
            let mut vratit = false;
            ui.horizontal(|ui| {
                if ui.button(RichText::new(tr("Uložit změny")).strong()).clicked() {
                    spravce::posli(Akce::BodUpravit(b.handle, novy));
                }
                vratit = ui.button(tr("Vrátit")).clicked();
            });
            if vratit {
                self.vyber_edit = None;
            }
        }
        ui.separator();

        // styl
        sekce(ui, tr("Styl"));
        let jmeno = styl.as_ref().map_or(id_stylu.clone(), |s| s.jmeno.clone());
        ui.horizontal(|ui| {
            if let Some(v) = combo_stylu_bodu(ui, "vyber_styl", stav, styl.as_ref(), &jmeno) {
                spravce::posli(Akce::StylBodu(vec![b.handle], v));
            }
            if ui.button(tr("✏ Upravit")).on_hover_text(tr("otevře editor stylu")).clicked() {
                self.rezim = Rezim::Vykres;
                self.vyber_uzel(Uzel::StylBodu(id_stylu.clone()));
            }
        });
        ui.add_space(4.0);
        let vysvetleni = match &puvod {
            Puvod::Rucne => tr("Styl je nastavený přímo tomuto bodu.").to_string(),
            Puvod::Kod(k) => trf!("Styl je podle kódu {} – nastaveno ve stylu „{}“ (pole pro kódy).", k, jmeno),
            Puvod::Skupina(g) => trf!("Styl je podle skupiny „{}“.", g),
            Puvod::Standard => tr("Bod nemá styl z kódu ani ze skupiny – platí výchozí Standard.").to_string(),
        };
        let kod = b.bod.kod().to_uppercase();
        let skupina_se_stylem = stav
            .skupiny
            .iter()
            .find(|g| g.skupina.id != VSECHNY && !g.skupina.styl.is_empty() && g.skupina.obsahuje(&b.bod, &b.zdroj))
            .map(|g| g.skupina.jmeno.clone());
        ramecek(ui, ZELENA_TMAVA, ZELENA_RAM, |ui| {
            ui.label(RichText::new(vysvetleni).color(Color32::from_rgb(201, 233, 218)));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                stitek(ui, tr("ručně"), puvod == Puvod::Rucne);
                ui.label(RichText::new("›").color(SEDA));
                stitek(ui, &if kod.is_empty() { tr("kód").to_string() } else { trf!("kód {}", kod) }, matches!(puvod, Puvod::Kod(_)));
                ui.label(RichText::new("›").color(SEDA));
                let g = match (&puvod, &skupina_se_stylem) {
                    (Puvod::Skupina(g), _) | (_, Some(g)) => trf!("skupina {}", g),
                    _ => tr("skupina").to_string(),
                };
                stitek(ui, &g, matches!(puvod, Puvod::Skupina(_)));
                ui.label(RichText::new("›").color(SEDA));
                stitek(ui, "Standard", puvod == Puvod::Standard);
            });
            ui.label(RichText::new(tr("Přednost má styl nastavený ručně, pak podle kódu, pak podle skupiny.")).small().color(SEDA));
        });
        if puvod == Puvod::Rucne && ui.button(tr("Zrušit ruční styl")).on_hover_text(tr("bod převezme styl podle kódu nebo skupiny")).clicked()
        {
            spravce::posli(Akce::StylBodu(vec![b.handle], String::new()));
        }
        ui.separator();

        // skupiny a povrchy
        sekce(ui, tr("Patří do skupin"));
        ui.horizontal_wrapped(|ui| {
            for g in stav.skupiny.iter().filter(|g| g.skupina.obsahuje(&b.bod, &b.zdroj)) {
                if ui.button(&g.skupina.jmeno).on_hover_text(tr("otevřít skupinu")).clicked() {
                    self.rezim = Rezim::Vykres;
                    self.vyber_uzel(Uzel::Skupina(g.skupina.id.clone()));
                }
            }
        });
        if !stav.povrchy.is_empty() {
            sekce(ui, tr("V povrchu"));
            for p in &stav.povrchy {
                let ve_skupine = p.skupiny.iter().filter(|g| g.zapnuto && !g.chybi).any(|g| {
                    stav.skupiny.iter().any(|s| s.skupina.id == g.id && s.skupina.obsahuje(&b.bod, &b.zdroj))
                });
                let vylouceny = !kod.is_empty() && kody_z_textu(&p.vyloucene_kody).contains(&kod);
                ui.horizontal(|ui| {
                    let (t, barva) = match (ve_skupine, vylouceny) {
                        (true, false) => (tr("použit").to_string(), ZELENA),
                        (true, true) => (trf!("vyloučen (kód {})", kod), ORANZOVA),
                        _ => (tr("není v jeho skupinách").to_string(), SEDA),
                    };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, barva);
                    ui.label(&p.jmeno);
                    ui.label(RichText::new(t).color(barva));
                });
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Nová skupina z bodu")).clicked() {
                spravce::posli(Akce::SkupinaNova(vec![b.handle]));
            }
            if ui.button(tr("Otevřít v tabulce")).clicked() {
                self.otevri_v_tabulce(stav, &[b.handle]);
            }
            if ui.button(RichText::new(tr("Smazat bod")).color(CERVENA)).clicked() {
                super::smazat(super::pocet_smazanych(1), Akce::BodySmazat(vec![b.handle]));
            }
        });
    }

    fn otevri_v_tabulce(&mut self, stav: &StavSpravce, h: &[Handle]) {
        self.rezim = Rezim::Vykres;
        self.vybrane_body = h.iter().copied().collect();
        self.posunout_na = h.first().copied();
        self.hledat.clear();
        self.filtr_skupina = None;
        self.filtr_kody.clear();
        self.vyber_uzel(Uzel::Body);
        self.nacti_edit_bodu(stav);
    }

    fn vyber_vice_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, body: &[&BodVykresu]) {
        let h: Vec<Handle> = body.iter().map(|b| b.handle).collect();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(trf!("Vybráno: {}", super::pocet_bodu_text(body.len()))).size(18.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Ukázat")).clicked() {
                    ukazat_body(stav, &h);
                }
            });
        });
        ui.separator();
        // styly vybraných bodů
        let mut pocty: Vec<(String, usize)> = Vec::new();
        let mut puvody: Vec<(String, usize)> = Vec::new();
        for b in body {
            let (id, puvod) = stav.styl_bodu.get(&b.handle).cloned().unwrap_or((styly::STANDARD.into(), Puvod::Standard));
            match pocty.iter_mut().find(|x| x.0 == id) {
                Some(x) => x.1 += 1,
                None => pocty.push((id, 1)),
            }
            let p = match puvod {
                Puvod::Rucne => tr("ručně"),
                Puvod::Kod(_) => tr("podle kódu"),
                Puvod::Skupina(_) => tr("podle skupiny"),
                Puvod::Standard => tr("výchozí"),
            }
            .to_string();
            match puvody.iter_mut().find(|x| x.0 == p) {
                Some(x) => x.1 += 1,
                None => puvody.push((p, 1)),
            }
        }
        pocty.sort_by(|a, b| b.1.cmp(&a.1));
        sekce(ui, tr("Styl"));
        let spolecny = (pocty.len() == 1).then(|| styl_podle_id(stav, &pocty[0].0)).flatten();
        let text = match spolecny {
            Some(s) => s.jmeno.clone(),
            None => trf!("různé ({} stylů)", pocty.len()),
        };
        ui.horizontal(|ui| {
            if let Some(v) = combo_stylu_bodu(ui, "vyber_styl_vice", stav, spolecny, &text) {
                spravce::posli(Akce::StylBodu(h.clone(), v));
            }
        });
        for (id, n) in &pocty {
            ui.horizontal(|ui| {
                if let Some(st) = styl_podle_id(stav, id) {
                    ikona_stylu(ui, st, 18.0);
                    ui.label(&st.jmeno);
                }
                ui.label(RichText::new(format!("{n}×")).color(SEDA));
            });
        }
        let p: Vec<String> = puvody.iter().map(|(t, n)| format!("{t} {n}")).collect();
        ui.label(RichText::new(trf!("Odkud: {}", p.join(", "))).small().color(SEDA));
        ui.separator();
        sekce(ui, tr("Hromadně"));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Nová skupina z vybraných")).clicked() {
                spravce::posli(Akce::SkupinaNova(h.clone()));
            }
            if ui.button(tr("Otevřít v tabulce")).on_hover_text(tr("přečíslování, přidání do skupiny…")).clicked() {
                self.otevri_v_tabulce(stav, &h);
            }
            if ui.button(RichText::new(trf!("Smazat ({})", h.len())).color(CERVENA)).clicked() {
                super::smazat(super::pocet_smazanych(h.len()), Akce::BodySmazat(h.clone()));
            }
        });
    }

    fn vyber_povrch(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&p.jmeno).size(18.0).strong());
                let mut t = trf!("{} trojúhelníků", p.stat.trojuhelniku);
                if p.aktivni {
                    t = trf!("▶ aktivní povrch · {}", t);
                }
                ui.label(RichText::new(t).small().color(if p.aktivni { MODRA } else { SEDA }));
            });
        });
        if p.zastaraly {
            ramecek(ui, ZLUTA_TMAVA, Color32::from_rgb(107, 84, 32), |ui| {
                ui.label(RichText::new(tr("⚠ Zastaralý – zdroje se od posledního přepočtu změnily")).color(Color32::from_rgb(243, 215, 154)));
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new(tr("⟳ Přepočítat")).strong()).clicked() {
                spravce::posli(Akce::Prepocitat(vec![id.clone()]));
            }
            if ui.button(RichText::new(tr("Prohlédnout ve 3D")).strong()).clicked() {
                self.pohled.otevri(vec![id.clone()]);
                self.rezim = Rezim::Pohled3D;
            }
            if !p.aktivni && ui.button(tr("Nastavit jako aktivní")).clicked() {
                spravce::posli(Akce::Aktivovat(id.clone()));
            }
            if ui.button(tr("Ve stromu")).clicked() {
                self.rezim = Rezim::Vykres;
                self.vyber_uzel(Uzel::Povrch(id.clone()));
            }
            if ui.button(RichText::new(tr("Smazat")).color(CERVENA)).clicked() {
                super::smazat(trf!("Povrch „{}“ je smazaný.", p.jmeno), Akce::Smazat(id.clone()));
            }
        });
        if self.vyber_povrch_id != p.id {
            self.vyber_povrch_id = p.id.clone();
            self.kody = p.vyloucene_kody.clone();
        }
        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new(tr("Zobrazení")).strong()).id_salt("vp_zobrazeni").default_open(true).show(ui, |ui| {
            self.nastaveni_povrchu(ui, stav, p);
        });
        let pocet = |f: &dyn Fn(&DruhPolozky) -> bool| p.polozky.iter().filter(|x| f(&x.druh)).count();
        let n_def = p.skupiny.len() + p.polozky.len() + p.upravy.len();
        egui::CollapsingHeader::new(RichText::new(trf!("Z čeho se povrch počítá ({})", n_def)).strong())
            .id_salt("vp_definice")
            .default_open(false)
            .show(ui, |ui| {
                for g in &p.skupiny {
                    ui.horizontal(|ui| {
                        let mut zap = g.zapnuto;
                        if ui.checkbox(&mut zap, trf!("Skupina {}", g.jmeno)).changed() {
                            spravce::posli(Akce::SkupinaVPovrchu(id.clone(), g.id.clone(), Some(zap)));
                        }
                        ui.label(RichText::new(trf!("{} bodů", g.pocet)).color(SEDA));
                        if g.chybi {
                            ui.label(RichText::new(tr("chybí ve výkrese")).color(ORANZOVA));
                        }
                    });
                }
                for (c, n) in [
                    (Cast::Hranice, pocet(&|d| matches!(d, DruhPolozky::Hranice | DruhPolozky::Dira))),
                    (Cast::Spojnice, pocet(&|d| matches!(d, DruhPolozky::Spojnice))),
                    (Cast::Objekty, pocet(&|d| matches!(d, DruhPolozky::Objekt(_)))),
                    (Cast::Upravy, p.upravy.len()),
                ] {
                    let t = RichText::new(format!("{} ({n})", c.nazev())).color(if n == 0 { SEDA } else { MODRA });
                    if ui.link(t).clicked() {
                        self.rezim = Rezim::Vykres;
                        self.vyber_uzel(Uzel::Definice(id.clone(), c));
                    }
                }
            });
        let s = &p.stat;
        egui::CollapsingHeader::new(RichText::new(trf!("Statistika  ·  {} – {} m n. m.", super::cz(s.z_min, 2), super::cz(s.z_max, 2))).strong())
            .id_salt("vp_stat")
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("vp_stat_g").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.label(tr("Body"));
                    ui.label(trf!("{} (vyloučeno podle kódu {})", p.pocet_bodu, p.vylouceno_bodu));
                    ui.end_row();
                    ui.label(tr("Trojúhelníky"));
                    ui.label(s.trojuhelniku.to_string());
                    ui.end_row();
                    ui.label(tr("Plocha"));
                    ui.label(format!("2D {} m²,  3D {} m²", super::cz(s.plocha_2d, 1), super::cz(s.plocha_3d, 1)));
                    ui.end_row();
                });
            });
        let n = p.upozorneni();
        if n > 0 {
            egui::CollapsingHeader::new(RichText::new(trf!("⚠ Upozornění ({})", n)).color(ORANZOVA).strong())
                .id_salt("vp_upoz")
                .default_open(false)
                .show(ui, |ui| detail_upozorneni(ui, p));
        }
    }

    fn vyber_vice_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, povrchy: &[&PrehledPovrchu]) {
        let ids: Vec<String> = povrchy.iter().map(|p| p.id.clone()).collect();
        ui.add_space(6.0);
        ui.label(RichText::new(trf!("Vybráno {} povrchů", povrchy.len())).size(18.0).strong());
        for p in povrchy {
            ui.label(format!("• {} – {}", p.jmeno, nazev_stylu_povrchu(stav, &p.styl)));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tr("Styl všem"));
            egui::ComboBox::from_id_salt("vp_styl_vice").selected_text(tr("vyberte…")).width(200.0).show_ui(ui, |ui| {
                for st in &stav.styly_povrchu {
                    if ui.selectable_label(false, &st.jmeno).clicked() {
                        spravce::posli(Akce::Nastav(ids.clone(), Zmena::Styl(st.id.clone())));
                    }
                }
            });
        });
        if ui.button(tr("⟳ Přepočítat vybrané")).clicked() {
            spravce::posli(Akce::Prepocitat(ids));
        }
    }

    /// Postup „Jak na to“ v uzlu Výkres.
    pub(super) fn postup(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let ma_cary = stav.povrchy.iter().any(|p| p.polozky.iter().any(|x| matches!(x.druh, DruhPolozky::Hranice | DruhPolozky::Spojnice)));
        let ma_upravy = stav.povrchy.iter().any(|p| !p.upravy.is_empty());
        let ma_styly = stav.styl_bodu.values().any(|(_, p)| *p != Puvod::Standard);
        let zdroje = stav.zdroje.join(", ");
        let kroky: [(bool, &str, String); 5] = [
            (
                !stav.body.is_empty(),
                tr("Import bodů"),
                if stav.body.is_empty() {
                    tr("Tlačítko Import bodů na záložce Terén; formát se rozpozná sám.").into()
                } else {
                    trf!("{} bodů · {}", stav.body.len(), zdroje)
                },
            ),
            (
                !stav.povrchy.is_empty(),
                tr("Nový povrch"),
                tr("Stávající terén ze skupiny Zaměření (bez kódů STR).").into(),
            ),
            (ma_cary, tr("Hranice a spojnice"), tr("Vyberte křivky ve výkrese a přidejte je jako hranici nebo spojnice (záložka Terén, skupina Definice povrchu).").into()),
            (ma_upravy, tr("Úpravy trojúhelníků"), tr("Otočit, přidat nebo smazat hranu – projeví se hned po kliknutí.").into()),
            (ma_styly, tr("Styly"), tr("Symboly podle kódů (např. STR = strom), vrstevnice a popisy.").into()),
        ];
        let prvni_neudelany = kroky.iter().position(|k| !k.0);
        ui.label(RichText::new(tr("Jak na to")).strong());
        for (i, (hotovo, nazev, popis)) in kroky.iter().enumerate() {
            let aktualni = prvni_neudelany == Some(i);
            let (pozadi, ram) = if *hotovo {
                (ZELENA_TMAVA, ZELENA_RAM)
            } else if aktualni {
                (Color32::from_rgb(27, 37, 49), Color32::from_rgb(61, 143, 214))
            } else {
                (Color32::from_rgb(35, 35, 35), Color32::from_rgb(51, 51, 51))
            };
            ramecek(ui, pozadi, ram, |ui| {
                ui.horizontal(|ui| {
                    let znak = if *hotovo { "✔".to_string() } else { (i + 1).to_string() };
                    let barva = if *hotovo { ZELENA } else if aktualni { MODRA } else { SEDA };
                    ui.label(RichText::new(znak).strong().size(16.0).color(barva));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(*nazev).strong());
                        ui.label(RichText::new(popis).small().color(SEDA));
                        if aktualni {
                            match i {
                                1 => {
                                    if ui.button(tr("Vytvořit stávající terén")).clicked() {
                                        spravce::posli(Akce::NovyPovrchPas("Stavajici".into(), tr("Stávající terén").into(), true));
                                    }
                                }
                                3 => {
                                    ui.label(RichText::new(tr("Nástroje jsou na pásu karet: Terén › Úpravy trojúhelníků")).small().color(SEDA));
                                }
                                4 => {
                                    if ui.button(tr("Styly bodů")).clicked() {
                                        self.vyber_uzel(Uzel::StylyBodu);
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
                });
            });
        }
        let mut skryt = false;
        if ui.checkbox(&mut skryt, tr("Už neukazovat")).changed() {
            nastaveni::zmen(|n| n.postup_skryt = true);
        }
    }
}
