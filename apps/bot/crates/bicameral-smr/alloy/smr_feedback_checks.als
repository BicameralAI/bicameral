module smr_feedback_checks

open smr_feedback_meta_model

pred FeedbackCompatibility {
  no accepted: AcceptedAssertion |
    accepted.subject = ProposedAssertion.subject
    and accepted.truth != ProposedAssertion.truth
}
