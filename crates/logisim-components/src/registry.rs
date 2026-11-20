//! Library registry: every component library exposes `pub fn library() -> Library`.
//! Add a library with ONE line in `lib.rs`'s `libraries!` list.

use logisim_core::Component;

/// A placeable part with default options.
pub struct PartDesc {
    pub name: &'static str,
    pub make: fn() -> Box<dyn Component>,
}

pub struct Library {
    pub name: &'static str,
    pub parts: Vec<PartDesc>,
}

/// Declares the library modules and builds [`all_libraries`].
#[macro_export]
macro_rules! libraries {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        /// Every registered library, in menu order.
        pub fn all_libraries() -> Vec<$crate::registry::Library> {
            vec![$($m::library()),*]
        }
    };
}
