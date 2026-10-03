//! Storage for the exact ordered observation / own-action sequence. Public histories
//! retain their existing Vec<String> representation and lexical admission order.
use super::{Memory, Observation};

pub(super) trait Histories: Clone + Default {
    type Path: Clone;
    type Key: AsRef<str> + Clone;
    fn root(&mut self, world: &str, observed: &Observation) -> [Self::Path; 2];
    fn advance(
        &mut self,
        previous: &[Self::Path; 2],
        actions: [&str; 2],
        observed: &Observation,
    ) -> [Self::Path; 2];
    fn keys(&mut self, memory: &[Self::Path; 2]) -> [Self::Key; 2];
}

/// Reference storage. Its allocation and formatting operations match the old builder.
#[derive(Clone, Default)]
pub(super) struct Plain;
impl Histories for Plain {
    type Path = Vec<Memory>;
    type Key = String;
    fn root(&mut self, world: &str, o: &Observation) -> [Self::Path; 2] {
        [
            vec![Memory::Observe(o.public.clone(), o.private[0].clone())],
            vec![
                Memory::Type(world.into()),
                Memory::Observe(o.public.clone(), o.private[1].clone()),
            ],
        ]
    }
    fn advance(&mut self, p: &[Self::Path; 2], a: [&str; 2], o: &Observation) -> [Self::Path; 2] {
        let mut next = p.clone();
        next[0].push(Memory::Action(a[0].into()));
        next[1].push(Memory::Action(a[1].into()));
        for (side, remembered) in next.iter_mut().enumerate() {
            remembered.push(Memory::Observe(o.public.clone(), o.private[side].clone()));
        }
        next
    }
    fn keys(&mut self, m: &[Self::Path; 2]) -> [Self::Key; 2] {
        [format!("0:{:?}", m[0]), format!("1:{:?}", m[1])]
    }
}

#[cfg(feature = "experiment-interned-history")]
pub(super) use linked::Interned;

