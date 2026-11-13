//! Ukázka: okno samo projde stránky a vyfotí je do PNG (proměnná prostředí
//! TEREN_FOTO = složka). Jen pro vývoj (feature `ukazka`).

use std::path::PathBuf;

use eframe::egui;

use super::{Cast, Okno, Uzel};

pub(super) struct Foto {
    kroky: Vec<(&'static str, Uzel)>,
    i: usize,
    snimek: u32,
    ceka: bool,
    slozka: PathBuf,
}

impl Foto {
    pub(super) fn z_prostredi() -> Option<Self> {
        let slozka = PathBuf::from(std::env::var_os("TEREN_FOTO")?);
        let _ = std::fs::create_dir_all(&slozka);
        let kroky = vec![
            ("01_vykres", Uzel::Vykres),
            ("02_vybrano", Uzel::Vyber),
            ("03_body", Uzel::Body),
            ("04_skupiny", Uzel::Skupiny),
            ("05_skupina_zamereni", Uzel::Skupina("zam".into())),
            ("06_skupina_vsechny", Uzel::Skupina(teren::skupiny::VSECHNY.into())),
            ("07_povrchy", Uzel::Povrchy),
            ("08_povrch_stavajici", Uzel::Povrch("st".into())),
            ("09_povrch_navrh", Uzel::Povrch("np".into())),
            ("10_povrch_zdroje", Uzel::Definice("np".into(), Cast::Spojnice)),
            ("11_styly_bodu", Uzel::StylyBodu),
            ("12_styly_povrchu", Uzel::StylyPovrchu),
            ("13_kontrola", Uzel::Kontrola),
            ("14_nastaveni", Uzel::Nastaveni),
            ("15_styl_bodu", Uzel::StylBodu(crate::styly::STANDARD.into())),
        ];
        Some(Self { kroky, i: 0, snimek: 0, ceka: false, slozka })
    }
}

impl Okno {
    pub(super) fn foto_krok(&mut self, ctx: &egui::Context) {
        if self.foto.is_none() {
            match Foto::z_prostredi() {
                Some(f) => {
                    let w: f32 = std::env::var("TEREN_FOTO_SIRKA").ok().and_then(|s| s.parse().ok()).unwrap_or(1100.0);
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, 720.0)));
                    self.foto = Some(f);
                }
                None => return,
            }
        }
        ctx.request_repaint();
        let obrazek = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
        });
        let Some(f) = self.foto.as_mut() else { return };
        if f.i >= f.kroky.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some(img) = obrazek {
            if f.ceka {
                let cesta = f.slozka.join(format!("{}.png", f.kroky[f.i].0));
                uloz_png(&cesta, &img);
                f.i += 1;
                f.snimek = 0;
                f.ceka = false;
            }
            return;
        }
        if f.ceka {
            return;
        }
        if f.snimek == 0 {
            let u = f.kroky[f.i].1.clone();
            f.snimek = 1;
            self.vyber_uzel(u);
            return;
        }
        f.snimek += 1;
        if f.snimek >= 8 {
            f.ceka = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

fn uloz_png(cesta: &std::path::Path, img: &egui::ColorImage) {
    let Ok(soubor) = std::fs::File::create(cesta) else { return };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(soubor), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    if let Ok(mut w) = enc.write_header() {
        let _ = w.write_image_data(&data);
    }
}
