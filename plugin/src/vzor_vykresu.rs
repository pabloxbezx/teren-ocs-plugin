//! Vývojová pomůcka (jen s feature `ukazka`): z dosavadního vzorového výkresu
//! (body zaměření + jeden povrch se spojnicemi) sestaví nový vzor – tři
//! oblasti s vlastními skupinami bodů a povrchy, spojnice rozdělené mezi
//! povrchy, u prostředního trasu, niveletu a pohled profilu.
//! Příkaz `TER_DEV_VZOR` v Open CAD Studiu; výkres pak uložit (QSAVE).

use ocs_plugin_api::host::{EntityType, HostApi};
use teren::skupiny::{SeznamCisel, SkupinaBodu};

use crate::cogo;
use crate::povrchy::{self, SkupinaVPovrchu, ZaznamPovrchu};
use crate::vykres::{PresHost, Vykres};

/// Hranice oblastí: body se dělí podle souřadnice Y dvěma největšími mezerami.
fn hranice_oblasti(ys: &mut [f64]) -> [f64; 2] {
    ys.sort_by(|a, b| a.total_cmp(b));
    let mut mezery: Vec<(f64, f64)> = ys.windows(2).map(|w| (w[1] - w[0], (w[0] + w[1]) / 2.0)).collect();
    mezery.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut h = [mezery[0].1, mezery[1].1];
    h.sort_by(|a, b| a.total_cmp(b));
    h
}

fn oblast(y: f64, h: [f64; 2]) -> usize {
    if y < h[0] {
        2
    } else if y < h[1] {
        1
    } else {
        0
    }
}

fn stred_cary(e: &EntityType) -> Option<[f64; 2]> {
    let body: Vec<[f64; 2]> = match e {
        EntityType::Polyline3D(p) => p.vertices.iter().map(|v| [v.position.x, v.position.y]).collect(),
        EntityType::LwPolyline(p) => p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect(),
        EntityType::Line(l) => vec![[l.start.x, l.start.y], [l.end.x, l.end.y]],
        _ => return None,
    };
    let n = body.len() as f64;
    (n > 0.0).then(|| [body.iter().map(|b| b[0]).sum::<f64>() / n, body.iter().map(|b| b[1]).sum::<f64>() / n])
}

pub fn sestav(host: &mut dyn HostApi) {
    let mut v = PresHost(host);
    if let Err(e) = sestav_v(&mut v) {
        v.chyba(&format!("Vzor: {e}"));
    }
}

