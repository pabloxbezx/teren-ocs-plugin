//! Karta detailu: nadpis s ikonou a stavem, hlavní čísla, nejčastější akce
//! a záložky. Používá ji detail povrchu, skupiny a bodů.

use teren::preklad::tr;
use teren::trf;

use eframe::egui::{self, Color32, RichText, Sense};

use super::strom::{ik, kresli_ikonu, Ikona};
use super::{
    detail_upozorneni, detail_upravy, smazat, Cast, Okno, Rezim, Uzel, CERVENA, MODRA, ORANZOVA, SEDA,
    ZELENA,
};
use crate::povrchy::{cislo_cz, DruhPolozky};
use crate::spravce::{self, Akce, PrehledPovrchu, StavSpravce, Zmena};
use crate::styly;

/// Záložka na kartě povrchu (zdroje a upozornění mají vlastní uzel stromu).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ZalozkaPovrchu {
    #[default]
    Prehled,
    Export,
}

/// Barevný štítek (stav, aktivní…).
/// Text začínající „● “ dostane místo znaku nakreslenou tečku (písmo egui ● nemá).
pub(super) fn pilulka(ui: &mut egui::Ui, text: &str, barva: Color32) -> egui::Response {
    let (tecka, text) = match text.strip_prefix("● ") {
        Some(t) => (true, t),
        None => (false, text),
    };
    egui::Frame::new()
        .fill(barva.gamma_multiply(0.16))
        .stroke(egui::Stroke::new(1.0, barva.gamma_multiply(0.55)))
        .corner_radius(9.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                if tecka {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, barva);
                }
                ui.label(RichText::new(text).color(barva).size(12.0));
            });
        })
        .response
}

/// Hlavní číslo s popiskem pod ním.
pub(super) fn kpi(ui: &mut egui::Ui, hodnota: &str, popis: &str) -> egui::Response {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_min_width(80.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(hodnota).strong().size(15.0));
                ui.label(RichText::new(popis).small().color(SEDA));
            });
        })
        .response
}

/// Dlaždice zdroje povrchu: ikona, počet, název; klik otevře seznam.
fn dlazdice(ui: &mut egui::Ui, ikona: &Ikona, pocet: usize, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(158.0, 44.0), Sense::click());
    let v = ui.visuals();
    let stroke = if resp.hovered() { v.widgets.hovered.bg_stroke } else { v.widgets.noninteractive.bg_stroke };
    ui.painter().rect(rect, 7.0, v.extreme_bg_color, stroke, egui::StrokeKind::Inside);
    let nula = pocet == 0;
    kresli_ikonu(ui, ikona, egui::pos2(rect.left() + 18.0, rect.center().y), nula);
    let barva = if nula { SEDA } else { v.strong_text_color() };
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.top() + 13.0),
        egui::Align2::LEFT_CENTER,
        pocet.to_string(),
        egui::FontId::proportional(16.0),
        barva,
    );
    ui.painter().text(egui::pos2(rect.left() + 34.0, rect.top() + 31.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(12.0), SEDA);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Řádek seznamu (skupiny, povrchy, podezřelé body): ikona, název, šedý popis
/// pod ním a údaj vpravo; celý řádek je klikací.
pub(super) fn radek_seznamu(ui: &mut egui::Ui, ikona: &Ikona, nazev: &str, popis: &str, vpravo: &str, vpravo_barva: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), 40.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let v = ui.visuals().clone();
    let fill = if resp.hovered() { v.widgets.hovered.weak_bg_fill } else { v.extreme_bg_color };
    ui.painter().rect(rect, 6.0, fill, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    kresli_ikonu(ui, ikona, egui::pos2(rect.left() + 18.0, rect.center().y), false);
    let x = rect.left() + 36.0;
    let p = ui.painter();
    let gv = p.layout_no_wrap(vpravo.to_string(), egui::FontId::proportional(12.5), vpravo_barva);
    let xr = rect.right() - 12.0 - gv.size().x;
    p.galley(egui::pos2(xr, rect.center().y - gv.size().y / 2.0), gv, vpravo_barva);
    let sirka = (xr - x - 10.0).max(20.0);
    let g = egui::WidgetText::from(RichText::new(nazev).strong()).into_galley(ui, Some(egui::TextWrapMode::Truncate), sirka, egui::TextStyle::Body);
    ui.painter().galley(egui::pos2(x, rect.top() + 4.0), g, v.strong_text_color());
    let g = egui::WidgetText::from(RichText::new(popis).size(11.5)).into_galley(ui, Some(egui::TextWrapMode::Truncate), sirka, egui::TextStyle::Small);
    ui.painter().galley(egui::pos2(x, rect.top() + 22.0), g, SEDA);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Řádek záložek; vrací kliknutou.
pub(super) fn zalozky<T: Copy + PartialEq>(ui: &mut egui::Ui, aktivni: T, polozky: &[(T, String)]) -> Option<T> {
    let mut vysledek = None;
    ui.horizontal(|ui| {
        for (z, t) in polozky {
            let on = *z == aktivni;
            let text = if on { RichText::new(t).strong() } else { RichText::new(t).color(SEDA) };
            let r = ui.add(egui::Button::new(text).frame(false));
            if on {
                let rr = r.rect;
                ui.painter().hline(rr.x_range(), rr.bottom() + 3.0, egui::Stroke::new(2.0, MODRA));
            }
            if r.clicked() {
                vysledek = Some(*z);
            }
            ui.add_space(6.0);
        }
    });
    ui.add_space(4.0);
    ui.separator();
    vysledek
}

/// Nadpis karty: ikona, název (případně pole pro přejmenování) a štítky.
pub(super) fn nadpis_karty(ui: &mut egui::Ui, ikona: &Ikona, nazev: &str, stitky: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::hover());
        let img = rect;
        match ikona {
            Ikona::Svg(src) => {
                egui::Image::new(src.clone()).paint_at(ui, img);
            }
            i => kresli_ikonu(ui, i, img.center(), false),
        }
        let r = ui.label(RichText::new(nazev).size(18.0).strong());
        stitky(ui);
        r
    })
    .inner
}

