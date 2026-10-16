use bicameral_smr::{map_feedback_verdict, BoundedCheckOutcome, FeedbackVerdict};

#[test]
fn maps_the_complete_conclusive_verdict_table() {
    use BoundedCheckOutcome::{Impossible, Possible};

    let cases = [
        (Possible, Impossible, FeedbackVerdict::Entailed),
        (Impossible, Possible, FeedbackVerdict::Refuted),
        (Possible, Possible, FeedbackVerdict::Underdetermined),
        (Impossible, Impossible, FeedbackVerdict::Conflict),
    ];

    for (assertion, negated_assertion, expected) in cases {
        assert_eq!(
            map_feedback_verdict(assertion, negated_assertion),
            Ok(expected),
        );
    }
}

#[test]
fn incomplete_checks_never_produce_a_verdict() {
    use BoundedCheckOutcome::{Impossible, Possible, Unavailable, Unjustified};

    for assertion in [Possible, Impossible, Unavailable, Unjustified] {
        for negated_assertion in [Possible, Impossible, Unavailable, Unjustified] {
            if matches!(assertion, Possible | Impossible)
                && matches!(negated_assertion, Possible | Impossible)
            {
                continue;
            }

            assert!(map_feedback_verdict(assertion, negated_assertion).is_err());
        }
    }
}

#[test]
fn verdict_wire_names_match_the_governed_contract() {
    let cases = [
        (FeedbackVerdict::Entailed, "\"entailed\""),
        (FeedbackVerdict::Refuted, "\"refuted\""),
        (FeedbackVerdict::Underdetermined, "\"underdetermined\""),
        (FeedbackVerdict::Conflict, "\"conflict\""),
    ];

    for (verdict, expected) in cases {
        assert_eq!(serde_json::to_string(&verdict).unwrap(), expected);
    }
}
