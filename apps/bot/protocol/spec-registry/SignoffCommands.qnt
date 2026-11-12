// -*- mode: Bluespec; -*-
/**
 * Durable command semantics for the ADR-0034 transitions (bicameral#40).
 *
 * SignoffTransitions.qnt says which transitions are legal. This module adds
 * how they arrive and persist, the parts #40 leaves open:
 *
 *   - Command identity and idempotency: every command carries an id. A retry
 *     with the same id and the same payload gets the recorded reply and
 *     changes nothing. The same id with a different payload is refused.
 *   - Stale commands: a command carries the version of every source it
 *     touches, as the caller saw them (and the policy version for a policy
 *     change). If any moved, the command is refused, so a command never
 *     applies to state its author did not see.
 *   - Authority: the role comes from the authenticated actor (Owner or not),
 *     never from the role the caller claims. Reject, resolve and policy
 *     changes are owner-only. Manual approve needs the policy's allow bit for
 *     that role; policy approve needs auto_approve.
 *   - Policy mutation: owner-only, versioned, prospective. Each approval
 *     records the policy version it was judged under; later changes never
 *     rewrite it.
 *   - Persistence and replay: accepted commands append one event to an
 *     append-only log. Replaying the log from the initial state re-checks
 *     every guard and rebuilds the same standing, versions and policy.
 *
 * Refused commands append nothing; their reply is recorded under the id so a
 * retry gets the same answer.
 *
 * Not modelled (follow-ups for #40): daemon identity, more than one snapshot
 * per source (same-Source version update), standalone SupersedeSnapshot
 * outside a conflict set, and the event/command wire format.
 *
 * Run (from this directory):
 *   quint test SignoffCommands.qnt --main=SignoffCommands3
 *   quint run SignoffCommands.qnt --main=SignoffCommands3 --invariant=Safety \
 *     --max-steps=40 --max-samples=2000
 */
module SignoffCommands {
  import SignoffLogic.* from "./SignoffTransitions"

  // ── Constants ────────────────────────────────────────────────────────────
  const Sources: Set[str]
  const Persons: Set[str]
  const Owner: str
  /// Command ids callers may use. Small, so ids get reused and retried.
  const CmdIds: Set[int]
  /// How many prepared-but-unsent commands may exist at once.
  const MaxPending: int

  run assumptionsTest = all {
    assert(Persons.contains(Owner)),
    assert(Sources.size() >= 2 and Sources.size() <= 3),
    assert(not(Persons.contains("none"))),
    assert(MaxPending >= 2),
  }

  // ── Types ────────────────────────────────────────────────────────────────

  /// ApprovePolicy: auto_approve plus the two non-conflict allow cells. The
  /// in_conflict=true cells are invariantly false, which canApprove enforces.
  type Policy = { autoApprove: bool, ownerMayApprove: bool, memberMayApprove: bool }

  pure val Kinds = Set(
    "track", "untrack", "extract", "check", "approve", "policyApprove",
    "reject", "resolve", "setPolicy",
  )

  type Cmd = {
    id: int,
    kind: str,
    actor: str,              // authenticated person
    claimedRole: str,        // what the caller says; never used for authority
    src: str,
    other: str,              // second source, for check
    value: str,              // check result, for check
    comp: Set[str],          // for resolve
    result: str -> str,      // for resolve
    newPolicy: Policy,       // for setPolicy
    expected: str -> int,    // source versions the caller saw
    expectedPolicy: int,     // policy version the caller saw
  }

  type Event = {
    cmd: Cmd,
    role: str,               // derived from the actor at commit
    policyVersion: int,      // policy in force when the command was judged
    appliedAt: str -> int,   // source versions just before this event
  }

  type Record = { cmd: Cmd, reply: str }   // reply: "accepted" or a refusal reason

  // ── State ────────────────────────────────────────────────────────────────
  var standing: State
  var versions: str -> int
  var policyHistory: List[Policy]          // index = policy version
  var log: List[Event]
  var outcomes: int -> Record
  var pending: Set[Cmd]
  var lastCmd: Cmd
  var lastReply: str
  var lastWasDuplicate: bool  // diagnostic, not part of the recorded reply

