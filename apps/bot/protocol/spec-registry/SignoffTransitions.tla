------------------------------ MODULE SignoffTransitions ------------------------------
\* Checker for the ADR-0034 snapshot transitions. Not the payload contract.
\* One current snapshot per source. A new track clears checks that named the
\* previous snapshot. A CheckRecord is immutable. A conflict edge counts only
\* while both sources are tracked or approved.
\* TLC 2.19, Sources = {s1, s2, s3}, 2026-09-29: 11577 distinct states,
\* 86123 generated, depth 13, no invariant failure.

EXTENDS FiniteSets, Integers, TLC

CONSTANTS Sources, Persons, Owner, Daemons

ASSUME Owner \in Persons
ASSUME Cardinality(Sources) \in 2..3

VARIABLES phase, tracker, ready, check, superseded

vars == <<phase, tracker, ready, check, superseded>>

Phases == {"out", "tracked", "approved", "rejected", "superseded"}
Results == {"approved", "rejected", "superseded"}
Pairs == { p \in SUBSET Sources : Cardinality(p) = 2 }

TypeOK ==
    /\ phase \in [Sources -> Phases]
    /\ tracker \in [Sources -> Persons \cup {"none"}]
    /\ ready \in [Sources -> BOOLEAN]
    /\ check \in [Pairs -> {"missing", "false", "true"}]
    /\ superseded \in [Sources -> BOOLEAN]

Init ==
    /\ phase = [s \in Sources |-> "out"]
    /\ tracker = [s \in Sources |-> "none"]
    /\ ready = [s \in Sources |-> FALSE]
    /\ check = [p \in Pairs |-> "missing"]
    /\ superseded = [s \in Sources |-> FALSE]

Active(s) == phase[s] \in {"tracked", "approved"}

Edge(a, b) ==
    /\ a # b
    /\ Active(a) /\ Active(b)
    /\ check[{a, b}] = "true"

Grow(S) == S \cup { t \in Sources : \E x \in S : Edge(x, t) }

\* Three steps cover a graph of at most three sources.
Component(s) == Grow(Grow(Grow({s})))

InConflict(s) == \E t \in Sources : Edge(s, t)

