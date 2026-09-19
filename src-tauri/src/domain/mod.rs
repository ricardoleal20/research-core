// Domain event types + projections (AD-1): each domain module owns its event
// payload schemas, exposed as typed constructors on `NewEvent` (AD-15), and
// the pure folds that read current state from the log.

pub mod evidence;
pub mod hypotheses;
pub mod missions;
pub mod spend;
