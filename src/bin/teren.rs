//! Příkazová řádka pro ověření jádra:
//! načte body, vytvoří TIN, vrstevnice a zapíše DXF.

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;
use teren::body::{nacti_body, FiltrKodu, FormatBodu, Oddelovac};
use teren::dxf::{Dxf, NastaveniDxf};
use teren::povrch::{NastaveniPovrchu, Povrch};
use teren::vrstevnice::{vrstevnice, NastaveniVrstevnic};

#[derive(Parser)]
#[command(name = "teren", version, about = "Terén: import bodů → TIN → vrstevnice → DXF")]
struct Argumenty {
    /// Soubor s body (TXT/CSV).
    vstup: PathBuf,
    /// Výstupní DXF (výchozí: stejné jméno jako vstup, přípona .dxf).
    #[arg(short, long)]
    vystup: Option<PathBuf>,
    /// Formát sloupců: P číslo, N/E souřadnice výkresu, Y/X S-JTSK kladné, Z výška, D popis, - přeskočit.
    #[arg(short, long, default_value = "PNEZD")]
    format: String,
    /// Oddělovač: mezera, carka, strednik, tab.
    #[arg(short, long, default_value = "mezera")]
    oddelovac: String,
    /// Počet řádků hlavičky k přeskočení.
    #[arg(long, default_value_t = 0)]
    hlavicka: usize,
    /// Do povrchu jen body s těmito kódy (odděleno čárkou).
    #[arg(long, value_delimiter = ',')]
    kody: Vec<String>,
    /// Z povrchu vyloučit body s těmito kódy (odděleno čárkou).
    #[arg(long, value_delimiter = ',')]
    vyloucit: Vec<String>,
    /// Maximální délka hrany trojúhelníku [m].
    #[arg(long)]
    max_hrana: Option<f64>,
    /// Interval vedlejších vrstevnic [m].
    #[arg(long, default_value_t = 0.5)]
    interval: f64,
    /// Interval hlavních vrstevnic [m].
    #[arg(long, default_value_t = 2.5)]
    hlavni: f64,
    /// Výška textu popisu bodů [m].
    #[arg(long, default_value_t = 0.5)]
    vyska_textu: f64,
    /// Nevykreslovat popisy bodů.
    #[arg(long)]
    bez_popisu: bool,
    /// Nevykreslovat trojúhelníky.
    #[arg(long)]
    bez_tin: bool,
}

fn main() -> ExitCode {
    match beh(Argumenty::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("CHYBA: {e}");
            ExitCode::FAILURE
        }
    }
}

fn beh(a: Argumenty) -> Result<(), String> {
    let oddelovac = Oddelovac::z_nazvu(&a.oddelovac).ok_or(format!("neznámý oddělovač '{}'", a.oddelovac))?;
    let mut format = FormatBodu::z_retezce(&a.format, oddelovac)?;
    format.preskocit_radku = a.hlavicka;

    let bajty = std::fs::read(&a.vstup).map_err(|e| format!("nelze číst {}: {e}", a.vstup.display()))?;
    // UTF-8, jinak Windows-1250 (běžné u exportů z CAD a geodetických programů)
    let text = match String::from_utf8(bajty) {
        Ok(t) => t.trim_start_matches('\u{feff}').to_string(),
        Err(e) => encoding_rs::WINDOWS_1250.decode(e.as_bytes()).0.into_owned(),
    };

    let import = nacti_body(&text, &format);
    println!("Načteno bodů: {}", import.body.len());
    if !import.chyby.is_empty() {
        println!("Nenačtené řádky: {}", import.chyby.len());
        for ch in import.chyby.iter().take(10) {
            println!("  {ch}");
        }
        if import.chyby.len() > 10 {
            println!("  … a dalších {}", import.chyby.len() - 10);
        }
    }

    let filtr = FiltrKodu { zahrnout: a.kody, vyloucit: a.vyloucit };
    let vyber: Vec<usize> = (0..import.body.len()).filter(|&i| filtr.propusti(&import.body[i])).collect();
    println!("Bodů do povrchu: {}", vyber.len());

    let povrch = Povrch::z_bodu(
        &import.body,
        &vyber,
        &NastaveniPovrchu { max_hrana: a.max_hrana, ..Default::default() },
    )
    .map_err(|e| e.to_string())?;

    if !povrch.duplicity.is_empty() {
        println!("Shodné body (vynechány): {}", povrch.duplicity.len());
        for d in povrch.duplicity.iter().take(10) {
            println!(
                "  bod {} = bod {} (rozdíl výšky {:+.3} m)",
                import.body[d.vynechany].cislo, import.body[d.ponechany].cislo, d.rozdil_z
            );
        }
    }
    if povrch.odstraneno_dlouhych > 0 {
        println!("Odstraněno trojúhelníků s dlouhou hranou: {}", povrch.odstraneno_dlouhych);
    }
    let s = povrch.statistika();
    println!("Trojúhelníků: {}", s.pocet_trojuhelniku);
    println!("Výška: {:.2} – {:.2} m n. m.", s.z_min, s.z_max);
    println!("Plocha 2D: {:.1} m², 3D: {:.1} m²", s.plocha_2d, s.plocha_3d);

    let vrst = vrstevnice(&povrch, &NastaveniVrstevnic { interval: a.interval, hlavni: a.hlavni });
    println!(
        "Vrstevnic: {} (hlavních {})",
        vrst.len(),
        vrst.iter().filter(|v| v.hlavni).count()
    );

    let mut dxf = Dxf::default();
    let vse: Vec<usize> = (0..import.body.len()).collect();
    dxf.pridej_body(&import.body, &vse, &NastaveniDxf { vyska_textu: a.vyska_textu, popisy_bodu: !a.bez_popisu });
    if !a.bez_tin {
        dxf.pridej_tin(&povrch);
    }
    dxf.pridej_vrstevnice(&vrst);

    let vystup = a.vystup.unwrap_or_else(|| a.vstup.with_extension("dxf"));
    std::fs::write(&vystup, dxf.do_bajtu()).map_err(|e| format!("nelze zapsat {}: {e}", vystup.display()))?;
    println!("Zapsáno: {}", vystup.display());
    Ok(())
}
