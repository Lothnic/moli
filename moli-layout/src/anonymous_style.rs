use crate::ResolvedLayoutStyle;

/// Renderer-assigned provenance of a projected style. Pass identities must
/// remain unique while any of their styles can still be used as parents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutStyleOrigin {
    pass: u64,
    owner: u64,
}

impl LayoutStyleOrigin {
    pub const fn new(pass: u64, owner: u64) -> Self {
        Self { pass, owner }
    }
}

/// Inheritance input for an anonymous box in the resolver's prepared pass.
/// The owner supplies provenance and diagnostics; it does not select a style
/// world or request another element-style observation.
#[derive(Clone, Copy, Debug)]
pub struct LayoutAnonymousStyleContext<'a, N> {
    pub owner: N,
    pub parent_style: &'a ResolvedLayoutStyle,
}

impl<'a, N> LayoutAnonymousStyleContext<'a, N> {
    pub const fn new(owner: N, parent_style: &'a ResolvedLayoutStyle) -> Self {
        Self {
            owner,
            parent_style,
        }
    }
}
