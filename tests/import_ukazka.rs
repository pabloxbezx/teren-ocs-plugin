//! Celý průchod importu na vymyšleném zaměření (stejná data jako v náhledu
//! návrhu): kódování, rozpoznání, chybné řádky, podezřelé body, duplicity.

use teren::body::{nacti_body, Bod, Desetinna, Oddelovac};
use teren::duplicity::{naplanuj, najdi, Politika};
use teren::kodovani::{self, Kodovani};
use teren::rozpoznani::{odlehle, podezrela_vyska, rozpoznej};

const UKAZKA: &str = "Číslo;Y;X;Z;Kód
4001;745210,12;1043520,45;312,48;T terén
4002;745215,80;1043518,02;312,61;T terén
4003;745221,33;1043516,90;312,75;OB obrubník
4004;745226,05;1043515,11;312,80;OB obrubník
4005;745230,72;1043512,64;312,92;ŠA šachta
4006;745224,40;1043530,18;312,55;T terén
4007;745218,91;1043533,70;312,40;T terén
4008;745212,47;1043536,25;312,22;STR strom
4009;745207,30;1043529,88;312,31;T terén
4010;745232,15;1043526,40;313,05;HR hrana
4011;745238,60;1043522,95;313,18;HR hrana
4012;745243,02;1043519,33;313,30;T terén
4005;745241,77;1043531,06;313,12;T terén
4013;7452a8,10;1043538,44;312,97;T terén
4014;745229,90;1043541,20;;T terén
4015;745222,64;1043544,85;3125,50;T terén
4016;745216,03;1043546,70;312,18;PL plot
4017;745209,58;1043543,12;312,06;PL plot
4018;745204,11;1043537,95;312,10;T terén
4019;745201,76;1043524,60;312,25;T terén
4020;745203,40;1043517,72;312,36;T terén
4021;74521,40;1043512,30;312,44;T terén
4022;745246,85;1043527,90;313,41;T terén
";

#[test]
fn ukazka_cely_pruchod() {
    let bajty = teren::koduj_cp1250(UKAZKA);
    let (text, k) = kodovani::nacti(&bajty);
    assert_eq!(k, Kodovani::Cp1250);
    assert_eq!(text, UKAZKA);

    let kand = &rozpoznej(&text)[0];
    assert_eq!((kand.format.as_str(), kand.oddelovac, kand.hlavicka), ("PYXZD", Oddelovac::Strednik, 1));
    assert_eq!(kand.desetinna, Desetinna::Carka);

    let v = nacti_body(&text, &kand.format_bodu());
    assert_eq!(v.body.len(), 21);
    let chybne: Vec<(usize, Option<usize>)> = v.chyby.iter().map(|c| (c.radek, c.sloupec)).collect();
    assert_eq!(chybne, vec![(15, Some(1)), (16, Some(3))]);

    // podezřelé: překlep ve výšce (4015) a chybějící číslice v Y (4021)
    let vysky: Vec<&str> = v.body.iter().filter(|b| podezrela_vyska(b.z)).map(|b| b.cislo.as_str()).collect();
    assert_eq!(vysky, vec!["4015"]);
    let daleko: Vec<&str> = odlehle(&v.body).into_iter().map(|i| v.body[i].cislo.as_str()).collect();
    assert_eq!(daleko, vec!["4021"]);

    // duplicity: 4005 dvakrát v souboru, 4010 už je ve výkresu
    let vykres = vec![Bod { cislo: "4010".into(), x: -745230.40, y: -1043522.70, z: 313.0, popis: "HR".into() }];
    let d = najdi(&v.body, &vykres);
    assert_eq!(d.v_souboru.len(), 1);
    assert_eq!(v.body[d.v_souboru[0]].cislo, "4005");
    assert_eq!(d.ve_vykresu.len(), 1);
    assert_eq!(naplanuj(&v.body, &vykres, Politika::Preskocit).pridat.len(), 19);
    let p = naplanuj(&v.body, &vykres, Politika::Prepsat);
    assert_eq!((p.pridat.len(), p.prepsat.len()), (19, 1));
    assert_eq!(naplanuj(&v.body, &vykres, Politika::Ponechat).pridat.len(), 21);
}
