//! Nápověda (F1) a slovníček pojmů.

use teren::preklad::{tr, tr_dyn};
use eframe::egui::{self, RichText};

use super::{Okno, SEDA};

/// Slovníček – stejné vysvětlivky ukazuje „?“ vedle pojmů v Prospektoru.
pub const POJMY: &[(&str, &str)] = &[
    ("Aktivní povrch", "Povrch, se kterým pracují nástroje Úpravy trojúhelníků, Výška v bodě a Bod na povrchu. Nastavíte ho v horním řádku Prospektoru nebo dvojklikem na povrch ve stromu."),
    ("Skupina bodů", "Pravidla, podle kterých se vyberou body (kód, čísla, soubor importu). Povrch se skládá ze skupin – změna skupiny povrch označí jako zastaralý."),
    ("Spojnice (lomová hrana)", "3D křivka, kterou trojúhelníky nesmí překřížit – hrana svahu, pata a koruna, osa příkopu. Její vrcholy jsou vždy součástí povrchu."),
    ("Hranice / díra", "Uzavřená křivka: povrch se ořízne na hranici, uvnitř díry se vynechá."),
    ("Ruční úprava", "Otočená, přidaná či smazaná hrana, smazaný trojúhelník, přidaný nebo odebraný bod. Ukládá se k povrchu, při přepočtu se použije znovu a jde vypnout."),
    ("Zastaralý povrch", "Body, skupiny nebo spojnice se od posledního přepočtu změnily. Přepočítejte ho, nebo zapněte automatický přepočet."),
    ("Styl bodu", "Jak se bod kreslí: značka (křížek, symbol stromu…) a popisy (číslo, výška, kód). Platí ručně nastavený styl, jinak podle kódu, jinak podle skupiny, jinak Standard."),
    ("Styl povrchu", "Co se z povrchu kreslí: trojúhelníky, vrstevnice, popisy vrstevnic, obrys, šipky spádu – a v jakých barvách."),
    ("Kód bodu", "První slovo popisu bodu ze zaměření (T, HR, STR…). Podle kódu se volí styl a skupina; kód STR (stromy) se do terénu standardně nebere."),
];

/// Vysvětlivka pojmu (pro „?“).
pub fn vysvetlivka(pojem: &str) -> &'static str {
    POJMY.iter().find(|(p, _)| p.eq_ignore_ascii_case(pojem) || tr(p).eq_ignore_ascii_case(pojem)).map_or("", |(_, v)| tr(v))
}

/// Malý „?“ s vysvětlivkou po najetí myší.
pub fn otaznik(ui: &mut egui::Ui, pojem: &str) {
    let t = vysvetlivka(pojem);
    if !t.is_empty() {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
        let barva = if resp.hovered() { ui.visuals().strong_text_color() } else { SEDA };
        ui.painter().circle_stroke(r.center(), 6.5, egui::Stroke::new(1.0, barva));
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(10.5), barva);
        resp.on_hover_ui(|ui| {
                ui.set_max_width(320.0);
                ui.label(RichText::new(tr_dyn(pojem)).strong());
                ui.label(t);
            });
    }
}

struct Tema {
    id: &'static str,
    oddil: &'static str,
    nazev: &'static str,
    /// Odstavce; řádek začínající „1. “ atd. je krok postupu.
    text: &'static [&'static str],
}

