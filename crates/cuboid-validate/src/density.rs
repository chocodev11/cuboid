//! Mixed texel density: faces whose texels per unit differ from the model median by more than a
//! factor.

use cuboid_model::mc;

use crate::codes::UV_DENSITY;
use crate::ctx::{Ctx, Exported};
use crate::elements::Density;
use crate::issue::{Issue, Severity};

pub(crate) fn run(ctx: &mut Ctx, elements: &[Exported<'_>], faces: &[Density], ratio: f32) {
    if !ctx.on(UV_DENSITY) || faces.len() < 2 || ratio.is_nan() || ratio < 1.0 {
        return;
    }
    let mut values: Vec<f32> = faces.iter().map(|face| face.texels_per_unit).collect();
    let middle = values.len() / 2;
    let (_, &mut median, _) = values.select_nth_unstable_by(middle, f32::total_cmp);
    if median <= 0.0 || !median.is_finite() {
        return;
    }
    // Faces come in element order, so one run of the list is one element.
    for run in faces.chunk_by(|a, b| a.elem == b.elem) {
        let worst = run
            .iter()
            .map(|face| (face, face.texels_per_unit / median))
            .filter(|&(_, factor)| factor > ratio || factor < 1.0 / ratio)
            .max_by(|a, b| a.1.max(1.0 / a.1).total_cmp(&b.1.max(1.0 / b.1)));
        let Some((face, factor)) = worst else {
            continue;
        };
        let Some(ex) = elements.get(face.elem as usize) else {
            continue;
        };
        ctx.emit(UV_DENSITY, || {
            Issue::new(
                UV_DENSITY,
                Severity::Warning,
                vec![ex.id],
                format!(
                    "{}: the {} face has {:.2} texels per unit, {:.2} times the model median of {:.2}",
                    ex.name(),
                    mc::face_key(face.face),
                    face.texels_per_unit,
                    factor,
                    median
                ),
                "Resize the face rectangle so that every face shows about the same texels per unit",
            )
        });
    }
}
