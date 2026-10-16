/// Struct symbol for verified struct-binding tests.
///
/// Evidence states produced by binding this type are graph readiness
/// observations, not compliance verdicts.
pub struct OrderId(pub u64);

impl OrderId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}
