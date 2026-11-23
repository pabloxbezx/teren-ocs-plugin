//! Trasy, nivelety a pohledy podélných profilů v Prospektoru: větev stromu
//! a karty (přehled tras, trasa, pohled).

use crate::lang::tr;
use teren::trf;

use eframe::egui::{self, RichText, Sense};
use egui::collapsing_header::CollapsingState;

use super::strom::{ik, polozka, Ikona, Vpravo};
use super::{cz, czi, karta, shoda, smazat, Okno, Uzel, CERVENA, MODRA, ORANZOVA, SEDA, ZELENA};
use crate::spravce::{self, Akce, StavSpravce};
use crate::tra::prospektor::{PrehledPohledu, PrehledTrasy};

// Příkaz pluginu Prospektor spustit neumí (Open CAD Studio ho z jiného
// vlákna nepustí) – objekt vybere a poradí tlačítko na pásu karet.
const N_VLASTNOSTI: &str = "Trasa je vybraná ve výkrese – výjimky stylu a vrcholy upravíte tlačítkem Vlastnosti trasy na pásu karet Terén.";
const N_POHLED: &str = "Trasa je vybraná ve výkrese – na pásu karet Terén klikněte na Podélný profil a potom do výkresu, kam profil umístit.";
const N_Z_POLYLINIE: &str = "Na pásu karet Terén klikněte na Nová trasa › Z polylinie a potom na polylinii osy.";
const N_KRESLIT: &str = "Na pásu karet Terén klikněte na Nová trasa › Kreslit vrcholy a klikejte vrcholy trasy.";
const N_NIV_TABULKA: &str = "Pohled je vybraný ve výkrese – na pásu karet Terén klikněte na Vrcholy nivelety.";
const N_NIVELETA: &str = "Pohled je vybraný ve výkrese – na pásu karet Terén klikněte na Niveleta a naklikejte vrcholy v pohledu.";

fn navod(h: Vec<ocs_plugin_api::host::Handle>, text: &str) {
    spravce::posli(Akce::Navod(h, text.to_string()));
}

fn stanice(s: f64) -> String {
    trasy::stanice::staniceni(s, trasy::stanice::FormatStanice::Km, 2, !crate::lang::anglicky())
}

fn ikona_trasy() -> Ikona {
    Ikona::Svg(ik!("tra_editor"))
}

fn ikona_pohledu() -> Ikona {
    Ikona::Svg(ik!("tra_profil"))
}

fn ikona_nivelety() -> Ikona {
    Ikona::Svg(ik!("tra_niveleta"))
}

fn nazev_pohledu(p: &PrehledPohledu) -> String {
    trf!("Pohled {} · {}", "View {} · {}", p.id, p.povrch)
}

fn popis_trasy(t: &PrehledTrasy) -> String {
    match &t.chyba {
        Some(e) => trf!("chyba geometrie: {}", "geometry error: {}", e),
        None => trf!(
            "{} m · {} vrcholů · {} oblouků · {}", "{} m · {} vertices · {} curves · {}",
            cz(t.delka, 2),
            t.vrcholu,
            t.oblouku,
            t.styl
        ),
    }
}

/// Stav trasy pro řádek seznamu: (text, barva).
fn stav_trasy(t: &PrehledTrasy) -> (String, egui::Color32) {
    if t.chyba.is_some() {
        (tr("chyba", "error").into(), CERVENA)
    } else if t.pohledy.iter().any(|p| p.zastaraly) {
        (tr("profil zastaralý", "profile outdated").into(), ORANZOVA)
    } else {
        (tr("aktuální", "up to date").into(), ZELENA)
    }
}