pub(super) fn pocet_casti(p: &PrehledPovrchu, c: Cast) -> usize {
    let pocet = |f: &dyn Fn(&DruhPolozky) -> bool| p.polozky.iter().filter(|x| f(&x.druh)).count();
    match c {
        Cast::Skupiny => p.skupiny.len(),
        Cast::Hranice => pocet(&|d| matches!(d, DruhPolozky::Hranice | DruhPolozky::Dira)),
        Cast::Spojnice => pocet(&|d| matches!(d, DruhPolozky::Spojnice)),
        Cast::Objekty => pocet(&|d| matches!(d, DruhPolozky::Objekt(_))),
        Cast::Upravy => p.upravy.len(),
    }
}

pub(super) const CASTI: [Cast; 5] = [Cast::Skupiny, Cast::Hranice, Cast::Spojnice, Cast::Objekty, Cast::Upravy];

/// Které „záložky“ karty povrchu jsou otevřené.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Prehled,
    Zdroje,
    Export,
    Upozorneni,
}

impl Okno {
    /// Karta povrchu se záložkami Přehled · Zdroje · Export · Upozornění.
    pub(super) fn karta_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        let skryty = p.styl.eq_ignore_ascii_case(styly::SKRYT);
        let ikona = if skryty { Ikona::Svg(ik!("ter_sp_skryt")) } else { Ikona::Svg(ik!("ter_vrstevnice")) };

