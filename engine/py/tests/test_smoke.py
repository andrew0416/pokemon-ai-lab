"""The installed wheel imports and answers its basic queries (run with pytest against the
wheel installed in the venv, not the source tree)."""

import lab_engine


def test_version_and_slots():
    assert lab_engine.version() == "0.1.0"
    assert lab_engine.slots("doubles") == 2
    assert lab_engine.slots("singles") == 1