impl Okno {
    /// Větev stromu Trasy (v sekci Data).
    pub(super) fn strom_tras(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let h = self.hledat.clone();
        let trasy: Vec<&PrehledTrasy> = stav.trasy.iter().filter(|t| shoda(&t.jmeno, &h)).collect();
        let zastarale = stav.trasy.iter().flat_map(|t| &t.pohledy).filter(|p| p.zastaraly).count();
        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new("u_trasy"), true)
            .show_header(ui, |ui| {
                let mut vpravo = Vec::new();
                if zastarale > 0 {
                    vpravo.push(Vpravo::Tecka(ORANZOVA));
                }
                vpravo.push(Vpravo::Pocet(stav.trasy.len().to_string()));
                let r = polozka(ui, self.uzel == Uzel::Trasy, &ikona_trasy(), RichText::new(tr("Trasy", "Alignments")), &vpravo, false);
                let r = if zastarale > 0 { r.on_hover_text(trf!("zastaralé pohledy profilu: {}", "outdated profile views: {}", zastarale)) } else { r };
                if r.clicked() {
                    self.vyber_uzel(Uzel::Trasy);
                }
                r.context_menu(|ui| menu_tras(ui, stav));
            })
            .body(|ui| {
                if trasy.is_empty() {
                    ui.label(RichText::new(if stav.pripojeno { tr("zatím žádná trasa", "no alignment yet") } else { tr("načítám…", "loading…") }).color(SEDA).small());
                }
                for t in trasy {
                    self.uzel_trasy(ui, stav, t);
                }
            });
    }

    fn uzel_trasy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, t: &PrehledTrasy) {
        let u = Uzel::Trasa(t.id.clone());
        let vybrana = stav.vyber_trasy.contains(&t.id);
        let deti = t.nivelety.len() + t.pohledy.len();
        let radek = |s: &mut Self, ui: &mut egui::Ui| {
            let mut vpravo = Vec::new();
            if t.chyba.is_some() {
                vpravo.push(Vpravo::Tecka(CERVENA));
            } else if t.pohledy.iter().any(|p| p.zastaraly) {
                vpravo.push(Vpravo::Tecka(ORANZOVA));
            }
            if vybrana {
                vpravo.push(Vpravo::Tecka(MODRA));
            }
            let r = polozka(ui, s.uzel == u, &ikona_trasy(), RichText::new(&t.jmeno), &vpravo, false).on_hover_text(popis_trasy(t));
            if r.clicked() {
                s.vyber_uzel(u.clone());
            }
            if r.double_clicked() {
                spravce::posli(Akce::Ukazat(t.stred, t.polomer, vec![t.insert]));
            }
            r.context_menu(|ui| s.menu_trasy(ui, t));
        };
        if deti == 0 {
            radek(self, ui);
            return;
        }
        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new(("u_trasa", t.id.as_str())), false)
            .show_header(ui, |ui| radek(self, ui))
            .body(|ui| {
                for n in &t.nivelety {
                    let r = polozka(
                        ui,
                        false,
                        &ikona_nivelety(),
                        RichText::new(trf!("Niveleta {}", "Design profile {}", n.jmeno)),
                        &[Vpravo::Pocet(trf!("{} vrch.", "{} vert.", n.vrcholu))],
                        false,
                    );
                    if r.clicked() {
                        self.vyber_uzel(u.clone());
                    }
                }
                for p in &t.pohledy {
                    let up = Uzel::Pohled(p.id.clone());
                    let mut vpravo = Vec::new();
                    if p.zastaraly {
                        vpravo.push(Vpravo::Tecka(ORANZOVA));
                    }
                    if stav.vyber_pohledy.contains(&p.id) {
                        vpravo.push(Vpravo::Tecka(MODRA));
                    }
                    let r = polozka(ui, self.uzel == up, &ikona_pohledu(), RichText::new(nazev_pohledu(p)), &vpravo, false);
                    let r = if p.zastaraly { r.on_hover_text(tr("trasa nebo povrch se změnily – pohled je potřeba překreslit", "the alignment or surface changed – the view needs to be redrawn")) } else { r };
                    if r.clicked() {
                        self.vyber_uzel(up.clone());
                    }
                    if r.double_clicked() {
                        spravce::posli(Akce::Ukazat(p.stred, p.polomer, vec![p.insert]));
                    }
                    r.context_menu(|ui| menu_pohledu(ui, t, p));
                }
            });
    }

    fn menu_trasy(&mut self, ui: &mut egui::Ui, t: &PrehledTrasy) {
        if ui.button(tr("Ukázat ve výkrese", "Show in the drawing")).clicked() {
            spravce::posli(Akce::Ukazat(t.stred, t.polomer, vec![t.insert]));
            ui.close();
        }
        if ui.button(tr("Editor trasy", "Alignment editor")).clicked() {
            spravce::posli(Akce::TrasaEditor(t.insert));
            ui.close();
        }
        if ui.button(tr("Vlastnosti trasy…", "Alignment properties…")).clicked() {
            navod(vec![t.insert], crate::lang::tr_tab(N_VLASTNOSTI));
            ui.close();
        }
        if ui.button(tr("Přejmenovat…", "Rename…")).clicked() {
            self.vyber_uzel(Uzel::Trasa(t.id.clone()));
            self.jmeno_trasy = (t.id.clone(), t.jmeno.clone(), t.pocatek, t.styl_id.clone());
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Nový podélný profil…", "New longitudinal profile…")).clicked() {
            navod(vec![t.insert], crate::lang::tr_tab(N_POHLED));
            ui.close();
        }
        ui.separator();
        if ui.button(RichText::new(tr("Smazat trasu", "Delete alignment")).color(CERVENA)).clicked() {
            smazat_trasu(t);
            self.vyber_uzel(Uzel::Trasy);
            ui.close();
        }
    }

    /// Karta se seznamem tras.
    pub(super) fn detail_trasy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        karta::nadpis_karty(ui, &ikona_trasy(), tr("Trasy", "Alignments"), |ui| {
            karta::pilulka(ui, &stav.trasy.len().to_string(), SEDA);
        });
        ui.add_space(4.0);
        if stav.trasy.is_empty() {
            karta::prazdny_stav(
                ui,
                ik!("tra_editor"),
                tr("Ve výkrese zatím není žádná trasa", "There is no alignment in the drawing yet"),
                tr("Trasu (osu) vytvoříte z polylinie, nebo ji nakreslíte klikáním. Potom k ní přidáte podélný profil a niveletu.", "Create an alignment from a polyline or draw it by clicking. Then add a longitudinal profile and a design profile."),
                |ui| {
                    if karta::tlacitko(ui, Some(ik!("tra_z_polylinie")), tr("Trasa z polylinie", "Alignment from polyline"), true).clicked() {
                        navod(Vec::new(), crate::lang::tr_tab(N_Z_POLYLINIE));
                    }
                    if karta::tlacitko(ui, Some(ik!("tra_kreslit")), tr("Nakreslit trasu", "Draw alignment"), false).clicked() {
                        navod(Vec::new(), crate::lang::tr_tab(N_KRESLIT));
                    }
                },
            );
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(ik!("tra_z_polylinie")), tr("Trasa z polylinie", "Alignment from polyline"), true).clicked() {
                navod(Vec::new(), crate::lang::tr_tab(N_Z_POLYLINIE));
            }
            if karta::tlacitko(ui, Some(ik!("tra_kreslit")), tr("Nakreslit trasu", "Draw alignment"), false).clicked() {
                navod(Vec::new(), crate::lang::tr_tab(N_KRESLIT));
            }
            if karta::tlacitko(ui, Some(ik!("tra_profil")), tr("Nový podélný profil…", "New longitudinal profile…"), false).clicked() {
                navod(Vec::new(), crate::lang::tr_tab(N_POHLED));
            }
            let zastarale = stav.trasy.iter().flat_map(|t| &t.pohledy).filter(|p| p.zastaraly).count();
            if zastarale > 0 && karta::tlacitko(ui, Some(ik!("tra_prepocitat")), &trf!("Překreslit zastaralé pohledy ({})", "Redraw outdated views ({})", zastarale), false).clicked() {
                spravce::posli(Akce::PohledyPrekreslit(None));
            }
        });
        ui.add_space(6.0);
        let hledat = self.hledat.clone();
        egui::ScrollArea::vertical().id_salt("seznam_tras").auto_shrink([false, false]).show(ui, |ui| {
            for t in stav.trasy.iter().filter(|t| shoda(&t.jmeno, &hledat)) {
                let (st, barva) = stav_trasy(t);
                let mut popis = popis_trasy(t);
                if !t.pohledy.is_empty() || !t.nivelety.is_empty() {
                    popis.push_str(&trf!(" · profilů {} · nivelet {}", " · profiles {} · design profiles {}", t.pohledy.len(), t.nivelety.len()));
                }
                let r = karta::radek_seznamu(ui, &ikona_trasy(), &t.jmeno, &popis, &st, barva);
                if r.clicked() {
                    self.vyber_uzel(Uzel::Trasa(t.id.clone()));
                }
                if r.double_clicked() {
                    spravce::posli(Akce::Ukazat(t.stred, t.polomer, vec![t.insert]));
                }
                r.context_menu(|ui| self.menu_trasy(ui, t));
            }
        });
    }

    /// Karta jedné trasy.
    pub(super) fn karta_trasy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        let Some(t) = stav.trasy.iter().find(|t| t.id == id).cloned() else {
            ui.label(RichText::new(tr("Trasa už ve výkrese není.", "The alignment is no longer in the drawing.")).color(SEDA));
            return;
        };
        let (st, barva) = stav_trasy(&t);
        karta::nadpis_karty(ui, &ikona_trasy(), &t.jmeno, |ui| {
            karta::pilulka(ui, &format!("● {st}"), barva);
            karta::pilulka(ui, &t.styl, SEDA).on_hover_text(if t.vyjimek > 0 {
                trf!("styl trasy, výjimek objektu: {}", "alignment style, object overrides: {}", t.vyjimek)
            } else {
                tr("styl trasy", "alignment style").to_string()
            });
            if stav.vyber_trasy.contains(&t.id) {
                karta::pilulka(ui, tr("vybraná ve výkrese", "selected in the drawing"), MODRA);
            }
        });
        if let Some(e) = &t.chyba {
            ui.label(RichText::new(trf!("Geometrie trasy nejde spočítat: {}", "The alignment geometry cannot be computed: {}", e)).color(CERVENA));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            karta::kpi(ui, &format!("{} m", cz(t.delka, 2)), tr("délka", "length"));
            karta::kpi(ui, &format!("{} – {}", stanice(t.pocatek), stanice(t.pocatek + t.delka)), tr("staničení", "stations"));
            karta::kpi(ui, &t.vrcholu.to_string(), tr("vrcholů", "vertices"));
            karta::kpi(ui, &t.oblouku.to_string(), tr("oblouků", "curves"));
            if let Some(r) = t.min_r {
                karta::kpi(ui, &format!("{} m", cz(r, 1)), tr("nejmenší R", "smallest R"));
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(ik!("tra_editor")), tr("Editor trasy", "Alignment editor"), true).clicked() {
                spravce::posli(Akce::TrasaEditor(t.insert));
            }
            if karta::tlacitko(ui, Some(ik!("tra_vlastnosti")), tr("Vlastnosti…", "Properties…"), false).clicked() {
                navod(vec![t.insert], crate::lang::tr_tab(N_VLASTNOSTI));
            }
            if karta::tlacitko(ui, None, tr("Ukázat ve výkrese", "Show in the drawing"), false).clicked() {
                spravce::posli(Akce::Ukazat(t.stred, t.polomer, vec![t.insert]));
            }
            if karta::tlacitko(ui, Some(ik!("tra_smazat")), tr("Smazat", "Delete"), false).clicked() {
                smazat_trasu(&t);
                self.vyber_uzel(Uzel::Trasy);
            }
        });

        // vlastnosti
        ui.add_space(10.0);
        if self.jmeno_trasy.0 != t.id {
            self.jmeno_trasy = (t.id.clone(), t.jmeno.clone(), t.pocatek, t.styl_id.clone());
        }
        let styly: Vec<(String, String)> = stav.styly_tras.iter().map(|s| (s.id.clone(), crate::lang::tr_dyn(&s.nazev))).collect();
        egui::Grid::new(("vlastnosti_trasy", t.id.as_str())).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Jméno", "Name"));
            ui.add(egui::TextEdit::singleline(&mut self.jmeno_trasy.1).desired_width(220.0));
            ui.end_row();
            ui.label(tr("Počáteční staničení", "Start station"));
            ui.add(egui::DragValue::new(&mut self.jmeno_trasy.2).speed(1.0).max_decimals(3).suffix(" m"));
            ui.end_row();
            ui.label(tr("Styl", "Style"));
            let nazev = styly.iter().find(|(id, _)| id == &self.jmeno_trasy.3).map_or_else(|| self.jmeno_trasy.3.clone(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt(("styl_trasy", t.id.as_str())).selected_text(nazev).show_ui(ui, |ui| {
                for (id, n) in &styly {
                    ui.selectable_value(&mut self.jmeno_trasy.3, id.clone(), n);
                }
            });
            ui.end_row();
        });
        let e = self.jmeno_trasy.clone();
        let zmeneno = !e.1.trim().is_empty() && (e.1.trim() != t.jmeno || (e.2 - t.pocatek).abs() > 1e-9 || e.3 != t.styl_id);
        ui.horizontal(|ui| {
            if ui.add_enabled(zmeneno, egui::Button::new(tr("Uložit změny", "Save changes"))).clicked() {
                spravce::posli(Akce::TrasaNastav(t.insert, e.1.trim().to_string(), e.2, e.3.clone()));
            }
            if zmeneno && ui.button(tr("Vrátit", "Revert")).clicked() {
                self.jmeno_trasy.0.clear();
            }
            if ui.button(tr("Uložit jako nový styl…", "Save as new style…")).on_hover_text(tr("vzhled této trasy (styl a výjimky) jako nový styl trasy ve výkrese", "this alignment's appearance (style and overrides) as a new alignment style in the drawing")).clicked() {
                spravce::posli(Akce::UlozStylTrasy(Box::new(styl_z_trasy(&t, &stav.styly_tras))));
            }
            if ui.button(tr("Upravit styl", "Edit style")).clicked() {
                self.vyber_uzel(Uzel::StylTrasy(t.styl_id.clone()));
            }
        });

        // podélné profily
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Podélné profily", "Longitudinal profiles")).strong());
            if ui.small_button(tr("+ nový", "+ new")).on_hover_text(tr("vloží nový pohled podélného profilu této trasy", "inserts a new longitudinal profile view of this alignment")).clicked() {
                navod(vec![t.insert], crate::lang::tr_tab(N_POHLED));
            }
        });
        if t.pohledy.is_empty() {
            ui.label(RichText::new(tr("Trasa zatím nemá podélný profil.", "The alignment has no longitudinal profile yet.")).color(SEDA));
        }
        for p in &t.pohledy {
            let (st, barva) = if p.zastaraly { (stitek_zastaraleho(false), ORANZOVA) } else { (tr("aktuální", "up to date"), ZELENA) };
            let popis = popis_pohledu(p);
            let r = karta::radek_seznamu(ui, &ikona_pohledu(), &nazev_pohledu(p), &popis, st, barva);
            if r.clicked() {
                self.vyber_uzel(Uzel::Pohled(p.id.clone()));
            }
            if r.double_clicked() {
                spravce::posli(Akce::Ukazat(p.stred, p.polomer, vec![p.insert]));
            }
            r.context_menu(|ui| menu_pohledu(ui, &t, p));
        }

        // nivelety
        ui.add_space(12.0);
        ui.label(RichText::new(tr("Nivelety", "Design profiles")).strong());
        if t.nivelety.is_empty() {
            ui.label(RichText::new(if t.pohledy.is_empty() {
                tr("Niveletu nakreslíte v podélném profilu (nejdřív vložte profil).", "Draw the design profile in a longitudinal profile (insert a profile first).")
            } else {
                tr("Trasa zatím nemá niveletu – nakreslete ji tlačítkem Niveleta v pohledu profilu.", "The alignment has no design profile yet – draw one with the Design profile button in a profile view.")
            })
            .color(SEDA));
        }
        for n in &t.nivelety {
            let sklony: Vec<String> = n.sklony.iter().map(|i| format!("{} ‰", cz(*i, 1))).collect();
            let popis = trf!(
                "{} vrcholů · {} oblouků · {} – {} · výšky {} – {} m · sklony {}", "{} vertices · {} curves · {} – {} · elevations {} – {} m · grades {}",
                n.vrcholu,
                n.oblouku,
                stanice(n.od),
                stanice(n.do_),
                cz(n.z_min, 2),
                cz(n.z_max, 2),
                sklony.join(", ")
            );
            let r = karta::radek_seznamu(ui, &ikona_nivelety(), &trf!("Niveleta {}", "Design profile {}", n.jmeno), &popis, "", SEDA);
            r.context_menu(|ui| {
                if let Some(p) = t.pohledy.first() {
                    if ui.button(tr("Vrcholy nivelety…", "Design profile vertices…")).clicked() {
                        navod(vec![p.insert], crate::lang::tr_tab(N_NIV_TABULKA));
                        ui.close();
                    }
                }
            });
        }
    }

    /// Karta pohledu podélného profilu.
    pub(super) fn karta_pohledu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        let Some((t, p)) = stav.trasy.iter().find_map(|t| t.pohledy.iter().find(|p| p.id == id).map(|p| (t.clone(), p.clone()))) else {
            ui.label(RichText::new(tr("Pohled profilu už ve výkrese není.", "The profile view is no longer in the drawing.")).color(SEDA));
            return;
        };
        karta::nadpis_karty(ui, &ikona_pohledu(), &trf!("Podélný profil {}", "Longitudinal profile {}", p.id), |ui| {
            let (st, barva) = if p.zastaraly { (stitek_zastaraleho(true), ORANZOVA) } else { (tr("● aktuální", "● up to date"), ZELENA) };
            karta::pilulka(ui, st, barva);
            if stav.vyber_pohledy.contains(&p.id) {
                karta::pilulka(ui, tr("vybraný ve výkrese", "selected in the drawing"), MODRA);
            }
        });
        if p.zastaraly {
            ui.label(RichText::new(tr("Trasa nebo povrch se od posledního kreslení změnily – pohled je potřeba překreslit.", "The alignment or surface changed since the last drawing – the view needs to be redrawn.")).color(ORANZOVA));
        }
        if p.povrch_chybi {
            ui.label(RichText::new(trf!("Povrch {} ve výkrese není – terénní profil nejde spočítat.", "Surface {} is not in the drawing – the ground profile cannot be computed.", p.povrch)).color(CERVENA));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::kpi(ui, &t.jmeno, tr("trasa", "alignment")).interact(Sense::click()).on_hover_text(tr("otevřít trasu", "open alignment")).clicked() {
                self.vyber_uzel(Uzel::Trasa(t.id.clone()));
            }
            karta::kpi(ui, &p.povrch, tr("povrch", "surface"));
            if let Some((od, do_)) = p.rozsah {
                karta::kpi(ui, &format!("{} – {}", stanice(od), stanice(do_)), tr("staničení", "stations"));
            }
            karta::kpi(ui, &format!("1:{}", cz(p.meritko, 0)), tr("měřítko délek", "horizontal scale"));
            karta::kpi(ui, &format!("{}×", cz(p.prevyseni, 0)), tr("převýšení", "vertical exaggeration"));
            // pásy podle stylu (p.pasu je jen záloha uložená v pohledu)
            let pasu = efektivni(&p.def, &stav.styly_profilu).pasy.iter().filter(|x| !x.skryty).count();
            karta::kpi(ui, &czi(pasu), tr("pásů", "bands"));
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, None, tr("Ukázat ve výkrese", "Show in the drawing"), true).clicked() {
                spravce::posli(Akce::Ukazat(p.stred, p.polomer, vec![p.insert]));
            }
            if karta::tlacitko(ui, Some(ik!("tra_prepocitat")), tr("Překreslit", "Redraw"), false).clicked() {
                spravce::posli(Akce::PohledyPrekreslit(Some(p.insert)));
            }
            if karta::tlacitko(ui, Some(ik!("tra_niveleta")), tr("Nakreslit niveletu", "Draw design profile"), false)
                .on_hover_text(tr("klikáním vrcholů v tomto pohledu; nahradí dosavadní niveletu trasy", "by clicking vertices in this view; replaces the alignment's current design profile"))
                .clicked()
            {
                navod(vec![p.insert], crate::lang::tr_tab(N_NIVELETA));
            }
            if !t.nivelety.is_empty() && karta::tlacitko(ui, Some(ik!("tra_vlastnosti")), tr("Vrcholy nivelety…", "Design profile vertices…"), false).clicked() {
                navod(vec![p.insert], crate::lang::tr_tab(N_NIV_TABULKA));
            }
            if karta::tlacitko(ui, Some(ik!("tra_smazat")), tr("Smazat", "Delete"), false).clicked() {
                smazat(trf!("Pohled profilu {} je smazaný.", "Profile view {} has been deleted.", p.id), Akce::PohledSmazat(p.insert));
                self.vyber_uzel(Uzel::Trasa(t.id.clone()));
            }
        });
        ui.add_space(10.0);
        self.editor_pohledu(ui, stav, &p);
    }
}

