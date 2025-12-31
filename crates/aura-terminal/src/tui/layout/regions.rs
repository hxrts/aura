//! Phantom types for type-level region enforcement.
//!
//! These types enable compile-time verification that content is placed
//! in the correct region of the TUI layout.

use std::marker::PhantomData;

/// Phantom types for type-level region enforcement
pub mod region {
    /// Navigation bar region (top NAV_HEIGHT rows)
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Nav;

    /// Middle content region (screen-specific, MIDDLE_HEIGHT rows)
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Middle;

    /// Footer region (bottom FOOTER_HEIGHT rows)
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Footer;
}

pub use region::*;

/// Marker trait for valid layout regions
pub trait LayoutRegion: Clone + Copy + Default {
    /// Human-readable name for the region
    const NAME: &'static str;

    /// Height of this region in rows
    const HEIGHT: u16;

    /// Width of this region in columns
    const WIDTH: u16;
}

impl LayoutRegion for Nav {
    const NAME: &'static str = "nav";
    const HEIGHT: u16 = super::dim::NAV_HEIGHT;
    const WIDTH: u16 = super::dim::TOTAL_WIDTH;
}

impl LayoutRegion for Middle {
    const NAME: &'static str = "middle";
    const HEIGHT: u16 = super::dim::MIDDLE_HEIGHT;
    const WIDTH: u16 = super::dim::TOTAL_WIDTH;
}

impl LayoutRegion for Footer {
    const NAME: &'static str = "footer";
    const HEIGHT: u16 = super::dim::FOOTER_HEIGHT;
    const WIDTH: u16 = super::dim::TOTAL_WIDTH;
}

/// A region marker that can be used in generic contexts
#[derive(Clone, Copy, Debug, Default)]
pub struct RegionMarker<R: LayoutRegion> {
    _marker: PhantomData<R>,
}

impl<R: LayoutRegion> RegionMarker<R> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub const fn height(&self) -> u16 {
        R::HEIGHT
    }

    #[must_use]
    pub const fn width(&self) -> u16 {
        R::WIDTH
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        R::NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_dimensions() {
        let nav = RegionMarker::<Nav>::new();
        assert_eq!(nav.height(), super::super::dim::NAV_HEIGHT);
        assert_eq!(nav.width(), super::super::dim::TOTAL_WIDTH);

        let middle = RegionMarker::<Middle>::new();
        assert_eq!(middle.height(), super::super::dim::MIDDLE_HEIGHT);
        assert_eq!(middle.width(), super::super::dim::TOTAL_WIDTH);

        let footer = RegionMarker::<Footer>::new();
        assert_eq!(footer.height(), super::super::dim::FOOTER_HEIGHT);
        assert_eq!(footer.width(), super::super::dim::TOTAL_WIDTH);
    }

    #[test]
    fn test_region_names() {
        assert_eq!(Nav::NAME, "nav");
        assert_eq!(Middle::NAME, "middle");
        assert_eq!(Footer::NAME, "footer");
    }
}