  // ── Pure layer ───────────────────────────────────────────────────────────

  pure def roleOf(owner: str, actor: str): str = if (actor == owner) "owner" else "member"

  pure def touched(c: Cmd): Set[str] =
    if (c.kind == "check") Set(c.src, c.other)
    else if (c.kind == "resolve") c.comp
    else if (c.kind == "setPolicy") Set()
    else Set(c.src)

  pure def ownerOnly(kind: str): bool = Set("reject", "resolve", "setPolicy").contains(kind)

  pure def mayApprove(p: Policy, role: str): bool =
    if (role == "owner") p.ownerMayApprove else p.memberMayApprove

  pure def authorized(owner: str, p: Policy, c: Cmd): bool = {
    val role = roleOf(owner, c.actor)
    if (ownerOnly(c.kind)) role == "owner"
    else if (c.kind == "approve") mayApprove(p, role)
    else if (c.kind == "policyApprove") p.autoApprove
    else true
  }

  /// The SignoffTransitions guard for the command's transition.
  pure def legal(s: State, srcs: Set[str], owner: str, c: Cmd): bool =
    if (c.kind == "track") canTrack(s, c.src)
    else if (c.kind == "untrack") canUntrack(s, c.src, c.actor)
    else if (c.kind == "extract") canCompleteExtract(s, c.src)
    else if (c.kind == "check") canWriteCheck(s, c.src, c.other)
    else if (c.kind == "approve" or c.kind == "policyApprove") canApprove(s, srcs, c.src)
    else if (c.kind == "reject") canReject(s, srcs, c.src)
    else if (c.kind == "resolve") canResolve(s, srcs, owner, c.comp, c.result, c.actor)
    else true  // setPolicy has no standing guard

  /// The SignoffTransitions effect. Depends only on the command, so replay
  /// can recompute it from the log.
  pure def effect(s: State, c: Cmd): State =
    if (c.kind == "track") applyTrack(s, c.src, c.actor)
    else if (c.kind == "untrack") applyUntrack(s, c.src)
    else if (c.kind == "extract") applyCompleteExtract(s, c.src)
    else if (c.kind == "check") applyWriteCheck(s, c.src, c.other, c.value)
    else if (c.kind == "approve" or c.kind == "policyApprove") applyApprove(s, c.src)
    else if (c.kind == "reject") applyReject(s, c.src)
    else if (c.kind == "resolve") applyResolve(s, c.comp, c.result)
    else s

  pure def fresh(v: str -> int, policyVersion: int, c: Cmd): bool = and {
    touched(c).forall(x => v.get(x) == c.expected.get(x)),
    c.kind == "setPolicy" implies c.expectedPolicy == policyVersion,
  }

  pure def bump(v: str -> int, c: Cmd): str -> int =
    v.keys().mapBy(x => if (touched(c).contains(x)) v.get(x) + 1 else v.get(x))

  pure def nextPolicy(h: List[Policy], c: Cmd): List[Policy] =
    if (c.kind == "setPolicy") h.append(c.newPolicy) else h

  /// Why a new command is refused, or "" if it is accepted. Order is fixed so
  /// the recorded reply is deterministic.
  pure def refusal(
    s: State, v: str -> int, h: List[Policy], srcs: Set[str], owner: str, c: Cmd
  ): str =
    if (not(fresh(v, h.length() - 1, c))) "stale"
    else if (not(authorized(owner, h[h.length() - 1], c))) "unauthorized"
    else if (not(legal(s, srcs, owner, c))) "illegal"
    else ""

  /// Replay: fold the log from the initial state, re-checking every guard.
  type Replay = { s: State, v: str -> int, h: List[Policy], ok: bool }

  pure def replay(l: List[Event], init: Replay, srcs: Set[str], owner: str): Replay =
    l.foldl(init, (r, e) => {
      val accepted = refusal(r.s, r.v, r.h, srcs, owner, e.cmd) == ""
      {
        s: effect(r.s, e.cmd),
        v: bump(r.v, e.cmd),
        h: nextPolicy(r.h, e.cmd),
        ok: r.ok and accepted and e.policyVersion == r.h.length() - 1,
      }
    })

