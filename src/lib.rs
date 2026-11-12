//! Výpočetní jádro pro terén: COGO body, TIN povrch, vrstevnice.
//!
//! Jádro nezávisí na Open CAD Studiu – plugin bude jen tenká vrstva nad ním.

pub mod body;
pub mod duplicity;
pub mod dxf;
pub mod formaty;
pub mod kodovani;
pub mod kontrola;
pub mod landxml;
pub mod preklad;
pub mod povrch;
pub mod rozpoznani;
pub mod skupiny;
pub mod vrstevnice;
pub mod z_vykresu;

/// Dekóduje text ve Windows-1250 (běžné u exportů z CAD a geodetických programů).
pub fn dekoduj_cp1250(bajty: &[u8]) -> String {
    encoding_rs::WINDOWS_1250.decode(bajty).0.into_owned()
}

/// Zakóduje text do Windows-1250 (znaky mimo kódovou stránku se nahradí).
pub fn koduj_cp1250(text: &str) -> Vec<u8> {
    encoding_rs::WINDOWS_1250.encode(text).0.into_owned()
}