fn popis_pohledu(p: &PrehledPohledu) -> String {
    let rozsah = p.rozsah.map_or_else(String::new, |(od, do_)| format!("{} – {} · ", stanice(od), stanice(do_)));
    trf!("{}1:{} · převýšení {}× · pásů {}", "{}1:{} · exaggeration {}× · bands {}", rozsah, cz(p.meritko, 0), cz(p.prevyseni, 0), p.pasu)
}

fn smazat_trasu(t: &PrehledTrasy) {
    let text = if t.pohledy.is_empty() {
        trf!("Trasa „{}“ je smazaná.", "Alignment “{}” has been deleted.", t.jmeno)
    } else {
        trf!("Trasa „{}“ je smazaná i s pohledy profilu ({}).", "Alignment “{}” has been deleted together with its profile views ({}).", t.jmeno, t.pohledy.len())
    };
    smazat(text, Akce::TrasaSmazat(t.insert));
}

fn menu_tras(ui: &mut egui::Ui, stav: &StavSpravce) {
    if ui.button(tr("Trasa z polylinie", "Alignment from polyline")).clicked() {
        navod(Vec::new(), crate::lang::tr_tab(N_Z_POLYLINIE));
        ui.close();
    }
    if ui.button(tr("Nakreslit trasu", "Draw alignment")).clicked() {
        navod(Vec::new(), crate::lang::tr_tab(N_KRESLIT));
        ui.close();
    }
    ui.separator();
    let zastarale = stav.trasy.iter().flat_map(|t| &t.pohledy).any(|p| p.zastaraly);
    if ui.add_enabled(zastarale, egui::Button::new(tr("Překreslit zastaralé pohledy", "Redraw outdated views"))).clicked() {
        spravce::posli(Akce::PohledyPrekreslit(None));
        ui.close();
    }
}