fn sestav_v(v: &mut PresHost) -> Result<(), String> {
    let doc = v.dokument()?;
    let body = cogo::body(&doc);
    if body.len() < 30 {
        return Err("ve výkrese nejsou body".into()); // i18n:ne
    }
    let mut ys: Vec<f64> = body.iter().map(|b| b.bod.y).collect();
    let h = hranice_oblasti(&mut ys);
    // oblasti číslované od severu (Area 1 = nejvyšší Y); jména anglicky,
    // aby byly anglické i hladiny povrchů (TER-SRF-Area1-…)
    let jmena = ["Area 1", "Area 2", "Area 3"];
    v.zpet("Vzorový výkres"); // i18n:ne

    // 1. skupiny bodů
    let mut id_skupin = Vec::new();
    for (i, jmeno) in jmena.iter().enumerate() {
        let cisla: Vec<String> = body.iter().filter(|b| oblast(b.bod.y, h) == i).map(|b| b.bod.cislo.clone()).collect();
        let mut sez = SeznamCisel::default();
        sez.pridej(&cisla);
        let s = SkupinaBodu {
            id: cogo::volne_id_skupiny(&doc, jmeno),
            jmeno: jmeno.to_string(),
            popis: format!("Survey points – {}", jmeno.to_lowercase()),
            cisla: sez.text(),
            ..Default::default()
        };
        let doc = v.dokument()?;
        cogo::uloz_skupinu(v, &doc, &s)?;
        v.info(&format!("Vzor: skupina „{}“ ({} bodů)", s.jmeno, cisla.len())); // i18n:ne
        id_skupin.push(s.id);
    }

    // 2. povrchy ze skupin; spojnice starého povrchu podle polohy
    let doc = v.dokument()?;
    let stare = povrchy::povrchy(&doc);
    let spoj_stare: Vec<String> = stare.iter().map(|z| z.hladiny().spojnice.to_uppercase()).collect();
    let cary: Vec<EntityType> = doc.model_space_entities().filter(|e| spoj_stare.contains(&e.common().layer.to_uppercase())).cloned().collect();
    let mut id_povrchu = Vec::new();
    for (i, jmeno) in jmena.iter().enumerate() {
        let doc = v.dokument()?;
        let id = povrchy::volne_id(&doc, &format!("Area{}", i + 1));
        let mut z = ZaznamPovrchu::novy(id.clone(), jmeno.to_string(), true);
        z.skupiny = vec![SkupinaVPovrchu { id: id_skupin[i].clone(), zapnuto: true }];
        let hl = z.hladiny().spojnice;
        povrchy::zajisti_hladinu(v, &doc, &hl, 1);
        let mut n = 0;
        for e in cary.iter().filter(|e| stred_cary(e).is_some_and(|c| oblast(c[1], h) == i)) {
            v.uprav(povrchy::prirad_caru(e.clone(), &hl));
            n += 1;
        }
        let r = povrchy::prestav(v, &mut z)?;
        povrchy::vypis(v, &z, &r);
        v.info(&format!("Vzor: povrch „{jmeno}“, spojnic {n}"));
        id_povrchu.push(id);
    }
    // starý společný povrch pryč
    let doc = v.dokument()?;
    let mut prazdne: Vec<String> = Vec::new();
    for z in stare {
        let p = crate::hladiny::povrch(&z.id).to_uppercase();
        prazdne.extend(doc.layers.iter().map(|l| l.name.clone()).filter(|n| {
            let n = n.to_uppercase();
            n == p || n.starts_with(&format!("{p}-"))
        }));
        povrchy::smaz(v, &doc, &z)?;
    }
    // prázdné hladiny smazaných povrchů (OCS hladinu s objekty nesmaže)
    for name in prazdne {
        let _ = v.tabulka(ocs_plugin_api::host::TableOperation::LayerDelete { name, erase_objects: false });
    }
    // osiřelé hladiny starších verzí i se zbytky grafiky: hladiny pluginu se
    // před prvním příkazem přejmenovaly, takže `TER_…` už k ničemu nepatří
    for name in doc.layers.iter().map(|l| l.name.clone()).filter(|n| n.to_uppercase().starts_with("TER_")) {
        let _ = v.tabulka(ocs_plugin_api::host::TableOperation::LayerDelete { name, erase_objects: true });
    }
    v.hotovo();
    v.konec_kroku();

    // 3. trasa u prostředního povrchu: hlavní směr bodů oblasti 2
    let doc = v.dokument()?;
    let b2: Vec<[f64; 3]> = cogo::body(&doc).iter().filter(|b| oblast(b.bod.y, h) == 1).map(|b| [b.bod.x, b.bod.y, b.bod.z]).collect();
    let n = b2.len() as f64;
    let (cx, cy) = (b2.iter().map(|b| b[0]).sum::<f64>() / n, b2.iter().map(|b| b[1]).sum::<f64>() / n);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for b in &b2 {
        let (dx, dy) = (b[0] - cx, b[1] - cy);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let uhel = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (ux, uy) = (uhel.cos(), uhel.sin());
    let t: Vec<f64> = b2.iter().map(|b| (b[0] - cx) * ux + (b[1] - cy) * uy).collect();
    let (tmin, tmax) = (t.iter().cloned().fold(f64::MAX, f64::min) + 3.0, t.iter().cloned().fold(f64::MIN, f64::max) - 3.0);
    let bod = |s: f64, o: f64| [cx + s * ux - o * uy, cy + s * uy + o * ux];
    let p0 = bod(tmin, 0.0);
    let p1 = bod((tmin + tmax) / 2.0, 4.0);
    let p2 = bod(tmax, 0.0);
    use trasy::geometrie::Vrchol;
    let vrcholy = vec![Vrchol::new(p0[0], p0[1], 0.0), Vrchol::new(p1[0], p1[1], 30.0), Vrchol::new(p2[0], p2[1], 0.0)];
    let mut def = trasy::trasa::DefiniceTrasy::nova("T1", "Alignment 1", vrcholy.clone(), trasy::trasa::STYL_ZAKLAD);
    let g = trasy::geometrie::geometrie(&vrcholy, 0.0).map_err(|e| format!("{e:?}"))?;
    // niveleta: z výšek bodů u začátku a konce, uprostřed mírně níž
    let vyska_u = |p: [f64; 2]| {
        b2.iter().min_by(|a, b| (a[0] - p[0]).hypot(a[1] - p[1]).total_cmp(&(b[0] - p[0]).hypot(b[1] - p[1]))).map_or(0.0, |b| b[2])
    };
    let (z0, z2) = (vyska_u(p0) - 0.3, vyska_u(p2) - 0.3);
    let l = g.konec();
    def.nivelety = vec![trasy::niveleta::Niveleta::nova(
        "N1",
        "Grade 1",
        vec![
            trasy::niveleta::VrcholNivelety::new(0.0, (z0 * 100.0).round() / 100.0, 0.0),
            trasy::niveleta::VrcholNivelety::new((l / 2.0 * 10.0).round() / 10.0, (((z0 + z2) / 2.0 - 0.6) * 100.0).round() / 100.0, 0.0),
            trasy::niveleta::VrcholNivelety::new(l, (z2 * 100.0).round() / 100.0, 0.0),
        ],
    )];
    v.zpet("Vzorová trasa"); // i18n:ne
    let ins = crate::tra::objekt::uloz(v, Some(&doc), None, def)?;
    v.info(&format!("Vzor: trasa T1, délka {:.2} m", l)); // i18n:ne

    // 4. pohled profilu vedle oblasti (styl Jednoduchý)
    let doc = v.dokument()?;
    let _ = ins;
    let styly = crate::styly::styly_profilu(&doc);
    let styl = styly.iter().find(|s| s.id == "JEDNODUCHY").cloned().ok_or("styl Jednoduchý chybí")?; // i18n:ne
    let mut dp = trasy::pohled::DefinicePohledu::podle_stylu("P1", "T1", &id_povrchu[1], &styl);
    dp.meritko = 250.0;
    dp.prevyseni = 1.0;
    let xmax = b2.iter().map(|b| b[0]).fold(f64::MIN, f64::max);
    let ymin = b2.iter().map(|b| b[1]).fold(f64::MAX, f64::min);
    crate::tra::pohled::uloz(v, &doc, None, &dp, [xmax + 25.0, ymin])?;
    v.hotovo();
    v.konec_kroku();
    v.info("Vzor: hotovo – uložte výkres (QSAVE)."); // i18n:ne
    Ok(())
}