        // ── nadpis ──
        if self.prejmenovat {
            ui.horizontal(|ui| {
                ui.label(tr("Nové jméno"));
                let r = ui.add(egui::TextEdit::singleline(&mut self.jmeno).desired_width(240.0));
                if ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || ui.button(tr("Přejmenovat")).clicked() {
                    if self.jmeno.trim() != p.jmeno && !self.jmeno.trim().is_empty() {
                        spravce::posli(Akce::Nastav(vec![id.clone()], Zmena::Jmeno(self.jmeno.trim().to_string())));
                    }
                    self.prejmenovat = false;
                }
                if ui.button(tr("Zrušit")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.jmeno = p.jmeno.clone();
                    self.prejmenovat = false;
                }
            });
        } else {
            nadpis_karty(ui, &ikona, &p.jmeno, |ui| {
                if ui.small_button("✏").on_hover_text(tr("Přejmenovat")).clicked() {
                    self.jmeno = p.jmeno.clone();
                    self.prejmenovat = true;
                }
                if p.zastaraly {
                    pilulka(ui, tr("● zastaralý"), ORANZOVA).on_hover_text(tr("zdroje se od přepočtu změnily – klikněte Přepočítat"));
                } else {
                    pilulka(ui, tr("● aktuální"), ZELENA);
                }
                if p.aktivni {
                    pilulka(ui, tr("★ aktivní"), MODRA).on_hover_text(tr("s aktivním povrchem pracují nástroje Úpravy a Nový bod na povrchu"));
                }
                if skryty {
                    pilulka(ui, tr("skrytý"), SEDA).on_hover_text(tr("styl Nezobrazovat – povrch se počítá, ale nekreslí"));
                }
            });
        }