fn menu_pohledu(ui: &mut egui::Ui, t: &PrehledTrasy, p: &PrehledPohledu) {
    if ui.button(tr("Ukázat ve výkrese", "Show in the drawing")).clicked() {
        spravce::posli(Akce::Ukazat(p.stred, p.polomer, vec![p.insert]));
        ui.close();
    }
    if ui.button(tr("Překreslit", "Redraw")).clicked() {
        spravce::posli(Akce::PohledyPrekreslit(Some(p.insert)));
        ui.close();
    }
    if ui.button(tr("Nakreslit niveletu", "Draw design profile")).clicked() {
        navod(vec![p.insert], crate::lang::tr_tab(N_NIVELETA));
        ui.close();
    }
    if !t.nivelety.is_empty() && ui.button(tr("Vrcholy nivelety…", "Design profile vertices…")).clicked() {
        navod(vec![p.insert], crate::lang::tr_tab(N_NIV_TABULKA));
        ui.close();
    }
    ui.separator();
    if ui.button(RichText::new(tr("Smazat pohled", "Delete view")).color(CERVENA)).clicked() {
        smazat(trf!("Pohled profilu {} je smazaný.", "Profile view {} has been deleted.", p.id), Akce::PohledSmazat(p.insert));
        ui.close();
    }
}

// ── Editor vzhledu a pásů pohledu a stylu profilu ──────────────────────────

use trasy::pohled::{efektivni, nastaveni_s, DefinicePohledu, StylProfilu};

/// Štítek zastaralého pohledu: při odloženém přepočtu „čeká na přepočet“.
fn stitek_zastaraleho(tecka: bool) -> &'static str {
    let ceka = crate::prepocet::rezim() != crate::prepocet::Rezim::Automaticky;
    match (ceka, tecka) {
        (true, false) => tr("čeká na přepočet", "waiting for recalculation"),
        (true, true) => tr("● čeká na přepočet", "● waiting for recalculation"),
        (false, false) => tr("zastaralý", "out of date"),
        (false, true) => tr("● zastaralý", "● out of date"),
    }
}