  // ── Actions ──────────────────────────────────────────────────────────────

  val Pairs: Set[Set[str]] = Sources.powerset().filter(p => p.size() == 2)
  val currentPolicy: Policy = policyHistory[policyHistory.length() - 1]
  val policyVersion: int = policyHistory.length() - 1

  pure val NoPolicy: Policy = { autoApprove: false, ownerMayApprove: false, memberMayApprove: false }

  val noCmd: Cmd = {
    id: -1, kind: "", actor: "", claimedRole: "", src: "", other: "", value: "",
    comp: Set(), result: Map(), newPolicy: NoPolicy, expected: Map(), expectedPolicy: -1,
  }

  action init = {
    nondet auto = Set(true, false).oneOf()
    nondet ownerOk = Set(true, false).oneOf()
    nondet memberOk = Set(true, false).oneOf()
    all {
      standing' = initState(Sources, Pairs),
      versions' = Sources.mapBy(_ => 0),
      policyHistory' = [{ autoApprove: auto, ownerMayApprove: ownerOk, memberMayApprove: memberOk }],
      log' = [],
      outcomes' = Map(),
      pending' = Set(),
      lastCmd' = noCmd,
      lastReply' = "",
      lastWasDuplicate' = false,
    }
  }

  /// A caller builds a command against the state it sees now.
  action prepare(c: Cmd): bool = all {
    pending.size() < MaxPending,
    pending' = pending.union(Set(c)),
    standing' = standing, versions' = versions, policyHistory' = policyHistory,
    log' = log, outcomes' = outcomes, lastCmd' = lastCmd, lastReply' = lastReply,
    lastWasDuplicate' = lastWasDuplicate,
  }

  /// Retry of a command id already decided, with the same payload.
  action submitDuplicate(c: Cmd): bool = all {
    outcomes.keys().contains(c.id),
    outcomes.get(c.id).cmd == c,
    lastCmd' = c,
    lastReply' = outcomes.get(c.id).reply,
    lastWasDuplicate' = true,
    standing' = standing, versions' = versions, policyHistory' = policyHistory,
    log' = log, outcomes' = outcomes, pending' = pending,
  }

  /// A command id already decided, reused with a different payload.
  action submitIdConflict(c: Cmd): bool = all {
    outcomes.keys().contains(c.id),
    outcomes.get(c.id).cmd != c,
    lastCmd' = c,
    lastReply' = "idempotency_conflict",
    lastWasDuplicate' = false,
    standing' = standing, versions' = versions, policyHistory' = policyHistory,
    log' = log, outcomes' = outcomes, pending' = pending,
  }

  /// A new command that is refused. The reply is recorded under the id.
  action submitRefused(c: Cmd): bool = {
    val reason = refusal(standing, versions, policyHistory, Sources, Owner, c)
    all {
      not(outcomes.keys().contains(c.id)),
      reason != "",
      outcomes' = outcomes.put(c.id, { cmd: c, reply: reason }),
      lastCmd' = c,
      lastReply' = reason,
      lastWasDuplicate' = false,
      standing' = standing, versions' = versions, policyHistory' = policyHistory,
      log' = log, pending' = pending,
    }
  }

