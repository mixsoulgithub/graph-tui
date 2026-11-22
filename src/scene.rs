//! Nominal canvas size: the one-shot ruler used to project metre coordinates into cells when
//! importing `.dot`. Native documents have no notion of a canvas object -- coordinates are cells.

/// Nominal canvas of the layout (cells). Keep some slack: a 200x50 window then shows the whole
/// picture "zoomed" to fill rather than squeezed in -- squeezing rounds two nodes onto adjacent
/// cells
pub const NOMINAL: (f64, f64) = (184.0, 45.0);

/// Canvas spec: scaled by the square root of the node count so "cells per node" stays close to
/// the small-graph baseline. Without scaling, post-processing cannot find free space in dense
/// regions and nodes pile up
pub fn nominal_for(n: usize) -> (f64, f64) {
    let k = (n as f64 / 262.0).sqrt().max(1.0);
    (NOMINAL.0 * k, NOMINAL.1 * k)
}
