//! Bounded immutable transition jobs. Evaluator calls, observations, tree mutation,
//! summations and CFR remain serial. No generic Domain is sent to Rayon workers.
use super::*;
use rayon::prelude::*;
pub struct Settings<'a> {
    pub search: builder::growing::reuse::Settings,
    /// Persistent caller-owned pool, reused across searches. One thread is serial.
    pub pool: &'a rayon::ThreadPool,
}
pub(super) struct OwnedBatch<'a> {
    pub(super) pool: &'a rayon::ThreadPool,
    pub(super) ruleset: Ruleset,
    pub(super) us: SideId,
    pub(super) factored: bool,
}
impl<const N: usize, E: Evaluator<N> + ?Sized>
    builder::growing::Batch<WritingDomain<'_, N, E, true>> for OwnedBatch<'_>
{
    fn width(&self) -> usize {
        self.pool.current_num_threads().clamp(1, 4)
    }
    fn run(
        &self,
        _: &WritingDomain<'_, N, E, true>,
        p: &Position<N>,
        joints: &[[&Choice<N>; 2]],
    ) -> Vec<Result<Vec<(f64, Position<N>)>, String>> {
        // transitions_owned does not use the evaluator. Keep the caller's potentially
        // stateful evaluator on the serial thread by constructing a transition-only
        // adapter with an unused Material value inside each worker.
        self.pool.install(|| {
            joints
                .par_iter()
                .map(|a| {
                    let _mode = lab_engine::turn::FactoredScope::new(self.factored);
                    EngineDomain {
                        ruleset: self.ruleset,
                        options: EnumerateOptions::default(),
                        pruning: Pruning::All,
                        us: self.us,
                        evaluator: &lab_engine::eval::Material,
                    }
                    .transitions_owned(p, *a)
                })
                .collect()
        })
    }
}
pub fn growing<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: Settings<'_>,
) -> Result<builder::growing::ResultTree, Error> {
    let first = worlds.first().ok_or_else(|| Error("empty worlds".into()))?;
    let reference = visible(&first.position.state, us, knowledge)?;
    for w in worlds {
        if w.position.suspension.is_some() {
            return Err(Error(
                "root suspended state needs prior action memory".into(),
            ));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error("worlds disagree on declared known root state".into()));
        }
    }
    let domain = WritingDomain::<N, E, true> {
        inner: SnapshotDomain {
            inner: EngineDomain {
                ruleset,
                options: EnumerateOptions::default(),
                pruning: Pruning::All,
                us,
                evaluator,
            },
        },
        direct: settings.search.storage.direct_write,
    };
    let seeds: Vec<_> = worlds
        .iter()
        .map(|w| Seed {
            id: w.id.clone(),
            weight: w.weight,
            position: w.position.clone(),
        })
        .collect();
    let batch = OwnedBatch {
        pool: settings.pool,
        ruleset,
        us,
        factored: lab_engine::turn::factored_mode(),
    };
    builder::growing::reuse::search_batched(
        &domain,
        &seeds,
        limits,
        settings.search.growth,
        &builder::growing::Uniform,
        settings.search.storage,
        &batch,
    )
}
