//! Prohlížeč povrchu 3D: otáčení, převýšení, barvy podle výšky nebo sklonu.
//! Kreslí se v okně Prospektoru (egui), výkres se nemění.

use teren::preklad::tr;
use teren::trf;
use super::*;
use crate::spravce::Sit3D;
use egui::{epaint::Vertex, Mesh, PointerButton, Pos2, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Barvy {
    #[default]
    Vyska,
    Jednobarevne,
    Sklon,
}

pub(super) struct Pohled3D {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
    prevyseni: f32,
    barvy: Barvy,
    hrany: bool,
    body: bool,
    vrstevnice: bool,
    obrys: bool,
    spojnice: bool,
    /// Povrchy, které chce uživatel vidět (načítají se ve správci).
    pub povrchy: Vec<String>,
    /// Pro jaké povrchy jsou načtené sítě.
    nacteno: Vec<String>,
    /// Bod, kolem kterého se právě otáčí (místní souřadnice).
    pivot: Option<[f32; 3]>,
    /// Bod terénu pod kurzorem z minulého snímku (místní souřadnice).
    pod_kurzorem: Option<[f32; 3]>,
    /// Otisk povrchů ve výkrese – při změně se síť načte znovu.
    otisk: String,
    /// Požadavek na uložení obrázku a výřez okna, který se uloží.
    foto: bool,
    foto_vyrez: Option<egui::Rect>,
    pub(super) zprava: String,
}

impl Default for Pohled3D {
    fn default() -> Self {
        Self {
            yaw: -35f32.to_radians(),
            pitch: 40f32.to_radians(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            prevyseni: 2.0,
            barvy: Barvy::Vyska,
            hrany: true,
            body: false,
            vrstevnice: false,
            obrys: false,
            spojnice: false,
            povrchy: Vec::new(),
            nacteno: Vec::new(),
            pivot: None,
            pod_kurzorem: None,
            otisk: String::new(),
            foto: false,
            foto_vyrez: None,
            zprava: String::new(),
        }
    }
}

impl Pohled3D {
    pub(super) fn otevri(&mut self, povrchy: Vec<String>) {
        if povrchy != self.nacteno {
            self.povrchy = povrchy;
            self.nacti();
        }
    }

    fn nacti(&mut self) {
        self.nacteno = self.povrchy.clone();
        spravce::posli(Akce::Nacti3D(self.povrchy.clone()));
    }

    fn pohled_ze_strany(&mut self, yaw_stupne: f32) {
        self.yaw = yaw_stupne.to_radians();
        self.pitch = 25f32.to_radians();
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    fn vychozi(&mut self) {
        self.yaw = -35f32.to_radians();
        self.pitch = 40f32.to_radians();
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }
}

/// Barevná stupnice výšky (modrá – zelená – žlutá – hnědá).
fn barva_vysky(t: f32) -> [f32; 3] {
    let stupne: [(f32, [f32; 3]); 5] = [
        (0.0, [44.0, 110.0, 170.0]),
        (0.25, [70.0, 160.0, 120.0]),
        (0.5, [150.0, 185.0, 95.0]),
        (0.75, [200.0, 160.0, 90.0]),
        (1.0, [150.0, 100.0, 70.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in stupne.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let f = (t - a.0) / (b.0 - a.0);
            return [0, 1, 2].map(|k| a.1[k] + (b.1[k] - a.1[k]) * f);
        }
    }
    stupne[4].1
}

/// Barva podle sklonu ve stupních (rovina zelená, strmé červené).
fn barva_sklonu(st: f32) -> [f32; 3] {
    let t = (st / 35.0).clamp(0.0, 1.0);
    if t < 0.5 {
        let f = t / 0.5;
        [90.0 + 140.0 * f, 170.0 + 30.0 * f, 90.0 - 20.0 * f]
    } else {
        let f = (t - 0.5) / 0.5;
        [230.0, 200.0 - 130.0 * f, 70.0 - 10.0 * f]
    }
}

fn barva(c: [f32; 3], svetlo: f32, alfa: u8) -> Color32 {
    let k = |v: f32| (v * svetlo).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(k(c[0]), k(c[1]), k(c[2]), alfa)
}

/// Čára jako tenký čtyřúhelník v síti (kvůli pořadí kreslení).
fn cara_do_site(m: &mut Mesh, a: Pos2, b: Pos2, w: f32, c: Color32) {
    let d = b - a;
    let l = d.length();
    if l < 0.5 {
        return;
    }
    let n = Vec2::new(-d.y, d.x) / l * (w / 2.0);
    let i = m.vertices.len() as u32;
    for p in [a + n, a - n, b - n, b + n] {
        m.vertices.push(Vertex { pos: p, uv: egui::epaint::WHITE_UV, color: c });
    }
    m.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
}

/// Uloží obrázek jako PNG do složky výkresu (jinak do Obrázků uživatele).
fn uloz_png(img: &egui::ColorImage, slozka: &str, jmeno: &str) -> Result<String, String> {
    let mut adresar = std::path::PathBuf::from(slozka);
    if slozka.is_empty() || !adresar.is_dir() {
        let domov = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).ok_or(tr("není kam uložit"))?;
        adresar = std::path::PathBuf::from(domov).join("Pictures");
        if !adresar.is_dir() {
            adresar = adresar.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }
    }
    let cisty: String = jmeno.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let mut cesta = adresar.join(format!("Teren 3D - {}.png", cisty.trim()));
    let mut n = 2;
    while cesta.exists() {
        cesta = adresar.join(format!("Teren 3D - {} ({n}).png", cisty.trim()));
        n += 1;
    }
    let soubor = std::fs::File::create(&cesta).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(soubor), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = img
        .pixels
        .iter()
        .flat_map(|c| {
            let [r, g, b, _] = c.to_array();
            [r, g, b, 255]
        })
        .collect();
    w.write_image_data(&data).map_err(|e| e.to_string())?;
    Ok(cesta.display().to_string())
}

impl Okno {
    pub(super) fn panel_3d(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        if self.pohled.povrchy.is_empty() {
            if let Some(p) = stav.povrchy.iter().find(|p| p.aktivni).or(stav.povrchy.first()) {
                self.pohled.otevri(vec![p.id.clone()]);
            }
        }
        // povrch se ve výkrese změnil (přepočet, úprava, styl) → načíst znovu
        let otisk: String = stav
            .povrchy
            .iter()
            .filter(|p| self.pohled.povrchy.contains(&p.id))
            .map(|p| {
                format!(
                    "{}:{}:{:.4}:{:.4}:{:.2}:{}:{}:{}:{};",
                    p.id,
                    p.stat.trojuhelniku,
                    p.stat.z_min,
                    p.stat.z_max,
                    p.stat.plocha_3d,
                    p.upravy.len(),
                    p.polozky.len(),
                    p.interval,
                    p.styl
                )
            })
            .collect();
        if otisk != self.pohled.otisk {
            if !self.pohled.otisk.is_empty() && !self.pohled.povrchy.is_empty() {
                self.pohled.nacti();
            }
            self.pohled.otisk = otisk;
        }
        egui::Panel::left("p3d_nastaveni").default_size(220.0).resizable(false).show(ui, |ui| {
            self.nastaveni_3d(ui, stav);
        });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.kresli_3d(ui, stav);
        });
    }

    fn nastaveni_3d(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let p = &mut self.pohled;
        ui.add_space(6.0);
        ui.label(RichText::new(tr("POVRCHY")).small().strong().color(SEDA));
        let mut zmena = false;
        for s in &stav.povrchy {
            let mut zap = p.povrchy.contains(&s.id);
            if ui.checkbox(&mut zap, &s.jmeno).changed() {
                if zap {
                    p.povrchy.push(s.id.clone());
                } else {
                    p.povrchy.retain(|x| x != &s.id);
                }
                zmena = true;
            }
        }
        if p.povrchy.len() > 1 {
            ui.label(RichText::new(tr("další povrchy poloprůhledně")).small().color(SEDA));
        }
        if zmena {
            p.nacti();
        }
        if ui.small_button(tr("⟳ Načíst znovu")).on_hover_text(tr("po změně povrchu ve výkrese")).clicked() {
            p.nacti();
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tr("PŘEVÝŠENÍ")).small().strong().color(SEDA));
        ui.horizontal(|ui| {
            for k in [1.0, 2.0, 5.0, 10.0] {
                if ui.selectable_label((p.prevyseni - k).abs() < 1e-3, format!("×{k}")).clicked() {
                    p.prevyseni = k;
                }
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(tr("BARVY")).small().strong().color(SEDA));
        ui.radio_value(&mut p.barvy, Barvy::Vyska, tr("podle výšky"));
        ui.radio_value(&mut p.barvy, Barvy::Jednobarevne, tr("jednobarevně, stínovaně"));
        ui.radio_value(&mut p.barvy, Barvy::Sklon, tr("podle sklonu"));
        ui.add_space(6.0);
        ui.label(RichText::new(tr("ZOBRAZIT")).small().strong().color(SEDA));
        let mut barvy = nastaveni::akt().barvy_3d;
        let puvodni = barvy;
        for (i, zap, text) in [
            (4, &mut p.hrany, tr("hrany trojúhelníků")),
            (3, &mut p.body, tr("body")),
            (2, &mut p.vrstevnice, tr("vrstevnice")),
            (1, &mut p.obrys, tr("obrys")),
            (0, &mut p.spojnice, tr("spojnice (lomové hrany)")),
        ] {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut barvy[i]).on_hover_text(tr("barva"));
                ui.checkbox(zap, text);
            });
        }
        if barvy != puvodni {
            nastaveni::zmen(|n| n.barvy_3d = barvy);
        }
        if barvy != nastaveni::BARVY_3D && ui.small_button(tr("výchozí barvy")).clicked() {
            nastaveni::zmen(|n| n.barvy_3d = nastaveni::BARVY_3D);
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tr("POHLED")).small().strong().color(SEDA));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Izometrie")).on_hover_text("Ctrl+7").clicked() {
                p.vychozi();
            }
            if ui.button(tr("Shora")).on_hover_text("Ctrl+1").clicked() {
                p.pitch = 90f32.to_radians();
                p.yaw = 0.0;
                p.zoom = 1.0;
                p.pan = Vec2::ZERO;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("od")).color(SEDA));
            for (t, y, napoveda) in [(svet("J"), 0.0, tr("od jihu")), (svet("S"), 180.0, tr("od severu")), (svet("V"), -90.0, tr("od východu")), (svet("Z"), 90.0, tr("od západu"))] {
                if ui.button(t).on_hover_text(napoveda).clicked() {
                    p.pohled_ze_strany(y);
                }
            }
        });
        if ui.button(tr("Uložit obrázek…")).on_hover_text(tr("PNG do složky výkresu")).clicked() {
            p.foto = true;
        }
        if !p.zprava.is_empty() {
            ui.label(RichText::new(&p.zprava).small().color(ZELENA));
        }
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                tr("Ovládání jako v SolidWorksu:\n\
                 Prostřední tl. – otáčení kolem místa pod kurzorem\n\
                 Ctrl + prostřední / pravé – posun\n\
                 Shift + prostřední – zoom\n\
                 Kolečko – zoom ke kurzoru\n\
                 Levé tl. – také otáčení\n\
                 Dvojklik prostředním / F – celý povrch\n\
                 Dvojklik – vycentrovat místo\n\
                 Ctrl + dvojklik – ukázat ve výkrese\n\
                 Šipky – otáčení po 15°, Shift 90°\n\
                 Ctrl + šipky – posun, Z / Shift+Z – zoom"),
            )
            .small()
            .color(SEDA),
        );
    }

    fn kresli_3d(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let site: Vec<&Sit3D> = stav.site3d.iter().filter(|s| self.pohled.povrchy.contains(&s.id)).collect();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(21, 22, 23));
        if site.is_empty() {
            let t = if stav.povrchy.is_empty() { tr("Ve výkrese není žádný povrch.") } else { tr("Načítám povrch…") };
            painter.text(rect.center(), Align2::CENTER_CENTER, t, FontId::proportional(15.0), SEDA);
            return;
        }
        let p = &mut self.pohled;
        // uložení obrázku: požádat o snímek okna, pak z něj vyříznout pohled
        if p.foto {
            p.foto = false;
            p.foto_vyrez = Some(rect);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if let Some(vyrez) = p.foto_vyrez {
            let snimek = ui.input(|i| {
                i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
            });
            if let Some(img) = snimek {
                p.foto_vyrez = None;
                let jmeno: Vec<&str> = site.iter().map(|s| s.jmeno.as_str()).collect();
                p.zprava = match uloz_png(&img.region(&vyrez, Some(ui.ctx().pixels_per_point())), &stav.slozka, &jmeno.join(" + ")) {
                    Ok(c) => trf!("Uloženo: {}", c),
                    Err(e) => trf!("Obrázek se nepodařilo uložit: {}", e),
                };
            }
        }
        let tahne = resp.dragged();

        // rozsah všech sítí
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        let (mut zmin, mut zmax) = (f64::MAX, f64::MIN);
        for s in &site {
            for v in &s.vrcholy {
                x0 = x0.min(v[0]);
                x1 = x1.max(v[0]);
                y0 = y0.min(v[1]);
                y1 = y1.max(v[1]);
            }
            zmin = zmin.min(s.z_min);
            zmax = zmax.max(s.z_max);
        }
        let (cx, cy, cz) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, ((zmin + zmax) / 2.0) as f32);
        let rozmer = (x1 - x0).max(y1 - y0).max(1.0);
        let ex = p.prevyseni;
        let zaklad = rect.width().min(rect.height()) * 0.85 / rozmer;
        let promitni = |yaw: f32, pitch: f32, zoom: f32, pan: Vec2, v: [f32; 3]| -> Pos2 {
            let (sy, cyw) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let (x, y, z) = (v[0] - cx, v[1] - cy, (v[2] - cz) * ex);
            let x1 = x * cyw - y * sy;
            let y1 = x * sy + y * cyw;
            rect.center() + pan + Vec2::new(x1, -(y1 * sp + z * cp)) * (zaklad * zoom)
        };
        let stred_modelu = [cx, cy, cz];

        // ── ovládání (jako SolidWorks) ──
        let (mods, klavesy) = ui.input(|i| {
            let k = |key: egui::Key| i.key_pressed(key);
            (
                i.modifiers,
                [
                    k(egui::Key::ArrowLeft),
                    k(egui::Key::ArrowRight),
                    k(egui::Key::ArrowUp),
                    k(egui::Key::ArrowDown),
                    k(egui::Key::F),
                    k(egui::Key::Z),
                    k(egui::Key::Num1),
                    k(egui::Key::Num7),
                ],
            )
        });
        let prostredni = resp.dragged_by(PointerButton::Middle);
        let rotace = (prostredni && !mods.ctrl && !mods.shift) || resp.dragged_by(PointerButton::Primary);
        let posun = resp.dragged_by(PointerButton::Secondary) || (prostredni && mods.ctrl);
        let zoom_tazenim = prostredni && mods.shift && !mods.ctrl;
        if resp.drag_started() {
            p.pivot = Some(p.pod_kurzorem.unwrap_or(stred_modelu));
        }
        if !resp.dragged() {
            p.pivot = None;
        }
        let otoc = |p: &mut Pohled3D, dyaw: f32, dpitch: f32, pivot: [f32; 3]| {
            let pred = promitni(p.yaw, p.pitch, p.zoom, p.pan, pivot);
            p.yaw += dyaw;
            p.pitch = (p.pitch + dpitch).clamp(3f32.to_radians(), 90f32.to_radians());
            let po = promitni(p.yaw, p.pitch, p.zoom, p.pan, pivot);
            p.pan += pred - po;
        };
        let citlivost = std::f32::consts::PI / rect.width().min(rect.height()).max(200.0) * 1.3;
        if rotace {
            let d = resp.drag_delta();
            let pivot = p.pivot.unwrap_or(stred_modelu);
            otoc(p, d.x * citlivost, d.y * citlivost, pivot);
        }
        if posun {
            p.pan += resp.drag_delta();
        }
        let zoom_k = |p: &mut Pohled3D, f: f32, k: Option<Pos2>| {
            let f2 = (p.zoom * f).clamp(0.05, 300.0) / p.zoom;
            match k {
                // zoom ke kurzoru
                Some(m) => {
                    let c = rect.center() + p.pan;
                    p.pan += (m - c) * (1.0 - f2);
                }
                // oddalování ke středu okna
                None => p.pan *= f2,
            }
            p.zoom *= f2;
        };
        if zoom_tazenim {
            zoom_k(p, (-resp.drag_delta().y * 0.01).exp(), None);
        }
        if resp.hovered() {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            if s > 0.0 {
                zoom_k(p, (s * 0.0015).exp(), resp.hover_pos());
            } else if s < 0.0 {
                zoom_k(p, (s * 0.0015).exp(), None);
            }
            let krok = if mods.shift { 90f32 } else { 15f32 }.to_radians();
            let [vlevo, vpravo, nahoru, dolu, f, z, n1, n7] = klavesy;
            if mods.ctrl {
                let d = 40.0;
                if vlevo { p.pan.x -= d }
                if vpravo { p.pan.x += d }
                if nahoru { p.pan.y -= d }
                if dolu { p.pan.y += d }
                if n1 {
                    p.pitch = 90f32.to_radians();
                    p.yaw = 0.0;
                    p.zoom = 1.0;
                    p.pan = Vec2::ZERO;
                }
                if n7 {
                    p.vychozi();
                }
            } else {
                if vlevo { otoc(p, -krok, 0.0, stred_modelu) }
                if vpravo { otoc(p, krok, 0.0, stred_modelu) }
                if nahoru { otoc(p, 0.0, -krok, stred_modelu) }
                if dolu { otoc(p, 0.0, krok, stred_modelu) }
                if z {
                    zoom_k(p, if mods.shift { 1.25 } else { 0.8 }, None);
                }
            }
            if f {
                p.zoom = 1.0;
                p.pan = Vec2::ZERO;
            }
        }
        if resp.double_clicked_by(PointerButton::Middle) {
            p.zoom = 1.0;
            p.pan = Vec2::ZERO;
        }

        let (sy, cyw) = p.yaw.sin_cos();
        let (sp, cp) = p.pitch.sin_cos();
        let stred = rect.center() + p.pan;
        let meritko = zaklad * p.zoom;
        // projekce: (obrazovka, hloubka – větší = dál)
        let proj = |v: [f32; 3]| -> (Pos2, f32) {
            let (x, y, z) = (v[0] - cx, v[1] - cy, (v[2] - cz) * ex);
            let x1 = x * cyw - y * sy;
            let y1 = x * sy + y * cyw;
            let sx = x1;
            let syy = -(y1 * sp + z * cp);
            let hloubka = y1 * cp - z * sp;
            (stred + Vec2::new(sx, syy) * meritko, hloubka)
        };
        let svetlo = {
            let l = [-0.45f32, 0.55, 0.7];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            l.map(|x| x / n)
        };
        let rozsah_z = (zmax - zmin).max(0.01) as f32;
        let hrany = p.hrany && !(tahne && site.iter().map(|s| s.troj.len()).sum::<usize>() > 30_000);

        let barvy3d = nastaveni::akt().barvy_3d;
        let mut najeto: Option<[f64; 3]> = None;
        let mut mistni: Option<[f32; 3]> = None;
        let mys = resp.hover_pos();
        for (poradi, s) in site.iter().enumerate() {
            let alfa = if poradi == 0 { 255 } else { 150 };
            let pv: Vec<(Pos2, f32)> = s.vrcholy.iter().map(|v| proj(*v)).collect();
            let mut tri: Vec<(f32, usize)> =
                s.troj.iter().enumerate().map(|(i, t)| ((pv[t[0] as usize].1 + pv[t[1] as usize].1 + pv[t[2] as usize].1) / 3.0, i)).collect();
            tri.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut m = Mesh::default();
            let [hr, hg, hb] = barvy3d[4];
            let hrana_barva = Color32::from_rgba_unmultiplied(hr, hg, hb, 110);
            for &(_, i) in &tri {
                let t = s.troj[i];
                let [a, b, c] = t.map(|k| s.vrcholy[k as usize]);
                // normála (s převýšením pro stínování, bez něj pro sklon)
                let u = [b[0] - a[0], b[1] - a[1], (b[2] - a[2]) * ex];
                let w = [c[0] - a[0], c[1] - a[1], (c[2] - a[2]) * ex];
                let mut n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                n = n.map(|x| x / nl);
                if n[2] < 0.0 {
                    n = n.map(|x| -x);
                }
                let osv = 0.5 + 0.6 * (n[0] * svetlo[0] + n[1] * svetlo[1] + n[2] * svetlo[2]).max(0.0);
                let zc = (a[2] + b[2] + c[2]) / 3.0;
                let zakl = match p.barvy {
                    Barvy::Vyska => barva_vysky((zc - zmin as f32) / rozsah_z),
                    Barvy::Jednobarevne => [175.0, 180.0, 165.0],
                    Barvy::Sklon => {
                        let u0 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let w0 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        let n0 = [u0[1] * w0[2] - u0[2] * w0[1], u0[2] * w0[0] - u0[0] * w0[2], u0[0] * w0[1] - u0[1] * w0[0]];
                        let l0 = (n0[0] * n0[0] + n0[1] * n0[1] + n0[2] * n0[2]).sqrt().max(1e-9);
                        barva_sklonu((n0[2].abs() / l0).acos().to_degrees())
                    }
                };
                let col = barva(zakl, osv, alfa);
                let pa = pv[t[0] as usize].0;
                let pb = pv[t[1] as usize].0;
                let pc = pv[t[2] as usize].0;
                let base = m.vertices.len() as u32;
                for q in [pa, pb, pc] {
                    m.vertices.push(Vertex { pos: q, uv: egui::epaint::WHITE_UV, color: col });
                }
                m.indices.extend_from_slice(&[base, base + 1, base + 2]);
                if hrany && poradi == 0 {
                    cara_do_site(&mut m, pa, pb, 0.7, hrana_barva);
                    cara_do_site(&mut m, pb, pc, 0.7, hrana_barva);
                    cara_do_site(&mut m, pc, pa, 0.7, hrana_barva);
                }
            }
            painter.add(egui::Shape::mesh(m));
            // výška pod kurzorem (nejbližší trojúhelník první povrchu)
            if poradi == 0 && !tahne {
                if let Some(mp) = mys {
                    for &(_, i) in tri.iter().rev() {
                        let t = s.troj[i];
                        let (a, b, c) = (pv[t[0] as usize].0, pv[t[1] as usize].0, pv[t[2] as usize].0);
                        let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
                        if d.abs() < 1e-6 {
                            continue;
                        }
                        let l1 = ((b.y - c.y) * (mp.x - c.x) + (c.x - b.x) * (mp.y - c.y)) / d;
                        let l2 = ((c.y - a.y) * (mp.x - c.x) + (a.x - c.x) * (mp.y - c.y)) / d;
                        let l3 = 1.0 - l1 - l2;
                        if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                            let [va, vb, vc] = t.map(|k| s.vrcholy[k as usize]);
                            let q = |k: usize| (va[k] * l1 + vb[k] * l2 + vc[k] * l3) as f64;
                            najeto = Some([q(0) + s.pocatek.0, q(1) + s.pocatek.1, q(2)]);
                            mistni = Some([q(0) as f32, q(1) as f32, q(2) as f32]);
                            break;
                        }
                    }
                }
            }
            if poradi == 0 {
                if p.vrstevnice {
                    for (hlavni, b) in &s.vrstevnice {
                        let body: Vec<Pos2> = b.iter().map(|v| proj(*v).0).collect();
                        let [r, g, b2] = barvy3d[2];
                        let c = if *hlavni { Color32::from_rgb(r, g, b2) } else { Color32::from_rgba_unmultiplied(r, g, b2, 150) };
                        painter.add(egui::Shape::line(body, Stroke::new(if *hlavni { 1.4 } else { 0.8 }, c)));
                    }
                }
                let rgb = |i: usize| Color32::from_rgb(barvy3d[i][0], barvy3d[i][1], barvy3d[i][2]);
                if p.obrys {
                    for b in &s.obrys {
                        let body: Vec<Pos2> = b.iter().map(|v| proj(*v).0).collect();
                        painter.add(egui::Shape::line(body, Stroke::new(1.8, rgb(1))));
                    }
                }
                if p.spojnice {
                    for b in &s.spojnice {
                        let body: Vec<Pos2> = b.iter().map(|v| proj(*v).0).collect();
                        painter.add(egui::Shape::line(body, Stroke::new(1.6, rgb(0))));
                    }
                }
                if p.body {
                    for &i in &s.body {
                        painter.circle_filled(pv[i as usize].0, 2.0, rgb(3));
                    }
                }
            }
        }

        // kurzor a údaje
        if let (Some(m), Some([x, y, z])) = (mys, najeto) {
            painter.circle_stroke(m, 6.0, Stroke::new(1.5, Color32::WHITE));
            let t = format!("Z {z:.2} m\nY {:.2}\nX {:.2}", -x, -y);
            let g = painter.layout_no_wrap(t, FontId::monospace(12.0), Color32::WHITE);
            let r = egui::Rect::from_min_size(m + Vec2::new(14.0, 10.0), g.size() + Vec2::splat(10.0));
            painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(20, 20, 20, 225));
            painter.galley(r.min + Vec2::splat(5.0), g, Color32::WHITE);
            if resp.double_clicked() {
                if mods.ctrl {
                    spravce::posli(Akce::Ukazat([x, y], 15.0, Vec::new()));
                } else {
                    // vycentrovat místo
                    p.pan += rect.center() - m;
                }
            }
        }
        if !tahne {
            p.pod_kurzorem = mistni;
        }
        // bod otáčení
        if let (Some(pv), true) = (p.pivot, tahne && !posun) {
            let q = proj(pv).0;
            painter.circle_stroke(q, 5.0, Stroke::new(1.5, Color32::from_rgb(255, 210, 80)));
            painter.line_segment([q - Vec2::new(9.0, 0.0), q + Vec2::new(9.0, 0.0)], Stroke::new(1.2, Color32::from_rgb(255, 210, 80)));
            painter.line_segment([q - Vec2::new(0.0, 9.0), q + Vec2::new(0.0, 9.0)], Stroke::new(1.2, Color32::from_rgb(255, 210, 80)));
        }

        // legenda
        if p.barvy != Barvy::Jednobarevne {
            let l = egui::Rect::from_min_size(rect.right_top() + Vec2::new(-120.0, 14.0), Vec2::new(106.0, 190.0));
            painter.rect_filled(l, 6.0, Color32::from_rgba_unmultiplied(27, 27, 27, 230));
            let (nadpis, popisy): (&str, Vec<String>) = match p.barvy {
                Barvy::Sklon => (tr("Sklon [°]"), ["35+", "26", "17", "9", "0"].map(String::from).to_vec()),
                _ => (tr("Výška [m n. m.]"), (0..5).rev().map(|k| format!("{:.2}", zmin + (zmax - zmin) * k as f64 / 4.0)).collect()),
            };
            painter.text(l.min + Vec2::new(8.0, 8.0), Align2::LEFT_TOP, nadpis, FontId::proportional(11.0), SEDA);
            let pruh = egui::Rect::from_min_size(l.min + Vec2::new(10.0, 28.0), Vec2::new(14.0, 150.0));
            let n = 30;
            for k in 0..n {
                let t = 1.0 - k as f32 / (n - 1) as f32;
                let c = match p.barvy {
                    Barvy::Sklon => barva_sklonu(t * 35.0),
                    _ => barva_vysky(t),
                };
                let r = egui::Rect::from_min_size(pruh.min + Vec2::new(0.0, k as f32 * 5.0), Vec2::new(14.0, 5.0));
                painter.rect_filled(r, 0.0, barva(c, 1.0, 255));
            }
            for (k, t) in popisy.iter().enumerate() {
                painter.text(
                    pruh.right_top() + Vec2::new(8.0, k as f32 * 37.5),
                    Align2::LEFT_CENTER,
                    t,
                    FontId::monospace(11.0),
                    Color32::from_rgb(210, 210, 210),
                );
            }
        }

        // orientace (osy Y, X = sever, výška)
        let o = rect.left_bottom() + Vec2::new(44.0, -44.0);
        painter.circle_filled(o, 30.0, Color32::from_rgba_unmultiplied(30, 30, 30, 220));
        let osa = |v: [f32; 3]| {
            let (x, y, z) = (v[0], v[1], v[2]);
            let x1 = x * cyw - y * sy;
            let y1 = x * sy + y * cyw;
            Vec2::new(x1, -(y1 * sp + z * cp)) * 24.0
        };
        painter.line_segment([o, o + osa([0.0, 1.0, 0.0])], Stroke::new(2.0, Color32::from_rgb(229, 83, 75)));
        painter.line_segment([o, o + osa([1.0, 0.0, 0.0])], Stroke::new(2.0, Color32::from_rgb(111, 211, 155)));
        painter.line_segment([o, o + osa([0.0, 0.0, 1.0])], Stroke::new(2.0, Color32::from_rgb(95, 176, 255)));
        // klikací světové strany: klik = pohled z té strany
        let mys_klik = if resp.clicked() { resp.interact_pointer_pos() } else { None };
        for (t, smer, yaw) in [(svet("S"), [0.0, 1.0], 180.0), (svet("J"), [0.0, -1.0], 0.0), (svet("V"), [1.0, 0.0], -90.0), (svet("Z"), [-1.0, 0.0], 90.0)] {
            let q = o + osa([smer[0] * 1.3, smer[1] * 1.3, 0.0]);
            let blizko = mys.is_some_and(|m| m.distance(q) < 9.0);
            painter.text(q, Align2::CENTER_CENTER, t, FontId::proportional(if blizko { 13.0 } else { 11.0 }), if blizko { Color32::from_rgb(255, 210, 80) } else { Color32::WHITE });
            if mys_klik.is_some_and(|m| m.distance(q) < 9.0) {
                p.pohled_ze_strany(yaw);
            }
        }

        // stavový řádek
        let n_troj: usize = site.iter().map(|s| s.troj.len()).sum();
        let n_bodu: usize = site.first().map_or(0, |s| s.body.len());
        let jmena: Vec<&str> = site.iter().map(|s| s.jmeno.as_str()).collect();
        let info = trf!("{} · {} trojúhelníků · {} bodů · převýšení ×{} · {:.2} – {:.2} m n. m.", jmena.join(" + "), n_troj, n_bodu, p.prevyseni, zmin, zmax);
        painter.text(rect.left_bottom() + Vec2::new(90.0, -10.0), Align2::LEFT_BOTTOM, info, FontId::proportional(12.0), SEDA);
    }
}

/// Světová strana (S, J, V, Z) v jazyce rozhraní.
fn svet(cs: &'static str) -> &'static str {
    if !teren::preklad::anglicky() {
        return cs;
    }
    match cs {
        "S" => "N",
        "J" => "S",
        "V" => "E",
        _ => "W",
    }
}