fn jmeno_stylu(styly: &[StylProfilu], id: &str) -> String {
    styly.iter().find(|s| s.id == id).map_or_else(|| id.to_string(), |s| crate::lang::tr_dyn(&s.jmeno))
}

/// Nastavení pohledu převedené na styl (všechny hodnoty odlišné od schématu).
fn styl_z_definice(d: &DefinicePohledu, styly: &[StylProfilu], id: &str, jmeno: &str) -> StylProfilu {
    let n = nastaveni_s(d, styly);
    let hodnoty = trasy::pohled::SCHEMA
        .iter()
        .filter(|k| n.text(k.klic) != k.vychozi)
        .map(|k| (k.klic.to_string(), n.text(k.klic).to_string()))
        .collect();
    StylProfilu { id: id.into(), jmeno: jmeno.into(), hodnoty, pasy: d.pasy.clone(), meritko: d.meritko, prevyseni: d.prevyseni }
}

/// Název předlohy vzhledu podle nastavení.
fn nazev_predlohy(d: &DefinicePohledu, styly: &[StylProfilu]) -> String {
    let k = nastaveni_s(d, styly).text("predloha").to_string();
    trasy::vzhled::PREDLOHY.iter().find(|p| p.0 == k).map_or(k, |p| crate::lang::tr_tab(p.1).to_string())
}