  /// A new command that is accepted: one event, applied atomically.
  action submitAccepted(c: Cmd): bool = all {
    not(outcomes.keys().contains(c.id)),
    refusal(standing, versions, policyHistory, Sources, Owner, c) == "",
    standing' = effect(standing, c),
    versions' = bump(versions, c),
    policyHistory' = nextPolicy(policyHistory, c),
    log' = log.append({
      cmd: c, role: roleOf(Owner, c.actor), policyVersion: policyVersion, appliedAt: versions,
    }),
    outcomes' = outcomes.put(c.id, { cmd: c, reply: "accepted" }),
    lastCmd' = c,
    lastReply' = "accepted",
    lastWasDuplicate' = false,
    pending' = pending,
  }

  action submit: bool = all {
    pending.size() > 0,
    nondet c = pending.oneOf()
    any { submitDuplicate(c), submitIdConflict(c), submitRefused(c), submitAccepted(c) },
  }

  /// A caller sends a command straight away (no other command in between).
  action sendNow(c: Cmd): bool =
    any { submitDuplicate(c), submitIdConflict(c), submitRefused(c), submitAccepted(c) }

  /// A caller gives up on a prepared command.
  action discard: bool = all {
    pending.size() > 0,
    nondet c = pending.oneOf()
    all {
      pending' = pending.exclude(Set(c)),
      standing' = standing, versions' = versions, policyHistory' = policyHistory,
      log' = log, outcomes' = outcomes, lastCmd' = lastCmd, lastReply' = lastReply,
      lastWasDuplicate' = lastWasDuplicate,
    },
  }

  action step = {
    nondet id = CmdIds.oneOf()
    nondet kind = Kinds.oneOf()
    nondet actor = Persons.oneOf()
    nondet claimedRole = Set("owner", "member").oneOf()
    nondet src = Sources.oneOf()
    nondet other = Sources.exclude(Set(src)).oneOf()
    nondet value = Set("false", "true").oneOf()
    nondet comp = Sources.map(z => Component(standing, Sources, z)).oneOf()
    nondet result = comp.setOfMaps(Results).oneOf()
    val legalResults = comp.setOfMaps(Results).filter(r => canResolve(standing, Sources, Owner, comp, r, Owner))
    nondet honestResult = (if (legalResults.size() > 0) legalResults else comp.setOfMaps(Results)).oneOf()
    nondet auto = Set(true, false).oneOf()
    nondet ownerOk = Set(true, false).oneOf()
    nondet memberOk = Set(true, false).oneOf()
    val base: Cmd = {
      id: id, kind: kind, actor: actor, claimedRole: claimedRole,
      src: src, other: other, value: value, comp: comp, result: result,
      newPolicy: { autoApprove: auto, ownerMayApprove: ownerOk, memberMayApprove: memberOk },
      expected: versions, expectedPolicy: policyVersion,
    }
    // An honest caller mostly sends commands that would be accepted now;
    // an arbitrary caller sends anything. Both race through submit.
    val honestBase: Cmd = { ...base, result: honestResult }
    val acceptable = Kinds.filter(k => refusal(standing, versions, policyHistory, Sources, Owner, { ...honestBase, kind: k }) == "")
    nondet honestKind = (if (acceptable.size() > 0) acceptable else Kinds).oneOf()
    val honest: Cmd = { ...honestBase, kind: honestKind }
    any { sendNow(honest), prepare(honest), prepare(base), submit, discard }
  }

  // ── Invariants ───────────────────────────────────────────────────────────

  /// The SignoffTransitions safety properties hold on the durable standing.
  val StandingSafe: bool = and {
    twoApprovedAgree(standing, Sources),
    approvedRequiresReady(standing, Sources),
    supersededOnlyFromApproved(standing, Sources),
    outIsEmpty(standing, Sources),
    trackedHasTracker(standing, Sources, Persons),
  }

  /// Each command id appears at most once in the log (no double apply).
  val CommandIdsUnique: bool =
    log.indices().forall(i => log.indices().forall(j =>
      i != j implies log[i].cmd.id != log[j].cmd.id))

  /// Every event applied to exactly the source versions its author saw.
  val AppliedToObservedState: bool =
    log.foldl(true, (ok, e) => ok and touched(e.cmd).forall(x => e.appliedAt.get(x) == e.cmd.expected.get(x)))

  /// Owner-only transitions were made by the authenticated owner, whatever
  /// role the caller claimed.
  val OwnerOnlyByOwner: bool =
    log.foldl(true, (ok, e) => ok and (ownerOnly(e.cmd.kind) implies e.cmd.actor == Owner))

  /// Each approval was allowed by the policy in force when it was made. A
  /// later policy change does not make it unauthorized (prospective).
  val ApprovalsAuthorizedWhenMade: bool =
    log.foldl(true, (ok, e) => ok and and {
      e.cmd.kind == "approve" implies mayApprove(policyHistory[e.policyVersion], e.role),
      e.cmd.kind == "policyApprove" implies policyHistory[e.policyVersion].autoApprove,
    })

  /// Replaying the log from the start re-accepts every event and rebuilds
  /// the standing, the versions and the policy history exactly.
  val ReplayReconstructs: bool = {
    val r = replay(
      log,
      { s: initState(Sources, Pairs), v: Sources.mapBy(_ => 0), h: [policyHistory[0]], ok: true },
      Sources, Owner,
    )
    r.ok and r.s == standing and r.v == versions and r.h == policyHistory
  }

  /// Every recorded outcome agrees with the log: accepted ids are in the log,
  /// refused ids are not.
  val OutcomesMatchLog: bool =
    outcomes.keys().forall(id =>
      (outcomes.get(id).reply == "accepted") == log.foldl(false, (found, e) => found or e.cmd.id == id))

  val Safety: bool = and {
    StandingSafe, CommandIdsUnique, AppliedToObservedState, OwnerOnlyByOwner,
    ApprovalsAuthorizedWhenMade, ReplayReconstructs, OutcomesMatchLog,
  }

  // ── Witnesses ────────────────────────────────────────────────────────────
  val wAccepted: bool = lastReply == "accepted" and not(lastWasDuplicate)
  val wDuplicate: bool = lastReply == "accepted" and lastWasDuplicate
  val wIdConflict: bool = lastReply == "idempotency_conflict"
  val wStale: bool = lastReply == "stale"
  val wMemberClaimedOwnerRefused: bool =
    lastReply == "unauthorized" and lastCmd.claimedRole == "owner" and lastCmd.actor != Owner
  val wPolicyChanged: bool = policyHistory.length() > 1
  val wApprovedThenRevoked: bool =
    log.foldl(false, (found, e) => found or and {
      e.cmd.kind == "approve",
      not(mayApprove(currentPolicy, e.role)),
      standing.phase.get(e.cmd.src) == "approved",
    })
  val wResolved: bool = log.foldl(false, (found, e) => found or e.cmd.kind == "resolve")
  val wConflict: bool = Sources.exists(x => InConflict(standing, Sources, x))
  val wApprovedState: bool = Sources.exists(x => standing.phase.get(x) == "approved")
}

