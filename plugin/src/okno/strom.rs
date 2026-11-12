//! Strom Prospektoru: položky s ikonami z pásu karet, počty a stavem vpravo.

use teren::preklad::tr;
use teren::trf;

use eframe::egui::{self, Color32, RichText, Sense};
use egui::collapsing_header::CollapsingState;

use teren::skupiny::VSECHNY;

use super::{
    bod_odpovida, ikona_stylu, ikona_stylu_povrchu, import_nahled, pocty_stylu, pruvodce, shoda, Cast, Okno, Uzel, CERVENA,
    MODRA, ORANZOVA, SEDA, ZELENA,
};
use crate::povrchy::DruhPolozky;
use crate::spravce::{self, Akce, PrehledPovrchu, StavSpravce};
use crate::styly;

/// SVG ikona z `plugin/assets` (stejné jako na pásu karet).
pub(super) fn svg(uri: &'static str, bytes: &'static [u8]) -> egui::ImageSource<'static> {
    egui::ImageSource::Bytes { uri: std::borrow::Cow::Borrowed(uri), bytes: egui::load::Bytes::Static(bytes) }
}

macro_rules! ik {
    ($n:literal) => {
        $crate::okno::strom::svg(concat!("bytes://", $n, ".svg"), include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $n, ".svg")))
    };
}
pub(crate) use ik;

/// Ikona položky stromu: obrázek z pásu karet, nebo znak.
pub(super) enum Ikona {
    Svg(egui::ImageSource<'static>),
    Znak(&'static str, Color32),
}

/// Údaj vpravo na řádku stromu.
pub(super) enum Vpravo {
    /// Počet (šedě).
    Pocet(String),
    /// Stavová tečka.
    Tecka(Color32),
    /// Aktivní povrch.
    Hvezda,
    /// Zvýrazněný počet (upozornění).
    Odznak(String, Color32),
}

/// Ikona velikosti 16 px vycentrovaná v `stred`.
pub(super) fn kresli_ikonu(ui: &egui::Ui, ikona: &Ikona, stred: egui::Pos2, ztlumit: bool) {
    match ikona {
        Ikona::Svg(src) => {
            let mut img = egui::Image::new(src.clone());
            if ztlumit {
                img = img.tint(Color32::from_white_alpha(110));
            }
            img.paint_at(ui, egui::Rect::from_center_size(stred, egui::vec2(16.0, 16.0)));
        }
        Ikona::Znak(t, barva) => {
            ui.painter().text(stred, egui::Align2::CENTER_CENTER, *t, egui::FontId::proportional(14.0), *barva);
        }
    }
}

/// Řádek stromu přes celou šířku: ikona, název (zkrácený) a údaje vpravo.
pub(super) fn polozka(ui: &mut egui::Ui, vybrano: bool, ikona: &Ikona, text: RichText, vpravo: &[Vpravo], ztlumit: bool) -> egui::Response {
    let vyska = 21.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(60.0), vyska), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let vis = ui.visuals().clone();
    let p = ui.painter();
    if vybrano {
        p.rect_filled(rect, 4.0, vis.selection.bg_fill);
        p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())), 1.0, MODRA);
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, vis.widgets.hovered.weak_bg_fill);
    }
    let cy = rect.center().y;
    kresli_ikonu(ui, ikona, egui::pos2(rect.left() + 11.0, cy), ztlumit);
    let x = rect.left() + 24.0;
    // údaje vpravo, zprava doleva
    let mut xr = rect.right() - 6.0;
    let maly = egui::FontId::proportional(11.5);
    for v in vpravo.iter().rev() {
        match v {
            Vpravo::Pocet(t) => {
                let g = p.layout_no_wrap(t.clone(), maly.clone(), SEDA);
                let w = g.size().x;
                p.galley(egui::pos2(xr - w, cy - g.size().y / 2.0), g, SEDA);
                xr -= w + 7.0;
            }
            Vpravo::Tecka(c) => {
                p.circle_filled(egui::pos2(xr - 4.0, cy), 3.8, *c);
                xr -= 14.0;
            }
            Vpravo::Hvezda => {
                p.text(egui::pos2(xr, cy), egui::Align2::RIGHT_CENTER, "★", egui::FontId::proportional(12.5), ORANZOVA);
                xr -= 17.0;
            }
            Vpravo::Odznak(t, c) => {
                let g = p.layout_no_wrap(t.clone(), maly.clone(), *c);
                let w = g.size().x + 10.0;
                let r = egui::Rect::from_min_size(egui::pos2(xr - w, cy - 8.0), egui::vec2(w, 16.0));
                p.rect_filled(r, 8.0, c.gamma_multiply(0.18));
                p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, c.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                p.galley(egui::pos2(r.left() + 5.0, cy - g.size().y / 2.0), g, *c);
                xr -= w + 6.0;
            }
        }
    }
    let barva = if ztlumit {
        SEDA
    } else if vybrano {
        vis.strong_text_color()
    } else {
        vis.text_color()
    };
    let g = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), (xr - x - 4.0).max(10.0), egui::TextStyle::Body);
    ui.painter().galley(egui::pos2(x, cy - g.size().y / 2.0), g, barva);
    resp
}