const TEMATA: &[Tema] = &[
    Tema {
        id: "teren",
        oddil: "Postupy",
        nazev: "Terén ze zaměření",
        text: &[
            "1. Importujte body: Terén › Body › Import bodů. Formát i oddělovač se rozpoznají samy, body se objeví v Prospektoru pod Body.",
            "2. Vytvořte povrch: Nový povrch › Stávající terén (z bodů), nebo v Prospektoru pravým tlačítkem na Povrchy. Vezme skupinu Zaměření bez kódů STR.",
            "3. Doplňte spojnice (lomové hrany): Spojnice přes body – klikejte na body zaměření; nebo vyberte hotové 3D křivky a klikněte Přidat spojnici.",
            "4. Ořízněte povrch: nakreslete uzavřenou křivku a klikněte Hranice (díra vynechá vnitřek).",
            "5. Prohlédněte trojúhelníky (styl Trojúhelníky nebo záložka 3D) a opravte je nástroji ze skupiny Úpravy trojúhelníků.",
            "6. Nastavte styl s vrstevnicemi a krok vrstevnic – pravým tlačítkem na povrch › Styl / Vrstevnice po.",
            "Povrch se přepočítá sám, když je zapnutý automatický přepočet; jinak ukáže ⚠ zastaralý a stačí kliknout Přepočítat.",
        ],
    },
    Tema {
        id: "upravy",
        oddil: "Postupy",
        nazev: "Úprava trojúhelníků",
        text: &[
            "Nástroje Otočit hranu, Přidat hranu, Smazat hranu, Smazat trojúhelník, Přidat bod a Odebrat bod pracují s aktivním povrchem. Klikejte do výkresu, změna se projeví hned; Enter nebo Esc nástroj ukončí.",
            "Každá úprava se uloží k povrchu (strom › Z čeho se povrch počítá › Ruční úpravy trojúhelníků). Tam ji můžete ukázat, vypnout nebo smazat.",
            "Hranu, kterou drží spojnice, nejde otočit ani smazat. Bod, který je vrcholem spojnice, zůstane v povrchu, i když ho odeberete – nejdřív upravte spojnici.",
        ],
    },
    Tema {
        id: "styly",
        oddil: "Postupy",
        nazev: "Styly a symboly",
        text: &[
            "Styl bodu určuje značku a popisy. Přiřadí se ručně (pás karet › Styl bodu, nebo pravým tlačítkem na řádek tabulky), jinak podle kódu (např. STR = strom), jinak podle skupiny, jinak Standard.",
            "Styl povrchu určuje, co se kreslí – trojúhelníky, vrstevnice s popisy, obrys, spád. Mění se pravým tlačítkem na povrch › Styl.",
            "Styly upravíte v Prospektoru pod Styly. Vestavěný styl můžete změnit a vrátit k původnímu nastavení; Kopírovat jako nový vytvoří vlastní styl.",
        ],
    },
    Tema {
        id: "skupiny",
        oddil: "Postupy",
        nazev: "Skupiny bodů",
        text: &[
            "Skupina vybírá body podle pravidel: mají kód, mají číslo (rozsahy 1-50), jsou ze souboru. Zvolte, zda stačí aspoň jedno pravidlo, nebo musí platit všechna.",
            "Vždy vyloučit: kódy, čísla nebo ručně vytvořené body, které do skupiny nepatří, i když splní pravidla.",
            "Ručně přidané a odebrané body mají přednost před pravidly – přidáte je z výběru ve výkrese.",
            "Náhled vpravo barevně ukazuje, které body do skupiny patří; klik na bod řekne proč.",
            "Povrch se skládá ze skupin. Po uložení změněné skupiny se povrch přepočítá (nebo ukáže, že je zastaralý).",
        ],
    },
    Tema {
        id: "prospektor",
        oddil: "Ovládání",
        nazev: "Prospektor",
        text: &[
            "Výběr – ukazuje, co máte vybrané ve výkrese (bod, povrch, čáru) a co s tím jde dělat.",
            "Výkres – strom všeho, co plugin ve výkrese má: body, skupiny, povrchy, styly, nastavení.",
            "3D – prohlížeč povrchů.",
            "Pravé tlačítko na uzlu stromu nebo na řádku tabulky otevře nabídku akcí.",
            "Dvojklik na povrch ho nastaví jako aktivní, dvojklik na bod v tabulce ho ukáže ve výkrese.",
            "Ukázat v Prospektoru (pás karet) skočí na objekt vybraný ve výkrese. F1 otevře tuto nápovědu.",
        ],
    },
    Tema {
        id: "3d",
        oddil: "Ovládání",
        nazev: "Prohlížeč 3D",
        text: &[
            "Ovládání jako v SolidWorksu:",
            "Prostřední tlačítko – otáčení kolem místa pod kurzorem; levé tlačítko také otáčí.",
            "Ctrl + prostřední (nebo pravé) – posun; Shift + prostřední – zoom; kolečko – zoom ke kurzoru.",
            "Dvojklik prostředním nebo F – celý povrch; dvojklik – vycentrovat místo; Ctrl + dvojklik – ukázat ve výkrese.",
            "Šipky – otáčení po 15° (Shift 90°), Ctrl + šipky – posun, Z / Shift+Z – zoom. Ctrl+1 shora, Ctrl+7 izometrie.",
            "Kompas: klik na S, J, V, Z natočí pohled od dané světové strany. Uložit obrázek uloží PNG do složky výkresu.",
        ],
    },
    Tema { id: "pojmy", oddil: "Pojmy", nazev: "Slovníček", text: &[] },
];

impl Okno {
    /// Okno nápovědy uvnitř Prospektoru (F1).
    pub(super) fn okno_napovedy(&mut self, ctx: &egui::Context) {
        let Some(tema) = self.napoveda.clone() else { return };
        let mut otevreno = true;
        let mut nove = None;
        egui::Window::new(tr("Nápověda"))
            .open(&mut otevreno)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([620.0, 440.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        let mut oddil = "";
                        for t in TEMATA {
                            if t.oddil != oddil {
                                oddil = t.oddil;
                                ui.add_space(6.0);
                                ui.label(RichText::new(tr(oddil).to_uppercase()).small().strong().color(SEDA));
                            }
                            if ui.selectable_label(tema == t.id, tr(t.nazev)).clicked() {
                                nove = Some(t.id.to_string());
                            }
                        }
                    });
                    ui.add_space(14.0);
                    egui::ScrollArea::vertical().id_salt("napoveda_s").max_height(420.0).auto_shrink([false, true]).show(ui, |ui| ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width().min(520.0));
                        let t = TEMATA.iter().find(|t| t.id == tema).unwrap_or(&TEMATA[0]);
                        ui.heading(tr(t.nazev));
                        ui.add_space(4.0);
                        if t.id == "pojmy" {
                            for (p, v) in POJMY {
                                ui.label(RichText::new(tr(p)).strong());
                                ui.label(tr(v));
                                ui.add_space(6.0);
                            }
                            ui.label(RichText::new(tr("Stejné vysvětlivky se ukážou po najetí na „?“ vedle pojmu v Prospektoru.")).small().color(SEDA));
                        } else {
                            for odst in t.text {
                                ui.label(tr(odst));
                                ui.add_space(4.0);
                            }
                        }
                    }));
                });
            });
        if let Some(n) = nove {
            self.napoveda = Some(n);
        }
        if !otevreno {
            self.napoveda = None;
        }
    }
}
