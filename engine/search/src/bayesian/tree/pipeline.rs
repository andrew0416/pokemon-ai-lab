//! Independent opt-in growth experiments; every flag defaults to false.
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub frontier_index: bool,
    pub owned_compiler: bool,
    pub incremental_sequence: bool,
    pub compressed_checks: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub owned_compiles: usize,
    pub sequence_full_builds: usize,
    pub sequence_delta_updates: usize,
    pub sequence_leaf_visits: usize,
    pub sequence_scale_refreshes: usize,
    pub sequence_fallbacks: usize,
    pub compressed_checks: usize,
    pub compressed_rejections: usize,
}
pub(crate) use super::workspace::cached::paper::pipeline::{solve_from, Context};
pub fn validate(settings: Settings, solver: super::paper::Settings) -> Result<(), super::Error> {
    if settings.incremental_sequence
        && (!settings.owned_compiler || !solver.sequence || solver.compact)
    {
        return Err(super::Error(
            "incremental sequence requires owned compiler, sequence=true, compact=false".into(),
        ));
    }
    if settings.compressed_checks && (!solver.sequence || solver.compact) {
        return Err(super::Error(
            "compressed checks require sequence=true, compact=false".into(),
        ));
    }
    Ok(())
}

pub struct Run {
    pub run: super::paper::Run,
    pub metrics: Metrics,
}
pub fn solve(
    tree: &super::Tree,
    config: super::Config,
    solver: super::paper::Settings,
    settings: Settings,
) -> Result<Run, super::Error> {
    let mut metrics = Metrics::default();
    let run = solve_from(
        tree,
        config,
        solver,
        None,
        settings,
        None,
        &mut Context::default(),
        &mut metrics,
    )?;
    Ok(Run { run, metrics })
}