/// Položka bez rozbalování, odsazená na úroveň rozbalovacích položek.
pub(super) fn list(ui: &mut egui::Ui, vybrano: bool, ikona: &Ikona, text: RichText, vpravo: &[Vpravo], ztlumit: bool) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().indent);
        polozka(ui, vybrano, ikona, text, vpravo, ztlumit)
    })
    .inner
}

fn sekce(ui: &mut egui::Ui, t: &str) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(t.to_uppercase()).small().color(SEDA));
    });
    ui.add_space(1.0);
}

fn cislo(n: usize) -> String {
    crate::povrchy::cislo_cz(n as f64)
}

pub(super) fn ikona_casti(c: Cast) -> Ikona {
    Ikona::Svg(match c {
        Cast::Skupiny => ik!("ter_skupina_bodu"),
        Cast::Hranice => ik!("ter_hranice"),
        Cast::Spojnice => ik!("ter_spojnice_body"),
        Cast::Objekty => ik!("ter_objekty"),
        Cast::Upravy => ik!("ter_otocit_hranu"),
    })
}

pub(super) fn popis_casti(c: Cast) -> &'static str {
    match c {
        Cast::Skupiny => tr("body povrchu = zapnuté skupiny bez vyloučených kódů"),
        Cast::Hranice => tr("vnější hranice povrchu a díry v něm"),
        Cast::Spojnice => tr("lomové hrany – trojúhelníky je nepřekříží"),
        Cast::Objekty => tr("body z objektů výkresu (bloky, body, 3D plochy…)"),
        Cast::Upravy => tr("ručně otočené, přidané a smazané hrany a trojúhelníky"),
    }
}

