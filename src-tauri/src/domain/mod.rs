// Domain event types + projections (AD-1): each domain module owns its event
// payload schemas, exposed as typed constructors on `NewEvent` (AD-15), and
// the pure folds that read current state from the log.

pub mod bridge;
pub mod chat;
pub mod checkpoints;
pub mod digest;
pub mod evidence;
pub mod export;
pub mod nightshift;
pub mod hypotheses;
pub mod jobs;
pub mod journals;
pub mod library;
pub mod manuscript;
pub mod missions;
pub mod onboarding;
pub mod proposals;
pub mod readiness;
pub mod receipts;
pub mod search;
pub mod skills;
pub mod spend;
pub mod submissions;
pub mod support;
pub mod telemetry;
pub mod trust;
pub mod verifier;
