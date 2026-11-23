// -*- mode: Bluespec; -*-
/**
 * ADR-0034 snapshot transitions. Translated from SignoffTransitions.tla
 * (removed in the same change); names, actions and invariants match the TLA+
 * one to one so the mapping stays obvious.
 *
 * One current snapshot per source. A new track clears checks that named the
 * previous snapshot. A CheckRecord is immutable. A conflict edge counts only
 * while both sources are tracked or approved.
 *
 * Command identity, idempotency, stale commands, policy mutation and replay
 * are in SignoffCommands.qnt (#40), which builds on this module.
 *
 * Run (from this directory):
 *   quint test SignoffTransitions.qnt --main=SignoffTransitions3
 *   quint run SignoffTransitions.qnt --main=SignoffTransitions3 --init=Init --step=Next \
 *     --invariant=Inv --max-steps=15 --max-samples=5000
 */
/// Types and pure operators shared by SignoffTransitions and SignoffCommands.
/// No constants or state: every operator takes what it needs as arguments.
module SignoffLogic {
  /// The five variables as one value, for the pure layer.
  type State = {
    phase: str -> str,
    tracker: str -> str,
    ready: str -> bool,
    check: Set[str] -> str,
    superseded: str -> bool,
  }

  pure val Phases = Set("out", "tracked", "approved", "rejected", "superseded")
  pure val Results = Set("approved", "rejected", "superseded")
  // ── Pure layer (TLA+ operators) ──────────────────────────────────────────

  pure def pair(a: str, b: str): Set[str] = Set(a, b)

  pure def Active(s: State, x: str): bool = Set("tracked", "approved").contains(s.phase.get(x))

  pure def Edge(s: State, a: str, b: str): bool = and {
    a != b,
    Active(s, a), Active(s, b),
    s.check.get(pair(a, b)) == "true",
  }

  pure def Grow(s: State, srcs: Set[str], S: Set[str]): Set[str] =
    S.union(srcs.filter(t => S.exists(x => Edge(s, x, t))))

  /// Three steps cover a graph of at most three sources.
  pure def Component(s: State, srcs: Set[str], x: str): Set[str] =
    Grow(s, srcs, Grow(s, srcs, Grow(s, srcs, Set(x))))

  pure def InConflict(s: State, srcs: Set[str], x: str): bool =
    srcs.exists(t => Edge(s, x, t))

  pure def OtherApprovedCovered(s: State, srcs: Set[str], x: str): bool =
    srcs.forall(t => (t != x and s.phase.get(t) == "approved") implies s.check.get(pair(x, t)) == "false")

  pure def ClearChecks(s: State, x: str): Set[str] -> str =
    s.check.keys().mapBy(p => if (p.contains(x)) "missing" else s.check.get(p))

  // Guards and effects. Each TLA+ action is split into can<Action> (the
  // enabling conjuncts) and apply<Action> (the primed conjuncts).

  pure def canTrack(s: State, x: str): bool = s.phase.get(x) == "out"
  pure def applyTrack(s: State, x: str, person: str): State = { ...s,
    phase: s.phase.set(x, "tracked"),
    tracker: s.tracker.set(x, person),
    ready: s.ready.set(x, false),
    check: ClearChecks(s, x),
  }

  pure def canUntrack(s: State, x: str, person: str): bool =
    s.phase.get(x) == "tracked" and person == s.tracker.get(x)
  pure def applyUntrack(s: State, x: str): State = { ...s,
    phase: s.phase.set(x, "out"),
    tracker: s.tracker.set(x, "none"),
    ready: s.ready.set(x, false),
  }

  pure def canCompleteExtract(s: State, x: str): bool =
    s.phase.get(x) == "tracked" and not(s.ready.get(x))
  pure def applyCompleteExtract(s: State, x: str): State = { ...s, ready: s.ready.set(x, true) }

  pure def canWriteCheck(s: State, a: str, b: str): bool = and {
    a != b,
    s.ready.get(a), s.ready.get(b),
    s.check.get(pair(a, b)) == "missing",
  }
  pure def applyWriteCheck(s: State, a: str, b: str, value: str): State =
    { ...s, check: s.check.set(pair(a, b), value) }

  /// Approve and PolicyApprove share their enabling condition in the TLA+.
  pure def canApprove(s: State, srcs: Set[str], x: str): bool = and {
    s.phase.get(x) == "tracked",
    s.ready.get(x),
    not(InConflict(s, srcs, x)),
    OtherApprovedCovered(s, srcs, x),
  }
  pure def applyApprove(s: State, x: str): State = { ...s, phase: s.phase.set(x, "approved") }

