//! Výpočetní jádro pluginu Trasy: směrové řešení trasy, staničení, styly.
//! Nezávisí na Open CAD Studiu – všechno se tu dá testovat `cargo test`.

pub mod geometrie;
pub mod niveleta;
pub mod pasy;
pub mod pohled;
pub mod lang {
    //! Jazyk rozhraní – dvojice `tr(cs, en)`, viz crate `teren_lang`.
    pub use teren_lang::*;
}
pub use teren_lang::trf;
pub mod profil;
pub mod stanice;
pub mod styl;
pub mod trasa;
pub mod upravy;
pub mod vzhled;
