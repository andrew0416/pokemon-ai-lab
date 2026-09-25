use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Number of active slots per side for each supported format.
#[pyfunction]
fn slots(format: &str) -> PyResult<usize> {
    match format {
        "singles" => Ok(lab_engine::Singles::SLOTS),
        "doubles" => Ok(lab_engine::Doubles::SLOTS),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "unknown format: {other}"
        ))),
    }
}

#[pymodule]
fn lab_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(slots, m)?)?;
    Ok(())
}