        // ── hlavní čísla ──
        let s = &p.stat;
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            kpi(ui, &cislo_cz(p.pocet_bodu as f64), tr("bodů"));
            kpi(ui, &cislo_cz(s.trojuhelniku as f64), tr("trojúhelníků"));
            kpi(ui, &format!("{:.2} – {:.2} m", s.z_min, s.z_max), tr("výška"));
            kpi(ui, &format!("{} m²", cislo_cz(s.plocha_2d.round())), tr("plocha 2D")).on_hover_text(trf!("plocha 3D {:.1} m²", s.plocha_3d));
        });

        // ── akce ──
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let t = RichText::new(tr("⟳ Přepočítat"));
            let b = if p.zastaraly { egui::Button::new(t.strong().color(Color32::WHITE)).fill(Color32::from_rgb(31, 111, 235)) } else { egui::Button::new(t) };
            if ui.add(b).clicked() {
                spravce::posli(Akce::Prepocitat(vec![id.clone()]));
            }
            if !p.aktivni && ui.button(tr("★ Nastavit jako aktivní")).clicked() {
                spravce::posli(Akce::Aktivovat(id.clone()));
            }
            if ui.button(tr("Prohlédnout ve 3D")).clicked() {
                self.pohled.otevri(vec![id.clone()]);
                self.rezim = Rezim::Pohled3D;
            }
            let mut auto = p.auto;
            if ui
                .checkbox(&mut auto, tr("přepočítávat automaticky"))
                .on_hover_text(tr("po každé změně bodů, skupin nebo spojnic se povrch přepočítá sám"))
                .changed()
            {
                spravce::posli(Akce::Nastav(vec![id.clone()], Zmena::Auto(auto)));
            }
            ui.menu_button(tr("Další ⏷"), |ui| self.menu_povrchu(ui, stav, p)).response.on_hover_text(tr("Další akce: styl, vrstevnice, přejmenovat, export, smazat"));
        });
        ui.add_space(6.0);

        // ── záložky ──
        let tab = match &self.uzel {
            Uzel::Definice(..) => Tab::Zdroje,
            Uzel::Upozorneni(_) => Tab::Upozorneni,
            _ if self.zalozka_povrchu == ZalozkaPovrchu::Export => Tab::Export,
            _ => Tab::Prehled,
        };
        let celkem: usize = CASTI.iter().map(|c| pocet_casti(p, *c)).sum();
        let mut polozky = vec![(Tab::Prehled, tr("Přehled").to_string()), (Tab::Zdroje, trf!("Zdroje ({})", celkem)), (Tab::Export, tr("Export").to_string())];
        if p.upozorneni() > 0 {
            polozky.push((Tab::Upozorneni, trf!("⚠ Upozornění ({})", p.upozorneni())));
        }
        if let Some(t) = zalozky(ui, tab, &polozky) {
            match t {
                Tab::Prehled | Tab::Export => {
                    self.vyber_uzel(Uzel::Povrch(id.clone()));
                    self.zalozka_povrchu = if t == Tab::Export { ZalozkaPovrchu::Export } else { ZalozkaPovrchu::Prehled };
                }
                Tab::Zdroje => {
                    let c = CASTI.iter().copied().find(|c| pocet_casti(p, *c) > 0).unwrap_or(Cast::Skupiny);
                    self.vyber_uzel(Uzel::Definice(id.clone(), c));
                }
                Tab::Upozorneni => self.vyber_uzel(Uzel::Upozorneni(id.clone())),
            }
        }

        egui::ScrollArea::vertical().id_salt(("karta_povrchu", tab as u8)).auto_shrink([false, false]).show(ui, |ui| match tab {
            Tab::Prehled => self.prehled_povrchu(ui, stav, p),
            Tab::Export => self.export_povrchu(ui, stav, p),
            Tab::Upozorneni => detail_upozorneni(ui, p),
            Tab::Zdroje => {
                let akt = if let Uzel::Definice(_, c) = &self.uzel { *c } else { Cast::Skupiny };
                ui.horizontal_wrapped(|ui| {
                    for c in CASTI {
                        let n = pocet_casti(p, c);
                        let t = format!("{} ({n})", c.nazev());
                        let t = if n == 0 && akt != c { RichText::new(t).color(SEDA) } else { RichText::new(t) };
                        if ui.selectable_label(akt == c, t).on_hover_text(super::strom::popis_casti(c)).clicked() {
                            self.vyber_uzel(Uzel::Definice(id.clone(), c));
                        }
                    }
                });
                ui.add_space(6.0);
                match akt {
                    Cast::Hranice => self.polozky(ui, p, &[DruhPolozky::Hranice, DruhPolozky::Dira]),
                    Cast::Spojnice => self.polozky(ui, p, &[DruhPolozky::Spojnice]),
                    Cast::Objekty => self.detail_objekty(ui, p),
                    Cast::Upravy => detail_upravy(ui, p),
                    Cast::Skupiny => self.detail_skupiny_povrchu(ui, stav, p),
                }
            }
        });
    }

    fn prehled_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        if p.upozorneni() > 0 {
            egui::Frame::new()
                .fill(ORANZOVA.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0, ORANZOVA.gamma_multiply(0.5)))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(trf!("⚠ {} upozornění z přepočtu", p.upozorneni())).color(ORANZOVA));
                        if ui.link(tr("Zobrazit")).clicked() {
                            self.vyber_uzel(Uzel::Upozorneni(id.clone()));
                        }
                    });
                });
            ui.add_space(6.0);
        }
        ui.label(RichText::new(tr("Z čeho se povrch počítá")).strong());
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            for c in CASTI {
                let r = dlazdice(ui, &super::strom::ikona_casti(c), pocet_casti(p, c), c.nazev());
                if r.on_hover_text(super::strom::popis_casti(c)).clicked() {
                    self.vyber_uzel(Uzel::Definice(id.clone(), c));
                }
            }
        });
        let skupiny: Vec<String> = p.skupiny.iter().filter(|g| g.zapnuto).map(|g| g.jmeno.clone()).collect();
        let mut t = if skupiny.is_empty() {
            tr("Povrch nebere body ze žádné skupiny.").to_string()
        } else {
            trf!("{} bodů ze skupin {} (vyloučeno podle kódu {})", p.pocet_bodu, skupiny.join(", "), p.vylouceno_bodu)
        };
        let z_objektu: usize = p.polozky.iter().filter(|x| x.zapnuto && matches!(x.druh, DruhPolozky::Objekt(_))).map(|x| x.vrcholu).sum();
        if z_objektu > 0 {
            t.push_str(&trf!(", {} z objektů výkresu", z_objektu));
        }
        ui.label(RichText::new(t).small().color(SEDA));
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Zobrazení a výpočet")).strong());
        ui.add_space(2.0);
        self.nastaveni_povrchu(ui, stav, p);
        ui.add_space(10.0);
        if ui.button(RichText::new(tr("Smazat povrch")).color(CERVENA)).clicked() {
            smazat(trf!("Povrch „{}“ je smazaný.", p.jmeno), Akce::Smazat(id.clone()));
        }
    }
}
