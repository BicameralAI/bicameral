module smr_feedback_meta_model

abstract sig AtomKind {}
one sig ActionKind, EventKind, InvariantKind, StateKind, TermKind,
  TransitionKind extends AtomKind {}

abstract sig TruthValue {}
one sig TrueValue, FalseValue extends TruthValue {}

abstract sig RelationKind {}
one sig Precedes, Guards, Triggers, Produces, PartOf extends RelationKind {}

abstract sig ModelAtom {
  atomKind: one AtomKind
}

abstract sig AcceptedAssertion {
  subject: one ModelAtom,
  truth: one TruthValue
}

abstract sig AtomRelation {
  fromAtom: one ModelAtom,
  toAtom: one ModelAtom,
  relationKind: one RelationKind
}

one sig ProposedAssertion {
  subject: one ModelAtom,
  truth: one TruthValue
}
