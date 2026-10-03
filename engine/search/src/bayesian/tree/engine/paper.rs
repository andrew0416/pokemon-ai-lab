//! Explicit changed-algorithm API; cadence=1 preserves the original selection schedule.
use super::*;
pub struct Settings<'a> {
    pub search: builder::growing::reuse::Settings,
    pub cadence: usize,
    pub solver: crate::bayesian::tree::paper::Settings,
    pub pool: Option<&'a rayon::ThreadPool>,
}
pub fn growing<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: Settings<'_>,
) -> Result<builder::growing::reuse::paper::PaperResult, Error> {
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
    let batch = settings
        .pool
        .filter(|p| p.current_num_threads() > 1)
        .map(|pool| parallel::OwnedBatch {
            pool,
            ruleset,
            us,
            factored: lab_engine::turn::factored_mode(),
        });
    builder::growing::reuse::paper::with_batch(
        &domain,
        &seeds,
        limits,
        settings.search.growth,
        &builder::growing::Uniform,
        settings.search.storage,
        settings.cadence,
        settings.solver,
        batch
            .as_ref()
            .map(|b| b as &dyn builder::growing::Batch<WritingDomain<'_, N, E, true>>),
    )
}
