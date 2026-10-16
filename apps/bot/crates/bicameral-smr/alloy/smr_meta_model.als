module smr_meta_model

abstract sig SemanticElement {}

abstract sig OccurrenceKind {}
one sig ActorKind, RoleKind, ActionKind, TargetKind, StateKind,
  ConditionKind, ValueKind extends OccurrenceKind {}

abstract sig BindingKind {}
one sig ExactRef, ProposedIdentity extends BindingKind {}

abstract sig BindingIdentity {
  bindingKind: one BindingKind
}

abstract sig Modality {}
one sig Asserted, Permitted, Required, Prohibited, Conditional extends Modality {}

abstract sig Polarity {}
one sig Positive, Negative extends Polarity {}

abstract sig Quantifier {}
one sig All, Some, One, Lone, NoneQ extends Quantifier {}

abstract sig RelationKind {}
one sig Precedes, RequiresRel, Enables, CausesTransition, ConditionalOn,
  AlternativeTo, Excludes, EquivalentTo, DistinctFrom extends RelationKind {}

abstract sig ModelAtom extends SemanticElement {
  occurrenceKind: one OccurrenceKind,
  bindingIdentity: one BindingIdentity
}

abstract sig ClaimAssertion extends SemanticElement {
  actorNode: lone ModelAtom,
  actionNode: one ModelAtom,
  targetNode: lone ModelAtom,
  preStateNode: lone ModelAtom,
  postStateNode: lone ModelAtom,
  guardNodes: set ModelAtom,
  effectNodes: set ModelAtom,
  exceptionAssertions: set ClaimAssertion,
  modality: one Modality,
  polarity: one Polarity,
  quantifier: one Quantifier
}

abstract sig AtomRelation extends SemanticElement {
  fromElement: one SemanticElement,
  toElement: one SemanticElement,
  relationKind: one RelationKind
}
