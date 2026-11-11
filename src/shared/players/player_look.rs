//! wash-fps `shared/players/PlayerLook.ts`: how a free-for-all player looks
//! to everyone, a body and a colour handed out by the server and sent in
//! each player's snapshot record as one byte, the model index in the high
//! four bits and the colour index in the low four.
//!
//! `PlayerLooks`, which hands the looks out, runs on the server and is not
//! ported.

/// `LOOK_MODELS`: the wire's order; append, never reorder.
pub const LOOK_MODELS: [&str; 5] = ["police", "female", "cartel", "riotkitty", "robot"];
/// `LOOK_COLORS`: the wire's order; append, never reorder.
pub const LOOK_COLORS: [&str; 8] = ["pink", "cyan", "lime", "orange", "yellow", "red", "violet", "white"];

/// A decoded look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub model: &'static str,
    pub color: &'static str,
}

/// `encodeLook`.
#[must_use]
pub const fn encode_look(model: u8, color: u8) -> u8 {
    ((model & 0x0f) << 4) | (color & 0x0f)
}

/// `decodeLook`: `None` for an index this build does not know.
#[must_use]
pub fn decode_look(look: u8) -> Option<Look> {
    Some(Look {
        model: LOOK_MODELS.get(usize::from(look >> 4)).copied()?,
        color: LOOK_COLORS.get(usize::from(look & 0x0f)).copied()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_a_model_and_a_colour_into_one_byte() {
        // PlayerLook.test.ts.
        let look = encode_look(4, 2);
        assert_eq!(
            decode_look(look),
            Some(Look {
                model: "robot",
                color: "lime"
            })
        );
        assert_eq!(decode_look(encode_look(15, 0)), None);
        assert_eq!(decode_look(encode_look(0, 9)), None);
    }
}
