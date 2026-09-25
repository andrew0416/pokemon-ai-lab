//! What the turn engine can and cannot simulate, per dex entry. This is the
//! machine-readable side of the support gate ([`super::support`]): the same checks the gate
//! runs, applied to every move, ability and item, so a coverage report never drifts from
//! what the engine actually refuses.

use crate::dex::{AbilityId, ItemId, MoveId};

use super::support::{ability_supported_on_field, item_supported_on_field, move_unsupported};
use super::switching::{item_start_handler, start_effect, switch_in_supported, StartEffect};

/// How far one dex entry is supported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    /// Fully usable in a turn.
    Supported,
    /// Usable while the holder is on the field, but it acts on switch-in in a way that is not
    /// implemented (so it can only start a scenario already on the field).
    NoSwitchIn { handlers: &'static [&'static str] },
    /// Refused; the reason names the Showdown callback list or mechanic.
    Unsupported { reason: String },
}

impl Support {
    pub fn is_supported(&self) -> bool {
        matches!(self, Support::Supported)
    }
}

pub fn move_support(id: MoveId) -> Support {
    match move_unsupported(id) {
        None => Support::Supported,
        Some(reason) => Support::Unsupported { reason },
    }
}

pub fn ability_support(id: AbilityId) -> Support {
    let data = id.data();
    // Trace never stays on the field: its switch-in (`switching::START_HANDLERS`) replaces it
    // with the copied ability, so its `onStart` / `onUpdate` are implemented there (a Trace with
    // nothing to copy is refused when it happens, and `check_state` refuses one left seeking).
    if matches!(start_effect(id), Some(StartEffect::Trace)) {
        return Support::Supported;
    }
    if !ability_supported_on_field(id) {
        return Support::Unsupported {
            reason: format!("callbacks {:?} are not implemented", data.handlers),
        };
    }
    if !switch_in_supported(id) {
        return Support::NoSwitchIn {
            handlers: data.handlers,
        };
    }
    Support::Supported
}

pub fn item_support(id: ItemId) -> Support {
    let data = id.data();
    if !item_supported_on_field(id) {
        return Support::Unsupported {
            reason: format!("callbacks {:?} are not implemented", data.handlers),
        };
    }
    if item_start_handler(id).is_some() {
        return Support::NoSwitchIn {
            handlers: data.handlers,
        };
    }
    Support::Supported
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::abilities;

    /// Trace is classified by its switch-in, which replaces it (not by the on-field table, where
    /// a Trace still seeking is refused).
    #[test]
    fn trace_is_supported_through_its_switch_in() {
        assert!(!ability_supported_on_field(abilities::TRACE));
        assert_eq!(ability_support(abilities::TRACE), Support::Supported);
    }
}
