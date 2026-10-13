//! Corsair keyboards this extension knows. Anything else from Corsair that has a
//! keyboard interface still gets a page, marked as not supported yet.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    pub name: &'static str,
    /// USB product id of the keyboard itself.
    pub keyboard: u16,
    /// A built-in Stream Deck, which is a separate USB device.
    pub stream_deck: Option<u16>,
    /// USB interface that speaks Corsair's Bragi protocol.
    pub bragi_interface: u8,
    pub keys: u16,
}

pub const CORSAIR: u16 = 0x1B1C;

pub const MODELS: &[Model] = &[Model {
    name: "Corsair Galleon 100 SD",
    keyboard: 0x2B0C,
    stream_deck: Some(0x2B18),
    bragi_interface: 1,
    keys: 99,
}];

pub fn by_keyboard(product: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.keyboard == product)
}

/// A product id that is a known Stream Deck part of some keyboard (not a keyboard page).
pub fn is_stream_deck(product: u16) -> bool {
    MODELS.iter().any(|m| m.stream_deck == Some(product))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_galleon() {
        let g = by_keyboard(0x2B0C).unwrap();
        assert_eq!(g.stream_deck, Some(0x2B18));
        assert_eq!(g.bragi_interface, 1);
        assert!(is_stream_deck(0x2B18) && !is_stream_deck(0x2B0C));
        assert!(by_keyboard(0x1234).is_none());
    }
}