  pure def canReject(s: State, srcs: Set[str], x: str): bool =
    s.phase.get(x) == "tracked" and not(InConflict(s, srcs, x))
  pure def applyReject(s: State, x: str): State = { ...s, phase: s.phase.set(x, "rejected") }

  /// The owner and the daemon are arguments of the command. They are not a
  /// field of result. result assigns the whole connected component.
  pure def canResolve(
    s: State, srcs: Set[str], owner: str, comp: Set[str], result: str -> str, person: str
  ): bool = and {
    person == owner,
    comp.size() >= 2,
    comp.exists(x => comp == Component(s, srcs, x)),
    comp.forall(x => InConflict(s, srcs, x)),
    result.keys() == comp,
    comp.forall(x => Results.contains(result.get(x))),
    comp.filter(x => result.get(x) == "approved").size() <= 1,
    comp.forall(x => or {
      result.get(x) == "rejected" and s.phase.get(x) == "tracked",
      result.get(x) == "superseded" and s.phase.get(x) == "approved",
      and {
        result.get(x) == "approved",
        Set("tracked", "approved").contains(s.phase.get(x)),
        s.phase.get(x) == "tracked" implies s.ready.get(x),
      },
    }),
    comp.exists(x => result.get(x) == "superseded") implies
      comp.filter(x => result.get(x) == "approved").size() == 1,
    {
      val approvedAfter =
        comp.filter(x => result.get(x) == "approved")
          .union(srcs.exclude(comp).filter(x => s.phase.get(x) == "approved"))
      tuples(approvedAfter, approvedAfter).forall(((a, b)) =>
        a != b implies s.check.get(pair(a, b)) == "false")
    },
  }
  pure def applyResolve(s: State, comp: Set[str], result: str -> str): State = { ...s,
    phase: s.phase.keys().mapBy(x => if (comp.contains(x)) result.get(x) else s.phase.get(x)),
    superseded: s.superseded.keys().mapBy(x =>
      s.superseded.get(x) or (comp.contains(x) and result.get(x) == "superseded")),
  }

  /// No tracked or untracked source remains. Further steps are not required.
  pure def Quiet(s: State, srcs: Set[str]): bool =
    srcs.forall(x => Set("approved", "rejected", "superseded").contains(s.phase.get(x)))

  pure def initState(srcs: Set[str], pairs: Set[Set[str]]): State = {
    phase: srcs.mapBy(_ => "out"),
    tracker: srcs.mapBy(_ => "none"),
    ready: srcs.mapBy(_ => false),
    check: pairs.mapBy(_ => "missing"),
    superseded: srcs.mapBy(_ => false),
  }


  // Invariant bodies, as pure operators so SignoffCommands can check the same
  // properties on its own state. SignoffTransitions' vals call these.

  pure def twoApprovedAgree(s: State, srcs: Set[str]): bool =
    tuples(srcs, srcs).forall(((a, b)) =>
      (a != b and s.phase.get(a) == "approved" and s.phase.get(b) == "approved") implies
        s.check.get(pair(a, b)) == "false")

  pure def approvedRequiresReady(s: State, srcs: Set[str]): bool =
    srcs.forall(x => s.phase.get(x) == "approved" implies s.ready.get(x))

  pure def supersededOnlyFromApproved(s: State, srcs: Set[str]): bool =
    srcs.forall(x => s.phase.get(x) == "superseded" implies s.superseded.get(x))

  pure def outIsEmpty(s: State, srcs: Set[str]): bool =
    srcs.forall(x => s.phase.get(x) == "out" implies (s.tracker.get(x) == "none" and not(s.ready.get(x))))

  pure def trackedHasTracker(s: State, srcs: Set[str], persons: Set[str]): bool =
    srcs.forall(x => s.phase.get(x) == "tracked" implies persons.contains(s.tracker.get(x)))
}

module SignoffTransitions {
  import SignoffLogic.*

  // ── CONSTANTS ────────────────────────────────────────────────────────────
  const Sources: Set[str]
  const Persons: Set[str]
  const Owner: str
  const Daemons: Set[str]

  // ASSUME Owner \in Persons
  // ASSUME Cardinality(Sources) \in 2..3
  // ("none" is the TLA+ tracker sentinel, so it must not be a person.)
  run assumptionsTest = all {
    assert(Persons.contains(Owner)),
    assert(Sources.size() >= 2 and Sources.size() <= 3),
    assert(not(Persons.contains("none"))),
  }

  // ── VARIABLES ────────────────────────────────────────────────────────────
  var phase: str -> str
  var tracker: str -> str
  var ready: str -> bool
  var check: Set[str] -> str
  var superseded: str -> bool

  val st: State = { phase: phase, tracker: tracker, ready: ready, check: check, superseded: superseded }

  val Pairs: Set[Set[str]] = Sources.powerset().filter(p => p.size() == 2)