#[cfg(feature = "experiment-interned-history")]
mod linked {
    use super::*;
    use std::collections::{hash_map::RandomState, HashMap};
    use std::hash::BuildHasher;
    use std::sync::Arc;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Event {
        Empty,
        Type(usize),
        Observe(usize, usize),
        Action(usize),
    }
    #[derive(Clone)]
    struct Link {
        previous: usize,
        event: Event,
        keys: [Option<Arc<str>>; 2],
    }
    /// IDs never leave this builder. Hashes select buckets; full key equality selects
    /// identities, including the previous link and each unmodified string atom.
    #[derive(Clone)]
    pub(in super::super) struct Interned<S = RandomState> {
        atoms: HashMap<Arc<str>, usize, S>,
        text: Vec<Arc<str>>,
        index: HashMap<(usize, Event), usize, S>,
        links: Vec<Link>,
    }
    impl<S: BuildHasher + Default> Default for Interned<S> {
        fn default() -> Self {
            Self {
                atoms: HashMap::with_hasher(S::default()),
                text: Vec::new(),
                index: HashMap::with_hasher(S::default()),
                links: vec![Link {
                    previous: 0,
                    event: Event::Empty,
                    keys: [None, None],
                }],
            }
        }
    }
    impl<S: BuildHasher + Default> Interned<S> {
        fn atom(&mut self, value: &str) -> usize {
            if let Some(&id) = self.atoms.get(value) {
                return id;
            }
            let id = self.text.len();
            let value: Arc<str> = value.into();
            self.atoms.insert(value.clone(), id);
            self.text.push(value);
            #[cfg(feature = "experiment-interned-history-observer")]
            super::observer::atom();
            id
        }
        fn append(&mut self, previous: usize, event: Event) -> usize {
            if let Some(&id) = self.index.get(&(previous, event)) {
                #[cfg(feature = "experiment-interned-history-observer")]
                super::observer::reused();
                return id;
            }
            let id = self.links.len();
            self.links.push(Link {
                previous,
                event,
                keys: [None, None],
            });
            self.index.insert((previous, event), id);
            #[cfg(feature = "experiment-interned-history-observer")]
            super::observer::linked();
            id
        }
        fn key(&mut self, player: usize, id: usize) -> Arc<str> {
            #[cfg(feature = "experiment-interned-history-observer")]
            super::observer::requested();
            if let Some(key) = &self.links[id].keys[player] {
                return key.clone();
            }
            let mut path: Vec<Memory<&str>> = Vec::new();
            let mut at = id;
            while at != 0 {
                path.push(match self.links[at].event {
                    Event::Empty => unreachable!("sentinel is not an event"),
                    Event::Type(a) => Memory::Type(&self.text[a]),
                    Event::Action(a) => Memory::Action(&self.text[a]),
                    Event::Observe(a, b) => Memory::Observe(&self.text[a], &self.text[b]),
                });
                at = self.links[at].previous;
            }
            // The same Memory enum/derived Debug defines Cursor and builder keys.
            // Borrow atoms for this one rendering; do not deep-clone the history.
            path.reverse();
            let key: Arc<str> = format!("{player}:{path:?}").into();
            self.links[id].keys[player] = Some(key.clone());
            #[cfg(feature = "experiment-interned-history-observer")]
            super::observer::formatted();
            key
        }
    }
    impl<S: BuildHasher + Default + Clone> Histories for Interned<S> {
        type Path = usize;
        type Key = Arc<str>;
        fn root(&mut self, world: &str, o: &Observation) -> [usize; 2] {
            let public = self.atom(&o.public);
            let own0 = self.atom(&o.private[0]);
            let own1 = self.atom(&o.private[1]);
            let world = self.atom(world);
            let zero = self.append(0, Event::Observe(public, own0));
            let kind = self.append(0, Event::Type(world));
            let one = self.append(kind, Event::Observe(public, own1));
            [zero, one]
        }
        fn advance(&mut self, p: &[usize; 2], a: [&str; 2], o: &Observation) -> [usize; 2] {
            let public = self.atom(&o.public);
            let mut next = *p;
            for side in 0..2 {
                let action = self.atom(a[side]);
                let own = self.atom(&o.private[side]);
                let action = self.append(p[side], Event::Action(action));
                next[side] = self.append(action, Event::Observe(public, own));
            }
            #[cfg(feature = "experiment-interned-history-observer")]
            super::observer::advanced();
            next
        }
        fn keys(&mut self, m: &[usize; 2]) -> [Arc<str>; 2] {
            [self.key(0, m[0]), self.key(1, m[1])]
        }
    }
}

#[cfg(feature = "experiment-interned-history-observer")]
pub mod observer {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub key_requests: usize,
        pub key_formats: usize,
        pub linked_events: usize,
        pub link_reuses: usize,
        pub interned_atoms: usize,
        pub advances: usize,
    }
    thread_local! { static COUNTS: Cell<Counts> = Cell::new(Counts::default()); }
    pub fn reset() {
        COUNTS.set(Counts::default());
    }
    pub fn counts() -> Counts {
        COUNTS.get()
    }
    fn update(f: impl FnOnce(&mut Counts)) {
        let mut c = COUNTS.get();
        f(&mut c);
        COUNTS.set(c);
    }
    pub(super) fn atom() {
        update(|c| c.interned_atoms += 1);
    }
    pub(super) fn reused() {
        update(|c| c.link_reuses += 1);
    }
    pub(super) fn linked() {
        update(|c| c.linked_events += 1);
    }
    pub(super) fn requested() {
        update(|c| c.key_requests += 1);
    }
    pub(super) fn formatted() {
        update(|c| c.key_formats += 1);
    }
    pub(super) fn advanced() {
        update(|c| c.advances += 1);
    }
}

#[cfg(all(test, feature = "experiment-interned-history"))]
mod tests;
