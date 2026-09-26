//! Drags (`dragIn` → `switchIn(..., isDrag)`) run no `BeforeSwitchOut` / `eachEvent('Update')`
//! before the dragged Pokémon leaves. Fixture from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Red Card drags out a Dragonite whose last Outrage just confused it (fatigue, in AfterMove);
/// no Update runs before the drag, so its Persim Berry stays uneaten on the bench.
#[test]
fn red_card_drag_skips_the_update_matches_showdown_exactly() {
    assert_exact_parity("red-card-drag-update");
}