module SignoffCommands3 {
  import SignoffCommands(
    Sources = Set("s1", "s2", "s3"),
    Persons = Set("p1", "p2"),
    Owner = "p1",
    CmdIds = 1.to(30),
    MaxPending = 3,
  ).*

  val track: Cmd = {
    ...noCmd, id: 1, kind: "track", actor: "p1", src: "s1",
    expected: Sources.mapBy(_ => 0), expectedPolicy: 0,
  }

  run acceptedRetryTest = init.then(sendNow(track)).expect(wAccepted)
    .then(sendNow(track)).expect(and {
      lastReply == "accepted", lastReply == outcomes.get(1).reply,
      wDuplicate, not(wAccepted), log.length() == 1, outcomes.keys().size() == 1, Safety,
    })

  action retryRefused(c: Cmd, reason: str): bool =
    sendNow(c).expect(lastReply == reason)
      .then(sendNow(c)).expect(and {
        lastReply == reason, lastReply == outcomes.get(c.id).reply,
        not(wAccepted), not(wDuplicate), log.length() == 0, outcomes.keys().size() == 1, Safety,
      })

  run unauthorizedRetryTest = init.then(retryRefused({
    ...track, kind: "setPolicy", actor: "p2", claimedRole: "owner",
  }, "unauthorized"))

  run staleRetryTest = init.then(retryRefused({
    ...track, expected: Sources.mapBy(_ => 1),
  }, "stale"))

  run illegalRetryTest = init.then(retryRefused({ ...track, kind: "untrack" }, "illegal"))

  run changedPayloadTest = init.then(sendNow(track))
    .then(sendNow({ ...track, kind: "untrack" })).expect(and {
      wIdConflict, not(wDuplicate), outcomes.get(1).cmd == track,
      outcomes.get(1).reply == "accepted", log.length() == 1, Safety,
    })
}