impl Okno {
    /// Sekce „Vzhled a pásy“ v kartě pohledu: přehled a tlačítko editoru.
    pub(super) fn editor_pohledu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPohledu) {
        let styly = &stav.styly_profilu;
        let d = efektivni(&p.def, styly);
        ui.label(RichText::new(tr("Vzhled a pásy", "Appearance and bands")).strong());
        // výběr stylu profilu – pohled převezme styl celý (nastavení i pásy)
        ui.horizontal(|ui| {
            ui.label(tr("Styl profilu", "Profile style"));
            let mut vybrany = d.styl.clone();
            egui::ComboBox::from_id_salt(("styl_pohledu", p.id.as_str()))
                .selected_text(jmeno_stylu(styly, &d.styl))
                .width(220.0)
                .show_ui(ui, |ui| {
                    for s in styly {
                        ui.selectable_value(&mut vybrany, s.id.clone(), crate::lang::tr_dyn(&s.jmeno));
                    }
                })
                .response
                .on_hover_text(tr("pohled převezme vzhled i pásy vybraného stylu; vlastní úpravy tohoto pohledu se zruší (vrátí je Ctrl+Z)", "the view takes the look and bands of the selected style; this view's own changes are dropped (Ctrl+Z brings them back)"));
            if vybrany != d.styl {
                let mut nova = p.def.clone();
                nova.styl = vybrany;
                // vlastní úpravy pohledu se zruší, volba upraveného terénu zůstane
                let upr = nova.vyjimky.get("povrch_upraveny").cloned();
                nova.vyjimky.clear();
                if let Some(u) = upr {
                    nova.vyjimky.insert("povrch_upraveny".into(), u);
                }
                nova.pasy_ze_stylu = true;
                spravce::posli(Akce::PohledUpravit(p.insert, Box::new(nova)));
            }
        });
        // měřítko délek a převýšení
        ui.horizontal(|ui| {
            const MERITKA: [f64; 8] = [100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0];
            const PREVYSENI: [f64; 7] = [1.0, 2.0, 2.5, 5.0, 10.0, 20.0, 50.0];
            let mut meritko = p.def.meritko;
            let mut prevyseni = p.def.prevyseni;
            ui.label(tr("Měřítko 1:", "Scale 1:"));
            egui::ComboBox::from_id_salt(("meritko_pohledu", p.id.as_str()))
                .selected_text(cz(meritko, 0))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for m in MERITKA {
                        ui.selectable_value(&mut meritko, m, cz(m, 0));
                    }
                });
            ui.add(egui::DragValue::new(&mut meritko).range(10.0..=100_000.0).speed(0.0).max_decimals(0))
                .on_hover_text(tr("vlastní měřítko délek", "custom length scale"));
            ui.add_space(10.0);
            ui.label(tr("Převýšení", "Exaggeration"));
            egui::ComboBox::from_id_salt(("prevyseni_pohledu", p.id.as_str()))
                .selected_text(format!("{}×", cz(prevyseni, 1)))
                .width(60.0)
                .show_ui(ui, |ui| {
                    for v in PREVYSENI {
                        ui.selectable_value(&mut prevyseni, v, format!("{}×", cz(v, 1)));
                    }
                });
            ui.add(egui::DragValue::new(&mut prevyseni).range(0.1..=200.0).speed(0.0).max_decimals(1).suffix("×"))
                .on_hover_text(tr("vlastní převýšení (měřítko výšek = měřítko délek / převýšení)", "custom exaggeration (vertical scale = length scale / exaggeration)"));
            ui.label(RichText::new(trf!("výšky 1:{}", "heights 1:{}", cz(meritko / prevyseni.max(0.1), 0))).color(SEDA));
            // ulož jen hotovou změnu (ne každý krok tažení hodnoty)
            let zmena = (meritko - p.def.meritko).abs() > 1e-9 || (prevyseni - p.def.prevyseni).abs() > 1e-9;
            if zmena && !ui.ctx().input(|i| i.pointer.any_down()) {
                let mut nova = p.def.clone();
                nova.meritko = meritko;
                nova.prevyseni = prevyseni;
                spravce::posli(Akce::PohledUpravit(p.insert, Box::new(nova)));
            }
        });
        // povrch „upravený terén“ (pásy a čára upraveného terénu)
        if stav.povrchy.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(tr("Upravený terén", "Modified ground"));
                let akt = p.def.vyjimky.get("povrch_upraveny").cloned().unwrap_or_default();
                let jmeno = |id: &str| stav.povrchy.iter().find(|x| x.id == id).map_or_else(|| tr("automaticky", "automatic").to_string(), |x| x.jmeno.clone());
                let mut vybrany = akt.clone();
                egui::ComboBox::from_id_salt(("upraveny_teren", p.id.as_str()))
                    .selected_text(jmeno(&akt))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut vybrany, String::new(), tr("automaticky", "automatic"));
                        for x in stav.povrchy.iter().filter(|x| x.id != p.def.povrch) {
                            ui.selectable_value(&mut vybrany, x.id.clone(), &x.jmeno);
                        }
                    })
                    .response
                    .on_hover_text(tr("povrch pro pásy a čáru „upravený terén“; automaticky = první další povrch (přednostně se jménem upravený / návrh)", "surface for the “modified ground” bands and line; automatic = the first other surface (preferably one named modified / design)"));
                if vybrany != akt {
                    let mut nova = p.def.clone();
                    if vybrany.is_empty() {
                        nova.vyjimky.remove("povrch_upraveny");
                    } else {
                        nova.vyjimky.insert("povrch_upraveny".into(), vybrany);
                    }
                    spravce::posli(Akce::PohledUpravit(p.insert, Box::new(nova)));
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            karta::kpi(ui, &nazev_predlohy(&d, styly), tr("vzhled", "appearance"));
            karta::kpi(ui, &czi(d.pasy.iter().filter(|x| !x.skryty).count()), tr("pásů", "bands"));
        });
        if !p.def.vyjimky.is_empty() {
            ui.label(RichText::new(trf!("výjimky tohoto pohledu: {}", "overrides in this view: {}", p.def.vyjimky.len())).color(ORANZOVA).size(11.0));
        }
        if !p.def.pasy_ze_stylu {
            ui.label(RichText::new(tr("vlastní pásy tohoto pohledu", "bands specific to this view")).color(ORANZOVA).size(11.0));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(ik!("tra_vlastnosti")), tr("Upravit vzhled…", "Edit appearance…"), true)
                .on_hover_text(tr("otevře editor vzhledu s živým náhledem – jen tento pohled", "opens the appearance editor with a live preview – this view only"))
                .clicked()
            {
                self.editor_profilu = super::editor_profilu::EditorProfilu::pro_pohled(stav, &p.id);
            }
            if karta::tlacitko(ui, None, tr("Upravit styl…", "Edit style…"), false).on_hover_text(tr("otevře editor stylu – změna platí pro všechny pohledy se stylem", "opens the style editor – a change applies to all views with the style")).clicked() {
                self.editor_profilu = super::editor_profilu::EditorProfilu::pro_styl(stav, &d.styl);
            }
            if karta::tlacitko(ui, None, tr("Uložit jako nový styl…", "Save as new style…"), false)
                .on_hover_text(tr("nastavení a pásy tohoto pohledu jako nový styl profilu ve výkrese", "this view’s settings and bands as a new profile style in the drawing"))
                .clicked()
            {
                let s = styl_z_definice(&d, styly, "", &trf!("Styl z pohledu {}", "Style from view {}", p.id));
                spravce::posli(Akce::UlozStylProfilu(Box::new(s)));
            }
        });
    }

    /// Přehled stylů profilů.
    pub(super) fn detail_styly_profilu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let pohledy: Vec<&PrehledPohledu> = stav.trasy.iter().flat_map(|t| t.pohledy.iter()).collect();
        karta::nadpis_karty(ui, &ikona_pohledu(), tr("Styly profilů", "Profile styles"), |ui| {
            karta::pilulka(ui, &czi(stav.styly_profilu.len()), SEDA);
        });
        ui.label(RichText::new(tr("Vzory pro pohledy podélných profilů: vzhled a sada pásů. Změna stylu překreslí všechny pohledy, které ho používají.", "Templates for profile views: appearance and band set. Changing a style redraws every view that uses it.")).color(SEDA));
        ui.add_space(6.0);
        if karta::tlacitko(ui, None, tr("+ Nový styl profilu", "+ New profile style"), true).clicked() {
            let mut s = stav.styly_profilu.iter().find(|s| s.id == "JEDNODUCHY").cloned().unwrap_or_else(|| StylProfilu::vestavene()[0].clone());
            s.id.clear();
            s.jmeno = tr("Nový styl profilu", "New profile style").to_string();
            spravce::posli(Akce::UlozStylProfilu(Box::new(s)));
        }
        ui.add_space(8.0);
        let mut klik = None;
        for s in &stav.styly_profilu {
            let n = pohledy.iter().filter(|p| p.def.styl == s.id).count();
            let mut popis = trf!("{} pásů · 1:{} · převýšení {}× · pohledů {}", "{} bands · 1:{} · exaggeration {}× · views {}", s.pasy.len(), cz(s.meritko, 0), cz(s.prevyseni, 1), n);
            if stav.ulozene_styly.contains(&s.id) {
                popis.push_str(if s.je_vestaveny() { tr(" · upravený", " · modified") } else { tr(" · vlastní", " · custom") });
            }
            let r = karta::radek_seznamu(ui, &ikona_pohledu(), &crate::lang::tr_dyn(&s.jmeno), &popis, "", SEDA);
            if r.clicked() {
                klik = Some(s.id.clone());
            }
        }
        if let Some(id) = klik {
            self.vyber_uzel(Uzel::StylProfilu(id));
        }
    }

    /// Karta stylu profilu: přehled, editor, duplikovat, smazat.
    pub(super) fn karta_stylu_profilu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        let Some(s) = stav.styly_profilu.iter().find(|s| s.id == id).cloned() else {
            ui.label(RichText::new(tr("Styl profilu už ve výkrese není.", "The profile style is no longer in the drawing.")).color(SEDA));
            return;
        };
        let pouziva: Vec<String> = stav.trasy.iter().flat_map(|t| t.pohledy.iter().filter(|p| p.def.styl == id).map(move |p| format!("{} ({})", p.id, t.jmeno))).collect();
        let ulozeny = stav.ulozene_styly.contains(&s.id);
        karta::nadpis_karty(ui, &ikona_pohledu(), &crate::lang::tr_dyn(&s.jmeno), |ui| {
            if s.je_vestaveny() {
                karta::pilulka(ui, if ulozeny { tr("vestavěný · upravený", "built-in · modified") } else { tr("vestavěný", "built-in") }, SEDA);
            } else {
                karta::pilulka(ui, tr("vlastní", "custom"), MODRA);
            }
        });
        ui.label(RichText::new(if pouziva.is_empty() { tr("Žádný pohled tento styl nepoužívá.", "No view uses this style.").to_string() } else { trf!("Používají ho pohledy: {}", "Used by views: {}", pouziva.join(", ")) }).color(SEDA));
        ui.add_space(6.0);
        let mut d = DefinicePohledu::novy("", "", "");
        d.styl = String::new();
        d.vyjimky = s.hodnoty.clone();
        ui.horizontal_wrapped(|ui| {
            karta::kpi(ui, &nazev_predlohy(&d, &[]), tr("vzhled", "appearance"));
            karta::kpi(ui, &czi(s.pasy.iter().filter(|x| !x.skryty).count()), tr("pásů", "bands"));
            karta::kpi(ui, &format!("1:{}", cz(s.meritko, 0)), tr("výchozí měřítko", "default scale"));
            karta::kpi(ui, &format!("{}×", cz(s.prevyseni, 1)), tr("převýšení", "vertical exaggeration"));
        });
        ui.add_space(4.0);
        for p in s.pasy.iter().filter(|x| !x.skryty) {
            let nazev = if p.nazev.trim().is_empty() { crate::lang::tr_tab(p.druh.nazev()).to_string() } else { p.nazev.clone() };
            ui.label(RichText::new(format!("{} · {}", if p.poloha == trasy::pasy::Poloha::Nad { tr("nad", "above") } else { tr("pod", "below") }, nazev)).color(SEDA).size(11.5));
        }
        ui.add_space(8.0);
        let (mut duplikovat, mut smazat) = (false, false);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(ik!("tra_vlastnosti")), tr("Upravit vzhled…", "Edit appearance…"), true).on_hover_text(tr("otevře editor stylu s živým náhledem", "opens the style editor with a live preview")).clicked() {
                self.editor_profilu = super::editor_profilu::EditorProfilu::pro_styl(stav, id);
            }
            if karta::tlacitko(ui, None, tr("Duplikovat", "Duplicate"), false).clicked() {
                duplikovat = true;
            }
            let (popis, smi) = if s.je_vestaveny() { (tr("Obnovit výchozí", "Restore default"), ulozeny) } else { (tr("Smazat", "Delete"), true) };
            if smi && karta::tlacitko(ui, None, popis, false).clicked() {
                smazat = true;
            }
        });
        if duplikovat {
            let mut n = s.clone();
            n.id.clear();
            n.jmeno = trf!("{} – kopie", "{} – copy", crate::lang::tr_dyn(&s.jmeno));
            spravce::posli(Akce::UlozStylProfilu(Box::new(n)));
        }
        if smazat {
            spravce::posli(Akce::SmazStylProfilu(s.id.clone()));
        }
    }
}

