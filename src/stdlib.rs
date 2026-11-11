//! The bundled library: `include "std/NAME.fitd"` splices one of these files.

/// MODULES holds every library module, as (name, source).
pub const MODULES: &[(&str, &str)] = &[
    ("nn", include_str!("std/nn.fitd")),
    ("flow", include_str!("std/flow.fitd")),
    ("palette", include_str!("std/palette.fitd")),
    ("grid", include_str!("std/grid.fitd")),
    ("arch", include_str!("std/arch.fitd")),
    ("uml", include_str!("std/uml.fitd")),
    ("er", include_str!("std/er.fitd")),
    ("tree", include_str!("std/tree.fitd")),
    ("time", include_str!("std/time.fitd")),
    ("model", include_str!("std/model.fitd")),
    ("seq", include_str!("std/seq.fitd")),
    ("venn", include_str!("std/venn.fitd")),
    ("algo", include_str!("std/algo.fitd")),
];

/// get returns the source of a `std/...` include target.
pub fn get(target: &str) -> Option<&'static str> {
    let name = target.strip_prefix("std/")?;
    let name = name.strip_suffix(".fitd").unwrap_or(name);
    MODULES.iter().find(|(n, _)| *n == name).map(|&(_, t)| t)
}

/// names lists the module names, for help text and diagnostics.
pub fn names() -> Vec<&'static str> {
    MODULES.iter().map(|&(n, _)| n).collect()
}
