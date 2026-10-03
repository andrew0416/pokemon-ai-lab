//! Preserve the exact Cursor key while formatting only newly appended events.
//! Public histories keep their existing ordering and interning. No domain callback is
//! cached, and the two players' private memories are always extended independently.
use super::{Memory, Observation};
#[cfg(feature = "experiment-growth-pipeline")]
use std::fmt::Write;

#[derive(Clone)]
pub(super) enum History {
    Raw([Vec<Memory>; 2]),
    #[cfg(feature = "experiment-growth-pipeline")]
    Encoded([String; 2]),
}
impl History {
    pub(super) fn raw(memory: [Vec<Memory>; 2]) -> Self {
        Self::Raw(memory)
    }
    #[cfg(feature = "experiment-growth-pipeline")]
    pub(super) fn new(memory: [Vec<Memory>; 2], encoded: bool) -> Self {
        let raw = Self::raw(memory);
        if encoded {
            Self::Encoded(raw.keys())
        } else {
            raw
        }
    }
    pub(super) fn keys(&self) -> [String; 2] {
        match self {
            Self::Raw(m) => [format!("0:{:?}", m[0]), format!("1:{:?}", m[1])],
            #[cfg(feature = "experiment-growth-pipeline")]
            Self::Encoded(keys) => keys.clone(),
        }
    }
    pub(super) fn advance(&self, own_actions: [&str; 2], obs: &Observation) -> Self {
        match self {
            Self::Raw(parent) => {
                let mut memory = parent.clone();
                for (side, m) in memory.iter_mut().enumerate() {
                    m.push(Memory::Action(own_actions[side].into()));
                    m.push(Memory::Observe(
                        obs.public.clone(),
                        obs.private[side].clone(),
                    ));
                }
                Self::Raw(memory)
            }
            #[cfg(feature = "experiment-growth-pipeline")]
            Self::Encoded(parent) => Self::Encoded(std::array::from_fn(|side| {
                let p = &parent[side];
                // Roots always contain Observe. Every extension appends two tokens;
                // the trailing ASCII ] is produced internally, never supplied by input.
                debug_assert!(p.ends_with(']'));
                let mut key = String::with_capacity(
                    p.len()
                        + own_actions[side].len()
                        + obs.public.len()
                        + obs.private[side].len()
                        + 32,
                );
                key.push_str(&p[..p.len() - 1]);
                // Borrowed and owned Memory share one derived Debug representation,
                // including Rust's escaping of quotes, control bytes and Unicode.
                write!(
                    key,
                    ", {:?}, {:?}]",
                    Memory::Action(own_actions[side]),
                    Memory::Observe(obs.public.as_str(), obs.private[side].as_str())
                )
                .expect("writing history to String cannot fail");
                key
            })),
        }
    }
}

#[cfg(all(test, feature = "experiment-growth-pipeline"))]
mod tests {
    use super::*;
    use crate::bayesian::tree::builder::Cursor;
    #[test]
    fn every_prefix_matches_cursor_with_escaping_and_independent_branches() {
        let labels = [
            "",
            "plain",
            "a\"b\\c\n\r\t\0",
            "💧메가진화\u{301}\u{2028}",
            "Action(\"fake\"), Observe(\"x\", \"y\")]",
        ];
        for shift in 0..labels.len() {
            let observation = |step: usize| Observation {
                public: labels[(step + shift) % labels.len()].repeat(1 + step % 3),
                private: [
                    format!("left-{}", labels[step % labels.len()]),
                    format!("right-{}", labels[(step + 1) % labels.len()]),
                ],
            };
            let initial = observation(0);
            let mut cursors = [
                Cursor::new(0, None, &initial).unwrap(),
                Cursor::new(1, Some(labels[shift]), &initial).unwrap(),
            ];
            let mut history =
                History::new(std::array::from_fn(|s| cursors[s].memory.clone()), true);
            for step in 0..128 {
                assert_eq!(history.keys(), std::array::from_fn(|s| cursors[s].key()));
                let saved = history.keys();
                let obs = observation(step);
                let actions = [
                    labels[step % labels.len()],
                    labels[(step + 2) % labels.len()],
                ];
                let child = history.advance(actions, &obs);
                let _sibling = history.advance(["other", "unseen"], &observation(step + 1));
                assert_eq!(history.keys(), saved);
                for side in 0..2 {
                    cursors[side].advance(actions[side], &obs);
                }
                assert_eq!(child.keys(), std::array::from_fn(|s| cursors[s].key()));
                history = child;
            }
        }
    }
}