  // ── Actions ──────────────────────────────────────────────────────────────

  action setState(s: State): bool = all {
    phase' = s.phase,
    tracker' = s.tracker,
    ready' = s.ready,
    check' = s.check,
    superseded' = s.superseded,
  }

  action Init = setState(initState(Sources, Pairs))

  // `daemon \in Daemons` is enforced by the nondet choice in Next.
  action Track(x: str, person: str, daemon: str): bool = all {
    canTrack(st, x), setState(applyTrack(st, x, person)),
  }
  action Untrack(x: str, person: str, daemon: str): bool = all {
    canUntrack(st, x, person), setState(applyUntrack(st, x)),
  }
  action CompleteExtract(x: str, daemon: str): bool = all {
    canCompleteExtract(st, x), setState(applyCompleteExtract(st, x)),
  }
  action WriteCheck(a: str, b: str, value: str, daemon: str): bool = all {
    canWriteCheck(st, a, b), setState(applyWriteCheck(st, a, b, value)),
  }
  action Approve(x: str, person: str, daemon: str): bool = all {
    canApprove(st, Sources, x), setState(applyApprove(st, x)),
  }
  action PolicyApprove(x: str, daemon: str): bool = all {
    canApprove(st, Sources, x), setState(applyApprove(st, x)),
  }
  action Reject(x: str, daemon: str): bool = all {
    canReject(st, Sources, x), setState(applyReject(st, x)),
  }
  action Resolve(comp: Set[str], result: str -> str, person: str, daemon: str): bool = all {
    canResolve(st, Sources, Owner, comp, result, person), setState(applyResolve(st, comp, result)),
  }
  action QuietStutter: bool = all { Quiet(st, Sources), setState(st) }

  /// TLA+ `\E comp \in SUBSET Sources, result \in [comp -> Results]`.
  /// Sampling draws comp from the components (the guard requires
  /// `comp = Component(s)` for some s, so nothing reachable is lost) and
  /// result from the functions on that component.
  action Next = {
    nondet x = Sources.oneOf()
    nondet y = Sources.oneOf()
    nondet person = Persons.oneOf()
    nondet daemon = Daemons.oneOf()
    nondet value = Set("false", "true").oneOf()
    nondet comp = Sources.map(z => Component(st, Sources, z)).oneOf()
    nondet result = comp.setOfMaps(Results).oneOf()
    any {
      Track(x, person, daemon),
      Untrack(x, person, daemon),
      CompleteExtract(x, daemon),
      WriteCheck(x, y, value, daemon),
      Approve(x, person, daemon),
      PolicyApprove(x, daemon),
      Reject(x, daemon),
      Resolve(comp, result, person, daemon),
      QuietStutter,
    }
  }

  // ── Invariants (same names and order as the TLA+ .cfg) ───────────────────

  val TypeOK: bool = and {
    phase.keys() == Sources and Sources.forall(x => Phases.contains(phase.get(x))),
    tracker.keys() == Sources and Sources.forall(x => Persons.union(Set("none")).contains(tracker.get(x))),
    ready.keys() == Sources,
    check.keys() == Pairs and Pairs.forall(p => Set("missing", "false", "true").contains(check.get(p))),
    superseded.keys() == Sources,
  }

  val TwoApprovedAgree: bool = twoApprovedAgree(st, Sources)
  val ApprovedRequiresReady: bool = approvedRequiresReady(st, Sources)
  val SupersededOnlyFromApproved: bool = supersededOnlyFromApproved(st, Sources)
  val OutIsEmpty: bool = outIsEmpty(st, Sources)
  val TrackedHasTracker: bool = trackedHasTracker(st, Sources, Persons)

  val Inv: bool = and {
    TypeOK, TwoApprovedAgree, ApprovedRequiresReady,
    SupersededOnlyFromApproved, OutIsEmpty, TrackedHasTracker,
  }

  // ── Witnesses (reachability; not in the TLA+) ────────────────────────────
  val wApproved: bool = Sources.exists(x => phase.get(x) == "approved")
  val wRejected: bool = Sources.exists(x => phase.get(x) == "rejected")
  val wSuperseded: bool = Sources.exists(x => phase.get(x) == "superseded")
  val wConflict: bool = Sources.exists(x => InConflict(st, Sources, x))
  val wTwoApproved: bool = Sources.filter(x => phase.get(x) == "approved").size() >= 2
}

/// Instance matching SignoffTransitions.cfg.
module SignoffTransitions3 {
  import SignoffTransitions(
    Sources = Set("s1", "s2", "s3"),
    Persons = Set("p1", "p2"),
    Owner = "p1",
    Daemons = Set("d1"),
  ).*
}
