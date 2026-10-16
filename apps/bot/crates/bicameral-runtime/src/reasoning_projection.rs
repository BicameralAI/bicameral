//! Re-exports from `bicameral_api::reasoning_projection` for runtime consumers.

pub use bicameral_api::reasoning_projection::{
    projection_request_from_external_ingest, projection_request_from_local_ingest,
    AlphaProjectionOutcome, CandidateDraftInput, CandidateHintInput,
    DeterministicProjectionProvider, ProjectedCandidateProposal, ProjectionProvider,
    ProjectionRequest, ProjectionResult, ReasoningMode, ReasoningProjectionService,
    UnavailableProjectionProvider,
};