// ── Styly tras ──────────────────────────────────────────────────────────────

use trasy::styl::Styl;
use trasy::trasa::SCHEMA as SCHEMA_TRASY;

/// Hodnota stylu trasy (chybí-li, výchozí ze schématu).
fn hodnota_trasy(s: &Styl, k: &str) -> String {
    s.hodnoty.get(k).cloned().unwrap_or_else(|| SCHEMA_TRASY.iter().find(|x| x.klic == k).map_or(String::new(), |x| x.vychozi.to_string()))
}

/// Nastaví hodnotu stylu trasy (výchozí hodnota schématu se neukládá).
fn nastav_trasy(s: &mut Styl, k: &str, v: String) {
    if SCHEMA_TRASY.iter().any(|x| x.klic == k && x.vychozi == v) {
        s.hodnoty.remove(k);
    } else {
        s.hodnoty.insert(k.to_string(), v);
    }
}

fn cislo_trasy(s: &Styl, k: &str) -> f64 {
    hodnota_trasy(s, k).replace(',', ".").trim().parse().unwrap_or(0.0)
}

fn text_cisla(x: f64) -> String {
    if (x - x.round()).abs() < 1e-9 { format!("{}", x.round() as i64) } else { format!("{x}") }
}

fn cislo_pole(ui: &mut egui::Ui, s: &mut Styl, k: &str, rozsah: std::ops::RangeInclusive<f64>, rychlost: f64, pripona: &str) {
    let mut x = cislo_trasy(s, k);
    if ui.add(egui::DragValue::new(&mut x).range(rozsah).speed(rychlost).max_decimals(if k.starts_with("desetin") { 0 } else { 2 }).suffix(pripona)).changed() {
        nastav_trasy(s, k, text_cisla(x));
    }
}

fn zaskrt_trasy(ui: &mut egui::Ui, s: &mut Styl, k: &str, popis: &'static str) {
    let mut b = matches!(hodnota_trasy(s, k).trim(), "1" | "ano" | "true");
    if ui.checkbox(&mut b, crate::lang::tr_tab(popis)).changed() {
        nastav_trasy(s, k, if b { "1" } else { "0" }.to_string());
    }
}

/// Styl z výsledných hodnot trasy (styl + výjimky).
fn styl_z_trasy(t: &PrehledTrasy, styly: &[Styl]) -> Styl {
    let h = trasy::styl::vysledne(SCHEMA_TRASY, styly, &t.styl_id, &t.vyjimky);
    Styl {
        id: String::new(),
        nazev: trf!("Styl z trasy {}", "Style from alignment {}", t.jmeno),
        rodic: None,
        hodnoty: SCHEMA_TRASY.iter().filter(|k| h.text(k.klic) != k.vychozi).map(|k| (k.klic.to_string(), h.text(k.klic).to_string())).collect(),
    }
}

impl Okno {
    /// Přehled stylů tras.
    pub(super) fn detail_styly_tras(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        karta::nadpis_karty(ui, &ikona_trasy(), tr("Styly tras", "Alignment styles"), |ui| {
            karta::pilulka(ui, &czi(stav.styly_tras.len()), SEDA);
        });
        ui.label(RichText::new(tr("Vzory vzhledu tras v situaci: staničení, popisy, barvy a tloušťky. Změna stylu překreslí všechny trasy, které ho používají.", "Templates for how alignments look in plan: stations, labels, colours and lineweights. Changing a style redraws every alignment that uses it.")).color(SEDA));
        ui.add_space(6.0);
        if karta::tlacitko(ui, None, tr("+ Nový styl trasy", "+ New alignment style"), true).clicked() {
            let mut s = stav.styly_tras.first().cloned().unwrap_or_default();
            s.id.clear();
            s.nazev = tr("Nový styl trasy", "New alignment style").to_string();
            spravce::posli(Akce::UlozStylTrasy(Box::new(s)));
        }
        ui.add_space(8.0);
        let mut klik = None;
        for s in &stav.styly_tras {
            let n = stav.trasy.iter().filter(|t| t.styl_id == s.id).count();
            let vedlejsi = cislo_trasy(s, "krok_vedlejsi");
            let hlavni = cislo_trasy(s, "krok_hlavni");
            let mut popis = trf!("1:{} · staničení {} / {} m · tras {}", "1:{} · stations {} / {} m · alignments {}", text_cisla(cislo_trasy(s, "meritko")), text_cisla(vedlejsi), text_cisla(hlavni), n);
            if stav.ulozene_styly.contains(&s.id) {
                popis.push_str(if trasy::trasa::je_vestaveny(&s.id) { tr(" · upravený", " · modified") } else { tr(" · vlastní", " · custom") });
            }
            if karta::radek_seznamu(ui, &ikona_trasy(), &crate::lang::tr_dyn(&s.nazev), &popis, "", SEDA).clicked() {
                klik = Some(s.id.clone());
            }
        }
        if let Some(id) = klik {
            self.vyber_uzel(Uzel::StylTrasy(id));
        }
    }

    /// Karta stylu trasy: jméno, staničení, popisy, barvy a tloušťky.
    pub(super) fn karta_stylu_trasy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        let Some(puvodni) = stav.styly_tras.iter().find(|s| s.id == id).cloned() else {
            ui.label(RichText::new(tr("Styl trasy už ve výkrese není.", "The alignment style is no longer in the drawing.")).color(SEDA));
            return;
        };
        if self.uprava_stylu_trasy.as_ref().is_none_or(|s| s.id != id) {
            self.uprava_stylu_trasy = Some(puvodni.clone());
        }
        let vestaveny = trasy::trasa::je_vestaveny(&puvodni.id);
        let ulozeny = stav.ulozene_styly.contains(&puvodni.id);
        let pouziva: Vec<String> = stav.trasy.iter().filter(|t| t.styl_id == id).map(|t| t.jmeno.clone()).collect();
        let (mut ulozit, mut vratit, mut duplikovat, mut smazat) = (false, false, false, false);
        let Some(s) = self.uprava_stylu_trasy.as_mut() else { return };
        karta::nadpis_karty(ui, &ikona_trasy(), &crate::lang::tr_dyn(&puvodni.nazev), |ui| {
            if vestaveny {
                karta::pilulka(ui, if ulozeny { tr("vestavěný · upravený", "built-in · modified") } else { tr("vestavěný", "built-in") }, SEDA);
            } else {
                karta::pilulka(ui, tr("vlastní", "custom"), MODRA);
            }
        });
        ui.label(RichText::new(if pouziva.is_empty() { tr("Žádná trasa tento styl nepoužívá.", "No alignment uses this style.").to_string() } else { trf!("Používají ho trasy: {}", "Used by alignments: {}", pouziva.join(", ")) }).color(SEDA));
        ui.add_space(6.0);

