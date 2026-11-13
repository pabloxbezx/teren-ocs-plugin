//! Ukázka okna Prospektoru bez Open CAD Studia – vymyšlená data, akce se
//! nikam neposílají. Jen pro vývoj (`cargo run --example ukazka_okna
//! --features ukazka`), do pluginu se nesestavuje.

use std::collections::HashMap;

use ocs_plugin_api::host::Handle;
use teren::body::Bod;
use teren::skupiny::SkupinaBodu;

use crate::cogo::BodVykresu;
use crate::povrchy::{DruhPolozky, Polozka, StatUlozena};
use crate::spravce::{self, PrehledPovrchu, PrehledSkupiny, PodezrelyBod, SkupinaPovrchu, StavSpravce};
use crate::styly::{self, StylBodu, StylPovrchu};

fn data() -> StavSpravce {
    let kody = ["TER", "TER", "TER", "HR", "STR", "TER", "KER", "OB", "TER", "ZD", "SL", "TER"];
    let mut seed: u64 = 7;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / (1u64 << 31) as f64
    };
    let mut body = Vec::new();
    for i in 0..240u64 {
        let k = kody[(rnd() * kody.len() as f64) as usize % kody.len()];
        body.push(BodVykresu {
            handle: Handle::new(0x1000 + i),
            bod: Bod {
                cislo: (1001 + i).to_string(),
                x: -742_310.0 - rnd() * 180.0,
                y: -1_041_220.0 - rnd() * 140.0,
                z: 311.4 + rnd() * 4.7,
                popis: k.to_string(),
            },
            zdroj: if i < 200 { "zamereni_2026-10.txt".into() } else { "doplneni.csv".into() },
            styl: String::new(),
            posun: None,
        });
    }
    let styly_bodu = StylBodu::vestavene();
    let styly_povrchu = StylPovrchu::vestavene();
    let mut styl_bodu = HashMap::new();
    for b in &body {
        let (id, puvod) = match b.bod.popis.as_str() {
            "STR" => (styly_bodu.iter().find(|s| s.je_vestaveny() && s.jmeno.contains("listnat")).map(|s| s.id.clone()), styly::Puvod::Kod("STR".into())),
            "KER" => (styly_bodu.iter().find(|s| s.jmeno.contains("Keř")).map(|s| s.id.clone()), styly::Puvod::Kod("KER".into())),
            _ => (None, styly::Puvod::Skupina("zam".into())),
        };
        styl_bodu.insert(b.handle, (id.unwrap_or_else(|| styly::STANDARD.into()), puvod));
    }
    let zam = SkupinaBodu { id: "zam".into(), jmeno: "Zaměření".into(), kody: vec!["TER".into(), "HR".into(), "OB".into()], ..Default::default() };
    let str_ = SkupinaBodu { id: "str".into(), jmeno: "Stromy".into(), kody: vec!["STR".into(), "KER".into()], ..Default::default() };
    let skupiny = vec![
        PrehledSkupiny { skupina: SkupinaBodu::vsechny(), pocet: 240, povrchy: vec![] },
        PrehledSkupiny { skupina: zam, pocet: 168, povrchy: vec!["Stávající terén".into(), "Návrh – parkoviště".into()] },
        PrehledSkupiny { skupina: str_, pocet: 41, povrchy: vec![] },
    ];
    let polozka = |h: u64, druh: DruhPolozky, n: usize, d: f64| Polozka {
        handle: Handle::new(h),
        druh,
        vrcholu: n,
        delka: d,
        stred: [-742_400.0, -1_041_290.0],
        rozmer: 20.0,
        zapnuto: true,
        hrany: false,
    };
    let sk = |zap| vec![SkupinaPovrchu { id: "zam".into(), jmeno: "Zaměření".into(), zapnuto: zap, pocet: 168, chybi: false }];
    let povrchy = vec![
        PrehledPovrchu {
            id: "st".into(),
            jmeno: "Stávající terén".into(),
            aktivni: true,
            zastaraly: false,
            auto: true,
            styl: styly_povrchu[0].id.clone(),
            interval: 0.5,
            hlavni: 2.5,
            max_hrana: None,
            skupiny: sk(true),
            vyloucene_kody: "STR".into(),
            stat: StatUlozena { trojuhelniku: 2840, z_min: 311.42, z_max: 316.08, plocha_2d: 18240.0, plocha_3d: 18391.0, bodu: 168 },
            pocet_bodu: 168,
            vylouceno_bodu: 41,
            polozky: vec![polozka(0x2001, DruhPolozky::Hranice, 42, 286.4), polozka(0x2002, DruhPolozky::Spojnice, 7, 48.2)],
            upravy: vec![],
            hlaseni: None,
        },
        PrehledPovrchu {
            id: "np".into(),
            jmeno: "Návrh – parkoviště".into(),
            aktivni: false,
            zastaraly: true,
            auto: false,
            styl: styly_povrchu.get(2).map_or(String::new(), |s| s.id.clone()),
            interval: 0.25,
            hlavni: 1.0,
            max_hrana: Some(30.0),
            skupiny: sk(true),
            vyloucene_kody: "STR, KER".into(),
            stat: StatUlozena { trojuhelniku: 786, z_min: 312.10, z_max: 314.85, plocha_2d: 4812.0, plocha_3d: 4860.0, bodu: 120 },
            pocet_bodu: 120,
            vylouceno_bodu: 48,
            polozky: (0..14).map(|i| polozka(0x3000 + i, DruhPolozky::Spojnice, 4 + i as usize, 12.0 + i as f64 * 3.1)).collect(),
            upravy: vec![],
            hlaseni: None,
        },
    ];
    StavSpravce {
        kontrola: Some(
            [(16usize, 0.82), (32, -2.64)]
                .iter()
                .map(|&(i, dz)| PodezrelyBod { handle: body[i].handle, cislo: body[i].bod.cislo.clone(), x: body[i].bod.x, y: body[i].bod.y, z: body[i].bod.z, dz })
                .collect(),
        ),
        zprava: "Terén: povrch Stávající terén přepočítán (2 840 trojúhelníků)".into(),
        vykres: "C:\\Zakázky\\ukazka_parcela.dwg".into(),
        slozka: "C:\\Zakázky".into(),
        pripojeno: true,
        dalsi_cislo: 1241,
        zdroje: vec!["zamereni_2026-10.txt".into(), "doplneni.csv".into()],
        meritko: 500.0,
        vyber_body: vec![Handle::new(0x1003), Handle::new(0x1004)],
        body,
        skupiny,
        povrchy,
        styly_bodu,
        styly_povrchu,
        styl_bodu,
        verze: 1,
        ..Default::default()
    }
}

/// Otevře okno Prospektoru s ukázkovými daty (blokuje do zavření).
pub fn spust() {
    spravce::ukazka(data());
    crate::okno::spust_ukazku();
}
