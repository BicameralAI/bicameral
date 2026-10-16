module smr_checks

open smr_meta_model

pred SnapshotInstance {}

pred sameUndirectedPair[left, right: AtomRelation] {
  (left.fromElement = right.fromElement and left.toElement = right.toElement)
  or (left.fromElement = right.toElement and left.toElement = right.fromElement)
}

assert NoSemanticIdentityConflict {
  no equivalent, distinct: AtomRelation |
    equivalent.relationKind = EquivalentTo
    and distinct.relationKind = DistinctFrom
    and sameUndirectedPair[equivalent, distinct]
}
