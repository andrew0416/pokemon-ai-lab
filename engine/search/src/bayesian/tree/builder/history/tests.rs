use super::super::Cursor;
use super::*;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
struct Collision;
impl Hasher for Collision {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _: &[u8]) {}
}

#[test]
fn linked_histories_match_cursor_escaping_depth_and_real_hash_collisions() {
    let mut storage = Interned::<BuildHasherDefault<Collision>>::default();
    let values = [
        "",
        "a",
        "a, b",
        "[Observe(\"x\", \"y\")]",
        "\0\n\t\\\"",
        "한글🦀é",
        "a\u{2028}b",
    ];
    for (n, world) in values.iter().enumerate() {
        let mut obs = Observation {
            public: values[n].into(),
            private: [values[(n + 1) % values.len()].into(), world.to_string()],
        };
        let mut memory = storage.root(world, &obs);
        let mut cursors = [
            Cursor::new(0, None, &obs).unwrap(),
            Cursor::new(1, Some(world), &obs).unwrap(),
        ];
        // At the builder's maximum decision depth, every old event is still present.
        for depth in 0..=128 {
            let keys = storage.keys(&memory);
            for side in 0..2 {
                assert_eq!(keys[side].as_ref(), cursors[side].key());
            }
            let actions = [
                values[(depth + n) % values.len()],
                values[(depth + n + 1) % values.len()],
            ];
            obs.public = values[(depth + 2) % values.len()].into();
            obs.private.swap(0, 1);
            let next = storage.advance(&memory, actions, &obs);
            assert_eq!(next, storage.advance(&memory, actions, &obs));
            for side in 0..2 {
                cursors[side].advance(actions[side], &obs);
            }
            memory = next;
        }
    }
}

#[test]
fn private_types_and_opponent_actions_never_enter_the_other_players_memory() {
    let mut storage: Interned = Interned::default();
    let mut o = Observation {
        public: "same public".into(),
        private: ["our secret".into(), "their secret".into()],
    };
    let a = storage.root("world-A", &o);
    o.private[1] = "another opponent secret".into();
    let b = storage.root("world-B", &o);
    assert_eq!(a[0], b[0]);
    assert_ne!(a[1], b[1]);
    let a = storage.advance(&a, ["same own action", "hidden commitment A"], &o);
    let b = storage.advance(&b, ["same own action", "hidden commitment B"], &o);
    assert_eq!(a[0], b[0]);
    assert_ne!(a[1], b[1]);
    let keys = storage.keys(&a);
    for hidden in ["world-A", "their secret", "hidden commitment A"] {
        assert!(!keys[0].contains(hidden));
    }
}

#[test]
fn equal_suffixes_do_not_erase_earlier_observations_or_own_actions() {
    let mut storage: Interned = Interned::default();
    let mut o = Observation {
        public: "earlier A".into(),
        private: [String::new(), String::new()],
    };
    let a = storage.root("world", &o);
    o.public = "earlier B".into();
    let b = storage.root("world", &o);
    o.public = "same current observation".into();
    let a = storage.advance(&a, ["same", "same"], &o);
    let b = storage.advance(&b, ["same", "same"], &o);
    assert_ne!(storage.keys(&a), storage.keys(&b));
    let c = storage.advance(&a, ["different own action", "same"], &o);
    let d = storage.advance(&a, ["same own action", "same"], &o);
    let c = storage.advance(&c, ["same suffix", "same"], &o);
    let d = storage.advance(&d, ["same suffix", "same"], &o);
    assert_ne!(c[0], d[0]);
    assert_eq!(c[1], d[1]);
    // Forking a builder preserves its IDs and cached keys without sharing mutable links.
    let key = storage.keys(&c);
    let mut fork = storage.clone();
    fork.advance(&c, ["fork only", "fork only"], &o);
    assert_eq!(key, storage.keys(&c));
    assert_eq!(key, fork.keys(&c));
}

#[cfg(feature = "experiment-interned-history-observer")]
#[test]
fn repeated_histories_reuse_links_and_only_format_each_distinct_key_once() {
    let mut storage: Interned = Interned::default();
    let o = Observation {
        public: "public".repeat(30),
        private: ["private 0".repeat(20), "private 1".repeat(20)],
    };
    observer::reset();
    let root = storage.root("kind", &o);
    for _ in 0..64 {
        let child = storage.advance(&root, ["own", "their"], &o);
        storage.keys(&child);
        storage.keys(&child);
    }
    let counts = observer::counts();
    assert_eq!(counts.advances, 64);
    assert_eq!(counts.key_requests, 256);
    assert_eq!(counts.key_formats, 2);
    assert!(counts.link_reuses >= 252);
    eprintln!("S26E_HISTORY_STORAGE {counts:?}");
}