        let mut jmeno = crate::lang::tr_dyn(&s.nazev);
        egui::Grid::new(("styl_trasy_obecne", id)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Jméno", "Name"));
            if ui.add(egui::TextEdit::singleline(&mut jmeno).desired_width(260.0)).changed() {
                s.nazev = jmeno.clone();
            }
            ui.end_row();
            ui.label(tr("Měřítko výkresu 1:", "Drawing scale 1:"));
            cislo_pole(ui, s, "meritko", 10.0..=100_000.0, 10.0, "");
            ui.end_row();
            ui.label(tr("Výška textu", "Text height"));
            cislo_pole(ui, s, "vyska_textu", 0.5..=20.0, 0.05, " mm");
            ui.end_row();
        });

        ui.add_space(8.0);
        ui.label(RichText::new(tr("Staničení", "Station")).strong());
        egui::Grid::new(("styl_trasy_stan", id)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Krok hlavního staničení", "Major station interval"));
            cislo_pole(ui, s, "krok_hlavni", 0.0..=100_000.0, 1.0, " m");
            ui.end_row();
            ui.label(tr("Krok vedlejšího staničení", "Minor station interval"));
            ui.horizontal(|ui| {
                cislo_pole(ui, s, "krok_vedlejsi", 0.0..=100_000.0, 1.0, " m");
                ui.label(RichText::new(tr("0 = nekreslit", "0 = do not draw")).color(SEDA).size(11.0));
            });
            ui.end_row();
            ui.label(tr("Formát", "Format"));
            let akt = trasy::stanice::FormatStanice::z_kodu(&hodnota_trasy(s, "format"));
            egui::ComboBox::from_id_salt(("format_trasy", id)).selected_text(akt.ukazka()).show_ui(ui, |ui| {
                for f in trasy::stanice::FormatStanice::VSE {
                    if ui.selectable_label(f == akt, f.ukazka()).clicked() {
                        nastav_trasy(s, "format", f.kod().to_string());
                    }
                }
            });
            ui.end_row();
            ui.label(tr("Desetinná místa staničení", "Station decimals"));
            cislo_pole(ui, s, "desetin_hlavni", 0.0..=4.0, 0.05, "");
            ui.end_row();
            ui.label(tr("Desetinná místa u hlavních bodů", "Decimals at key points"));
            cislo_pole(ui, s, "desetin_body", 0.0..=4.0, 0.05, "");
            ui.end_row();
        });
        zaskrt_trasy(ui, s, "carka", "Desetinná čárka");

        ui.add_space(8.0);
        ui.label(RichText::new(tr("Popisy", "Labels")).strong());
        zaskrt_trasy(ui, s, "popis_body", "Popisovat hlavní body (ZÚ, KÚ, TK, KT, TP, PK, KP, PT)");
        zaskrt_trasy(ui, s, "popis_vrcholy", "Popisovat vrcholy (V) a poloměry");
        ui.horizontal(|ui| {
            ui.label(tr("Zkratky bodů", "Point abbreviations"));
            let mut z = hodnota_trasy(s, "zkratky");
            if ui.add(egui::TextEdit::singleline(&mut z).desired_width(180.0)).on_hover_text(tr("ZÚ,KÚ,TK,KT,V,TP,PK,KP,PT oddělené čárkou", "ZÚ,KÚ,TK,KT,V,TP,PK,KP,PT separated by commas")).changed() { // i18n:ne // i18n:ne
                nastav_trasy(s, "zkratky", z);
            }
        });
        zaskrt_trasy(ui, s, "polygon", "Kreslit tečnový polygon");
        zaskrt_trasy(ui, s, "polomery", "Rozkreslit oblouky (poloměry ke středu)");
        zaskrt_trasy(ui, s, "netisk", "Netisknout (hladina, která se netiskne)");
        ui.horizontal(|ui| {
            ui.label(tr("Čára osy", "Centreline"));
            let akt = s.hodnoty.get("typ_osa").cloned().unwrap_or_else(|| "plna".into());
            for (k, popis) in [("plna", tr("plná", "solid")), ("cerchovana", tr("čerchovaná", "dash-dot"))] {
                if ui.selectable_label(akt == k, popis).clicked() && akt != k {
                    nastav_trasy(s, "typ_osa", k.to_string());
                }
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new(tr("Barvy a tloušťky", "Colours and lineweights")).strong());
        egui::Grid::new(("styl_trasy_barvy", id)).num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            for (cast, popis) in [("osa", "Osa"), ("staniceni", "Staničení"), ("body", "Hlavní body"), ("polygon", "Tečnový polygon")] {
                ui.label(crate::lang::tr_tab(popis));
                let kb = format!("barva_{cast}");
                let kt = format!("tloustka_{cast}");
                let mut b = cislo_trasy(s, &kb) as i16;
                if super::vyber_barvy(ui, &format!("{id}_{kb}"), &mut b) {
                    nastav_trasy(s, &kb, b.to_string());
                }
                let mut t = cislo_trasy(s, &kt) as i16;
                if super::vyber_tloustky(ui, &format!("{id}_{kt}"), &mut t) {
                    nastav_trasy(s, &kt, t.to_string());
                }
                ui.end_row();
            }
            // oblouky osy: 0 = stejně jako osa
            ui.label(tr("Oblouky osy", "Centreline arcs"));
            let mut b = cislo_trasy(s, "barva_oblouk") as i16;
            let mut jinak = b > 0;
            ui.horizontal(|ui| {
                if ui.checkbox(&mut jinak, tr("jinou barvou", "different colour")).changed() {
                    nastav_trasy(s, "barva_oblouk", if jinak { "5".into() } else { "0".into() });
                }
                if jinak && super::vyber_barvy(ui, &format!("{id}_barva_oblouk"), &mut b) {
                    nastav_trasy(s, "barva_oblouk", b.to_string());
                }
            });
            ui.end_row();
        });

        ui.add_space(8.0);
        let zmeneno = *s != puvodni;
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(zmeneno, egui::Button::new(tr("Uložit styl", "Save style"))).on_hover_text(tr("uloží styl do výkresu a překreslí trasy, které ho používají", "saves the style to the drawing and redraws the alignments that use it")).clicked() {
                ulozit = true;
            }
            if zmeneno && ui.button(tr("Vrátit změny", "Revert changes")).clicked() {
                vratit = true;
            }
            if ui.button(tr("Duplikovat", "Duplicate")).clicked() {
                duplikovat = true;
            }
            let (popis, smi) = if vestaveny { (tr("Obnovit výchozí", "Restore default"), ulozeny) } else { (tr("Smazat", "Delete"), true) };
            if ui.add_enabled(smi, egui::Button::new(popis)).clicked() {
                smazat = true;
            }
        });
        let s = s.clone();
        if ulozit {
            spravce::posli(Akce::UlozStylTrasy(Box::new(s.clone())));
        }
        if duplikovat {
            let mut n = s.clone();
            n.id.clear();
            n.nazev = trf!("{} – kopie", "{} – copy", crate::lang::tr_dyn(&s.nazev));
            spravce::posli(Akce::UlozStylTrasy(Box::new(n)));
        }
        if smazat {
            spravce::posli(Akce::SmazStylTrasy(puvodni.id.clone()));
            self.uprava_stylu_trasy = None;
        }
        if vratit {
            self.uprava_stylu_trasy = Some(puvodni);
        }
    }
}