impl Okno {
    pub(super) fn strom(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.add_space(4.0);
        let hledat = self.hledat.clone();
        let h = &hledat;
        let pocet_bodu = stav.body.iter().filter(|b| bod_odpovida(b, h)).count();
        let skupiny: Vec<_> = stav.skupiny.iter().filter(|s| shoda(&s.skupina.jmeno, h) || h.is_empty()).cloned().collect();
        let povrchy: Vec<_> = stav.povrchy.iter().filter(|p| shoda(&p.jmeno, h)).cloned().collect();

        // ── vybráno ve výkrese ──
        let nvyb = stav.vyber_body.len() + stav.vyber_povrchy.len();
        let vpravo = if nvyb > 0 {
            let t = if !stav.vyber_body.is_empty() {
                match stav.vyber_body.len() {
                    1 => tr("1 bod").to_string(),
                    n @ 2..=4 => trf!("{} body", n),
                    n => trf!("{} bodů", n),
                }
            } else {
                match stav.vyber_povrchy.len() {
                    1 => tr("1 povrch").to_string(),
                    n => trf!("{} povrchy", n),
                }
            };
            vec![Vpravo::Odznak(t, MODRA)]
        } else {
            vec![Vpravo::Pocet(tr("nic").into())]
        };
        if list(ui, self.uzel == Uzel::Vyber, &Ikona::Znak("◎", MODRA), RichText::new(tr("Vybráno ve výkrese")), &vpravo, false)
            .on_hover_text(tr("Vlastnosti bodu nebo povrchu, který je vybraný ve výkrese"))
            .clicked()
        {
            self.vyber_uzel(Uzel::Vyber);
        }

        // ── data ──
        sekce(ui, tr("Data"));
        let skryte = stav.styl_bodu.values().filter(|(id, _)| stav.styly_bodu.iter().any(|s| &s.id == id && s.skryt)).count();
        let t = if h.is_empty() { cislo(pocet_bodu) } else { trf!("{} nalezeno", cislo(pocet_bodu)) };
        let r = list(ui, self.uzel == Uzel::Body, &Ikona::Svg(ik!("ter_seznam")), RichText::new(tr("Body")), &[Vpravo::Pocet(t)], false);
        let r = if skryte > 0 { r.on_hover_text(trf!("{} bodů je skrytých (styl Nezobrazovat)", skryte)) } else { r };
        if r.clicked() {
            self.vyber_uzel(Uzel::Body);
        }
        r.context_menu(|ui| {
            if ui.button(tr("Importovat body ze souboru…")).clicked() {
                self.nahled_importu = Some(import_nahled::NahledImportu::novy(""));
            }
            ui.separator();
            if ui.button(tr("Zkontrolovat zaměření")).clicked() {
                spravce::posli(Akce::Kontrola);
                self.vyber_uzel(Uzel::Kontrola);
            }
            if ui.button(tr("Překreslit body")).clicked() {
                spravce::posli(Akce::ObnovitPopisy);
            }
        });

        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new("u_skupiny"), true)
            .show_header(ui, |ui| {
                let r = polozka(
                    ui,
                    self.uzel == Uzel::Skupiny,
                    &Ikona::Svg(ik!("ter_skupina_bodu")),
                    RichText::new(tr("Skupiny bodů")),
                    &[Vpravo::Pocet(stav.skupiny.len().to_string())],
                    false,
                );
                if r.clicked() {
                    self.vyber_uzel(Uzel::Skupiny);
                }
                r.context_menu(|ui| {
                    if ui.button(tr("Nová skupina")).clicked() {
                        spravce::posli(Akce::SkupinaNova(Vec::new()));
                    }
                });
            })
            .body(|ui| {
                for s in &skupiny {
                    let u = Uzel::Skupina(s.skupina.id.clone());
                    let mut t = RichText::new(&s.skupina.jmeno);
                    if s.skupina.id == VSECHNY {
                        t = t.italics();
                    }
                    let ikona = Ikona::Znak("", SEDA);
                    let r = polozka(ui, self.uzel == u, &ikona, t, &[Vpravo::Pocet(cislo(s.pocet))], false);
                    let r = if s.povrchy.is_empty() { r } else { r.on_hover_text(trf!("používá {}", s.povrchy.join(", "))) };
                    if r.clicked() {
                        self.vyber_uzel(u.clone());
                    }
                    r.context_menu(|ui| self.menu_skupiny(ui, stav, &s.skupina));
                }
            });

        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new("u_povrchy"), true)
            .show_header(ui, |ui| {
                let r = polozka(
                    ui,
                    self.uzel == Uzel::Povrchy,
                    &Ikona::Svg(ik!("ter_povrch_novy")),
                    RichText::new(tr("Povrchy")),
                    &[Vpravo::Pocet(stav.povrchy.len().to_string())],
                    false,
                );
                if r.clicked() {
                    self.vyber_uzel(Uzel::Povrchy);
                }
                r.context_menu(|ui| {
                    if ui.button(tr("Nový povrch – průvodce…")).clicked() {
                        self.pruvodce = Some(pruvodce::Pruvodce::novy(stav));
                    }
                    if ui.button(tr("Stávající terén (z bodů)")).clicked() {
                        spravce::posli(Akce::NovyPovrchPas("Stavajici".into(), tr("Stávající terén").into(), true));
                    }
                    if ui
                        .button(tr("Návrh z vybraných 3D čar"))
                        .on_hover_text(tr("nejdřív vyberte ve výkrese 3D čáry (hrany, paty a koruny svahů)"))
                        .clicked()
                    {
                        spravce::posli(Akce::NovyPovrchPas("Navrh".into(), tr("Návrh").into(), false));
                    }
                    ui.separator();
                    if ui.add_enabled(!stav.povrchy.is_empty(), egui::Button::new(tr("Přepočítat všechny"))).clicked() {
                        spravce::posli(Akce::Prepocitat(stav.povrchy.iter().map(|p| p.id.clone()).collect()));
                    }
                });
            })
            .body(|ui| {
                if povrchy.is_empty() {
                    ui.label(RichText::new(if stav.pripojeno { tr("zatím žádný povrch") } else { tr("načítám…") }).color(SEDA).small());
                }
                for p in &povrchy {
                    self.uzel_povrchu(ui, stav, p);
                }
            });

        // ── vzhled ──
        sekce(ui, tr("Vzhled"));
        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new("u_styly_bodu"), false)
            .show_header(ui, |ui| {
                let r = polozka(
                    ui,
                    self.uzel == Uzel::StylyBodu,
                    &Ikona::Svg(ik!("ter_styl_bodu")),
                    RichText::new(tr("Styly bodů")),
                    &[Vpravo::Pocet(stav.styly_bodu.len().to_string())],
                    false,
                );
                if r.clicked() {
                    self.vyber_uzel(Uzel::StylyBodu);
                }
            })
            .body(|ui| {
                let pocty = pocty_stylu(stav);
                for (nadpis_skupiny, symboly) in [(tr("popisy"), false), (tr("symboly"), true)] {
                    ui.label(RichText::new(nadpis_skupiny).small().color(SEDA));
                    for st in stav.styly_bodu.iter().filter(|s| s.znacka.je_symbol() == symboly) {
                        let u = Uzel::StylBodu(st.id.clone());
                        let n = pocty.get(&st.id).copied().unwrap_or(0);
                        ui.horizontal(|ui| {
                            ikona_stylu(ui, st, 16.0);
                            let r = polozka(
                                ui,
                                self.uzel == u,
                                &Ikona::Znak("", SEDA),
                                RichText::new(&st.jmeno),
                                &[Vpravo::Pocet(if n > 0 { cislo(n) } else { String::new() })],
                                n == 0,
                            );
                            if r.clicked() {
                                self.vyber_uzel(u.clone());
                            }
                            r.context_menu(|ui| self.menu_stylu_bodu(ui, stav, st, &u));
                        });
                    }
                }
            });
        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new("u_styly_povrchu"), false)
            .show_header(ui, |ui| {
                let r = polozka(
                    ui,
                    self.uzel == Uzel::StylyPovrchu,
                    &Ikona::Svg(ik!("ter_styl")),
                    RichText::new(tr("Styly povrchů")),
                    &[Vpravo::Pocet(stav.styly_povrchu.len().to_string())],
                    false,
                );
                if r.clicked() {
                    self.vyber_uzel(Uzel::StylyPovrchu);
                }
            })
            .body(|ui| {
                for st in &stav.styly_povrchu {
                    let u = Uzel::StylPovrchu(st.id.clone());
                    let n = stav.povrchy.iter().filter(|p| p.styl.eq_ignore_ascii_case(&st.id)).count();
                    ui.horizontal(|ui| {
                        ikona_stylu_povrchu(ui, st, 16.0);
                        let r = polozka(
                            ui,
                            self.uzel == u,
                            &Ikona::Znak("", SEDA),
                            RichText::new(&st.jmeno),
                            &[Vpravo::Pocet(if n > 0 { n.to_string() } else { String::new() })],
                            n == 0,
                        );
                        if r.clicked() {
                            self.vyber_uzel(u.clone());
                        }
                        r.context_menu(|ui| self.menu_stylu_povrchu(ui, stav, st, &u));
                    });
                }
            });

        // ── nástroje ──
        sekce(ui, tr("Nástroje"));
        let vpravo = match stav.kontrola.as_ref().map(|k| k.len()) {
            Some(n) if n > 0 => vec![Vpravo::Odznak(n.to_string(), ORANZOVA)],
            Some(_) => vec![Vpravo::Tecka(ZELENA)],
            None => Vec::new(),
        };
        let r = list(ui, self.uzel == Uzel::Kontrola, &Ikona::Svg(ik!("ter_vyska_v_bode")), RichText::new(tr("Kontrola zaměření")), &vpravo, false);
        if r.on_hover_text(tr("Najde body, jejichž výška vybočuje z okolí")).clicked() {
            self.vyber_uzel(Uzel::Kontrola);
        }
        if list(ui, self.uzel == Uzel::Nastaveni, &Ikona::Znak("⚙", SEDA), RichText::new(tr("Nastavení")), &[], false).clicked() {
            self.vyber_uzel(Uzel::Nastaveni);
        }
        ui.add_space(6.0);
    }

    fn uzel_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        if self.rozbalit.as_ref() == Some(&id) {
            self.rozbalit = None;
            let mut s = CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new(("u_povrch", id.as_str())), true);
            s.set_open(true);
            s.store(ui.ctx());
        }
        CollapsingState::load_with_default_open(ui.ctx(), egui::Id::new(("u_povrch", id.as_str())), false)
            .show_header(ui, |ui| {
                let skryty = p.styl.eq_ignore_ascii_case(styly::SKRYT);
                let mut t = RichText::new(&p.jmeno);
                if p.aktivni {
                    t = t.strong();
                }
                let mut vpravo = Vec::new();
                if p.aktivni {
                    vpravo.push(Vpravo::Hvezda);
                }
                if p.upozorneni() > 0 {
                    vpravo.push(Vpravo::Odznak(format!("⚠ {}", p.upozorneni()), ORANZOVA));
                }
                vpravo.push(Vpravo::Tecka(if p.zastaraly { ORANZOVA } else { ZELENA }));
                let ikona = if skryty { Ikona::Svg(ik!("ter_sp_skryt")) } else { Ikona::Svg(ik!("ter_vrstevnice")) };
                let mut tip = vec![if p.zastaraly {
                    tr("zastaralý – zdroje se od přepočtu změnily").to_string()
                } else {
                    tr("aktuální").to_string()
                }];
                if p.aktivni {
                    tip.push(tr("★ aktivní povrch (s ním pracují nástroje Úpravy a Nový bod na povrchu)").into());
                }
                if p.auto {
                    tip.push(tr("přepočítává se automaticky").into());
                }
                if skryty {
                    tip.push(tr("skrytý – styl Nezobrazovat").into());
                }
                if !p.aktivni {
                    tip.push(tr("dvojklik = nastavit jako aktivní").into());
                }
                let r = polozka(ui, self.uzel == Uzel::Povrch(id.clone()), &ikona, t, &vpravo, skryty).on_hover_text(tip.join("\n"));
                if r.clicked() {
                    self.vyber_uzel(Uzel::Povrch(id.clone()));
                }
                if r.double_clicked() {
                    spravce::posli(Akce::Aktivovat(id.clone()));
                }
                r.context_menu(|ui| self.menu_povrchu(ui, stav, p));
            })
            .body(|ui| {
                let pocet = |f: &dyn Fn(&DruhPolozky) -> bool| p.polozky.iter().filter(|x| f(&x.druh)).count();
                let casti = [
                    (Cast::Skupiny, p.skupiny.len()),
                    (Cast::Hranice, pocet(&|d| matches!(d, DruhPolozky::Hranice | DruhPolozky::Dira))),
                    (Cast::Spojnice, pocet(&|d| matches!(d, DruhPolozky::Spojnice))),
                    (Cast::Objekty, pocet(&|d| matches!(d, DruhPolozky::Objekt(_)))),
                    (Cast::Upravy, p.upravy.len()),
                ];
                for (c, n) in casti {
                    let u = Uzel::Definice(id.clone(), c);
                    let r = polozka(ui, self.uzel == u, &ikona_casti(c), RichText::new(c.nazev()), &[Vpravo::Pocet(n.to_string())], n == 0);
                    if r.on_hover_text(popis_casti(c)).clicked() {
                        self.vyber_uzel(u);
                    }
                }
                let n = p.upozorneni();
                if n > 0 {
                    let u = Uzel::Upozorneni(id.clone());
                    let r = polozka(
                        ui,
                        self.uzel == u,
                        &Ikona::Znak("⚠", ORANZOVA),
                        RichText::new(tr("Upozornění")).color(ORANZOVA),
                        &[Vpravo::Odznak(n.to_string(), ORANZOVA)],
                        false,
                    );
                    if r.clicked() {
                        self.vyber_uzel(u);
                    }
                }
            });
    }

    fn menu_stylu_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, st: &crate::styly::StylBodu, u: &Uzel) {
        if ui.button(tr("Upravit styl")).clicked() {
            self.vyber_uzel(u.clone());
        }
        if ui.button(tr("Kopírovat jako nový")).clicked() {
            let mut n = st.clone();
            n.id = String::new();
            n.jmeno = trf!("{} (kopie)", st.jmeno);
            spravce::posli(Akce::UlozStylBodu(n));
        }
        if st.je_vestaveny() {
            if stav.ulozene_styly.contains(&st.id) && ui.button(tr("Vrátit původní nastavení")).clicked() {
                spravce::posli(Akce::SmazStyl(st.id.clone()));
            }
        } else if ui.button(RichText::new(tr("Smazat styl")).color(CERVENA)).clicked() {
            spravce::posli(Akce::SmazStyl(st.id.clone()));
        }
    }

    fn menu_stylu_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, st: &crate::styly::StylPovrchu, u: &Uzel) {
        if ui.button(tr("Upravit styl")).clicked() {
            self.vyber_uzel(u.clone());
        }
        if ui.button(tr("Kopírovat jako nový")).clicked() {
            let mut n = st.clone();
            n.id = String::new();
            n.jmeno = trf!("{} (kopie)", st.jmeno);
            spravce::posli(Akce::UlozStylPovrchu(n));
        }
        if st.je_vestaveny() {
            if stav.ulozene_styly.contains(&st.id) && ui.button(tr("Vrátit původní nastavení")).clicked() {
                spravce::posli(Akce::SmazStyl(st.id.clone()));
            }
        } else if ui.button(RichText::new(tr("Smazat styl")).color(CERVENA)).clicked() {
            spravce::posli(Akce::SmazStyl(st.id.clone()));
        }
    }
}
