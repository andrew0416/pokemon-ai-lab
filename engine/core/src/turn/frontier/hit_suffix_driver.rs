//! Same group/key algorithm as run_group, with an inner retained-suffix replay loop.
//! Generic P remains unconstrained: only concrete run_stage can opt into the frame.
use super::*;
use crate::turn::hit_suffix;

pub(super) fn run<const N: usize, P: Clone + Eq + Hash>(
    group: Group<N, P>,
    next: &mut Positions<N, P>,
    finished: &mut Positions<N, Option<P>>,
    options: EnumerateOptions,
    stage: &mut impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
    buffers: &mut RunBuffers,
    stats: &mut Stats,
    first_hit_policy: crate::turn::first_hit::Policy,
) -> Result<Vec<Group<N, P>>, TurnError> {
    let Group {
        state: mut work,
        pending,
        weight,
        lazy,
    } = group;
    let spans: Vec<(usize, i16)> = lazy
        .iter()
        .map(|(u, d)| (usize::from(*u), d.span()))
        .collect();
    for &(unit, _) in &lazy {
        work.pokemon_mut(unit_ref(unit)).lazy = lazy::tag(usize::from(unit));
    }
    lazy::begin(&spans);
    let _guard = GroupGuard;
    #[cfg(test)]
    let input = work.clone();
    let mut chooser = Chooser::with_rolls(options.rolls);
    let mut start: Option<RunStart> = None;
    let mut after = pending.clone();
    let mut reached: Vec<(bool, u32, Component)> = Vec::new();
    let mut masked: Vec<(PokemonRef, i16, LazyTag)> = Vec::new();
    let mut units: Vec<u8> = Vec::new();
    loop {
        chooser.begin_run();
        after.clone_from(&pending);
        let mut owned = std::mem::take(buffers);
        let mut b = match &start {
            Some(start) => Battle::replay(&mut work, &mut chooser, start, owned),
            None => {
                owned.log.clear();
                Battle::recycle(&mut work, &mut chooser, owned)
            }
        };
        if start.is_none() {
            start = Some(b.run_start());
        }
        b.first_hit_policy = first_hit_policy;
        b.hit_suffix_allowed = true;
        let exhausted = loop {
            stats.runs += 1;
            // Preserve original precedence: an error wins even if lazy already requested work.
            let end = match stage(&mut b, &mut after) {
                Ok(end) => end,
                Err(error) => {
                    hit_suffix::error_rollback();
                    b.state.reverse(&b.log);
                    for &(unit, _) in &lazy {
                        b.state.pokemon_mut(unit_ref(unit)).lazy = lazy::tag(usize::from(unit));
                    }
                    #[cfg(test)]
                    {
                        assert_eq!(*b.state, input, "error full rollback");
                        assert_eq!(format!("{:?}", b.state), format!("{input:?}"));
                    }
                    clear_tags(b.state, &lazy);
                    *buffers = b.into_buffers();
                    return Err(error);
                }
            };
            if let Some((unit, request)) = lazy::take_request() {
                if b.hit_suffix_frame.is_some() {
                    hit_suffix::lazy_discarded(reached.len());
                }
                b.state.reverse(&b.log);
                clear_tags(b.state, &lazy);
                match request {
                    Request::Split { .. } => stats.splits += 1,
                    Request::Expand => stats.expansions += 1,
                }
                *buffers = b.into_buffers(); // drops the frame; reached has not been committed
                return Ok(split(work, pending, weight, lazy, unit as u8, request));
            }
            let p = weight * b.rng.probability();
            record_run(
                b.state,
                &after,
                end,
                p,
                &lazy,
                next,
                finished,
                &mut reached,
                &mut masked,
                &mut units,
            );
            // Consume leaf weight and key before advancing, exactly as the original DFS.
            let advanced = b.rng.advance();
            if advanced {
                if let Some(saved) = b.hit_suffix_frame.take() {
                    if b.rng.matches_checkpoint(&saved.chooser) {
                        b.rollback_hit_suffix(&saved.battle);
                        b.hit_suffix_frame = Some(saved);
                        b.rng.begin_run();
                        continue;
                    }
                    hit_suffix::invalidated();
                }
            }
            b.state.reverse(&b.log);
            for &(unit, _) in &lazy {
                b.state.pokemon_mut(unit_ref(unit)).lazy = lazy::tag(usize::from(unit));
            }
            #[cfg(test)]
            {
                assert_eq!(*b.state, input, "origin full rollback");
                assert_eq!(format!("{:?}", b.state), format!("{input:?}"));
            }
            break !advanced;
        };
        *buffers = b.into_buffers();
        if exhausted {
            break;
        }
    }
    for (done, id, component) in reached {
        if done {
            finished.entries[id as usize].components.push(component);
        } else {
            next.entries[id as usize].components.push(component);
        }
    }
    Ok(Vec::new())
}