OtherApprovedCovered(s) ==
    \A t \in Sources : (t # s /\ phase[t] = "approved") => check[{s, t}] = "false"

ClearChecks(s) == [p \in Pairs |-> IF s \in p THEN "missing" ELSE check[p]]

Track(s, person, daemon) ==
    /\ phase[s] = "out"
    /\ person \in Persons
    /\ daemon \in Daemons
    /\ phase' = [phase EXCEPT ![s] = "tracked"]
    /\ tracker' = [tracker EXCEPT ![s] = person]
    /\ ready' = [ready EXCEPT ![s] = FALSE]
    /\ check' = ClearChecks(s)
    /\ UNCHANGED superseded

Untrack(s, person, daemon) ==
    /\ phase[s] = "tracked"
    /\ person = tracker[s]
    /\ daemon \in Daemons
    /\ phase' = [phase EXCEPT ![s] = "out"]
    /\ tracker' = [tracker EXCEPT ![s] = "none"]
    /\ ready' = [ready EXCEPT ![s] = FALSE]
    /\ UNCHANGED <<check, superseded>>

CompleteExtract(s, daemon) ==
    /\ phase[s] = "tracked"
    /\ ~ready[s]
    /\ daemon \in Daemons
    /\ ready' = [ready EXCEPT ![s] = TRUE]
    /\ UNCHANGED <<phase, tracker, check, superseded>>

WriteCheck(a, b, value, daemon) ==
    /\ a # b
    /\ ready[a] /\ ready[b]
    /\ check[{a, b}] = "missing"
    /\ value \in {"false", "true"}
    /\ daemon \in Daemons
    /\ check' = [check EXCEPT ![{a, b}] = value]
    /\ UNCHANGED <<phase, tracker, ready, superseded>>

Approve(s, person, daemon) ==
    /\ phase[s] = "tracked"
    /\ ready[s]
    /\ ~InConflict(s)
    /\ OtherApprovedCovered(s)
    /\ person \in Persons
    /\ daemon \in Daemons
    /\ phase' = [phase EXCEPT ![s] = "approved"]
    /\ UNCHANGED <<tracker, ready, check, superseded>>

PolicyApprove(s, daemon) ==
    /\ phase[s] = "tracked"
    /\ ready[s]
    /\ ~InConflict(s)
    /\ OtherApprovedCovered(s)
    /\ daemon \in Daemons
    /\ phase' = [phase EXCEPT ![s] = "approved"]
    /\ UNCHANGED <<tracker, ready, check, superseded>>

Reject(s, daemon) ==
    /\ phase[s] = "tracked"
    /\ ~InConflict(s)
    /\ daemon \in Daemons
    /\ phase' = [phase EXCEPT ![s] = "rejected"]
    /\ UNCHANGED <<tracker, ready, check, superseded>>

\* The owner and the daemon are arguments of the command. They are not a
\* field of result. result assigns the whole connected component.
Resolve(comp, result, person, daemon) ==
    /\ person = Owner
    /\ daemon \in Daemons
    /\ Cardinality(comp) >= 2
    /\ \E s \in comp : comp = Component(s)
    /\ \A s \in comp : InConflict(s)
    /\ result \in [comp -> Results]
    /\ Cardinality({ x \in comp : result[x] = "approved" }) <= 1
    /\ \A x \in comp :
        \/ /\ result[x] = "rejected" /\ phase[x] = "tracked"
        \/ /\ result[x] = "superseded" /\ phase[x] = "approved"
        \/ /\ result[x] = "approved"
           /\ phase[x] \in {"tracked", "approved"}
           /\ (phase[x] = "tracked" => ready[x])
    /\ (\E x \in comp : result[x] = "superseded") =>
        Cardinality({ x \in comp : result[x] = "approved" }) = 1
    /\ LET approvedAfter ==
            { x \in comp : result[x] = "approved" }
            \cup { x \in Sources \ comp : phase[x] = "approved" }
       IN \A a, b \in approvedAfter : a # b => check[{a, b}] = "false"
    /\ phase' = [x \in Sources |-> IF x \in comp THEN result[x] ELSE phase[x]]
    /\ superseded' = [x \in Sources |->
        superseded[x] \/ (x \in comp /\ result[x] = "superseded")]
    /\ UNCHANGED <<tracker, ready, check>>

\* No tracked or untracked source remains. Further steps are not required.
Quiet == \A s \in Sources : phase[s] \in {"approved", "rejected", "superseded"}

Next ==
    \/ \E s \in Sources, person \in Persons, daemon \in Daemons :
        Track(s, person, daemon)
    \/ \E s \in Sources, person \in Persons, daemon \in Daemons :
        Untrack(s, person, daemon)
    \/ \E s \in Sources, daemon \in Daemons : CompleteExtract(s, daemon)
    \/ \E a, b \in Sources, value \in {"false", "true"}, daemon \in Daemons :
        WriteCheck(a, b, value, daemon)
    \/ \E s \in Sources, person \in Persons, daemon \in Daemons :
        Approve(s, person, daemon)
    \/ \E s \in Sources, daemon \in Daemons : PolicyApprove(s, daemon)
    \/ \E s \in Sources, daemon \in Daemons : Reject(s, daemon)
    \/ \E comp \in SUBSET Sources :
        \E result \in [comp -> Results], person \in Persons, daemon \in Daemons :
            Resolve(comp, result, person, daemon)
    \/ Quiet /\ UNCHANGED vars

Spec == Init /\ [][Next]_vars

TwoApprovedAgree ==
    \A a, b \in Sources :
        (a # b /\ phase[a] = "approved" /\ phase[b] = "approved") =>
            check[{a, b}] = "false"

ApprovedRequiresReady ==
    \A s \in Sources : phase[s] = "approved" => ready[s]

SupersededOnlyFromApproved ==
    \A s \in Sources : phase[s] = "superseded" => superseded[s]

OutIsEmpty ==
    \A s \in Sources :
        phase[s] = "out" => tracker[s] = "none" /\ ~ready[s]

TrackedHasTracker ==
    \A s \in Sources : phase[s] = "tracked" => tracker[s] \in Persons

================================================================================
