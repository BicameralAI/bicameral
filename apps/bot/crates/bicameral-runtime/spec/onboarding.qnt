// -*- mode: Bluespec; -*-
/**
 * Operator onboarding journey: pair → create Product → share to Cloud →
 * another operator joins. Daemon, browser (dashboard) and Cloud state as one
 * world, with the faults that real users hit: lost responses, a failed
 * registry write, daemon restarts, page reloads and Drive access.
 *
 * The model follows the code on main. Each invariant is a property the
 * product should have; a violation is a bug. Open fixes are switches so one
 * run shows what is still broken after they merge:
 *
 *   FIX_DRIVE_SCOPE   #70  request `drive` instead of `drive.file`   (#50)
 *   FIX_HEADER        #71  header follows daemon liveness            (#52)
 *   FIX_RETRY         #72  Retry usable after auto-resume            (#54)
 *   FIX_MARKER        none keep the create marker on post-append errors
 *   FIX_JOIN_ROUTE    none operator daemon can register a joined Product
 *
 * Source correspondence (apps/bot):
 *   pair/reattach      gateway routes.rs hosted_bridge_pair_* , durable_pairing_reattach
 *   createCall         runtime daemon.rs ProductCreate + persist_created_product_route;
 *                      dashboard lib/hosted-project-registration.ts createHostedProject
 *   publishCall        runtime daemon.rs ProductPublish; Cloud POST /api/v1/products
 *   autoResume         dashboard pages/HostedDashboard.svelte resume effect
 *   joinCall           runtime daemon.rs ProductJoin → resolve_request_route
 *   syncAndRestart     workspace-scoped daemon startup migration (harness #21 path)
 *   uiCallFails        dashboard lib/hosted-daemon-connection.ts disconnect listener
 *   header             dashboard lib/connect-status.ts bridgeStateToConnectStatus
 *
 * Abstracted: ticket signing and domain policy (tickets are valid and the
 * domain allowed), commit proofs, the in-memory registration cache (see
 * product_registry.qnt), Cloud enrollment, pairing denial/expiry.
 *
 * Run (from this directory), one instance per row:
 *   quint run onboarding.qnt --main=onboarding_main      --invariant=Intended
 *   quint run onboarding.qnt --main=onboarding_open_prs  --invariant=Intended
 *   quint run onboarding.qnt --main=onboarding_all_fixed --invariant=Intended
 */
module onboarding {
  // ── Constants ────────────────────────────────────────────────────────────
  const Accts: Set[str]
  /// Desktop install (`run_operator`): Memory substrate, no current route.
  const OPERATOR_SCOPED: bool
  const FIX_DRIVE_SCOPE: bool
  const FIX_HEADER: bool
  const FIX_RETRY: bool
  const FIX_MARKER: bool
  const FIX_JOIN_ROUTE: bool

  // ── Types ────────────────────────────────────────────────────────────────

  /// Browser recovery marker (localStorage) for one create attempt.
  /// phase: "none" | "create_requested" | "canonical_created"
  type Marker = { phase: str, key: int, pid: int }

  type World = {
    // Durable, shared
    products: int -> { creator: str, key: int },   // ledger CreateProduct events
    members: Set[(int, str)],                      // ledger ProductMemberAdded
    cloudDir: Set[int],                            // Cloud product directory
    // Per operator: daemon
    registry: str -> Set[int],                     // operator registry (persisted)
    paired: str -> bool,                           // durable pairing, not revoked
    session: str -> bool,                          // in-memory bridge session
    daemonUp: str -> bool,
    // Per operator: browser
    marker: str -> Marker,
    uiPhase: str -> str,       // idle|creating|create_retry|publishing|pub_retry|published|failed
    uiDisconnected: str -> bool,
    nameKnown: str -> bool,    // displayName seeded in the retry dialog
    folderReady: str -> bool,  // folder step completed in this page load
    resumed: str -> bool,      // auto-resume already ran this page load
    failedKey: str -> int,     // key of the attempt the UI last reported "failed"
    lastJoin: str -> str,      // reply to the operator's last join
    // Bookkeeping
    nextKey: int,
    nextPid: int,
    lastAction: str,
  }

  var w: World

  pure val NoMarker: Marker = { phase: "none", key: -1, pid: -1 }

  // ── Functional layer ─────────────────────────────────────────────────────

  pure def productsForKey(ps: int -> { creator: str, key: int }, key: int): Set[int] =
    ps.keys().filter(p => ps.get(p).key == key)

  /// Header pill: on main it follows the bridge state only (#52).
  def headerConnected(s: World, a: str): bool =
    if (FIX_HEADER) s.paired.get(a) and not(s.uiDisconnected.get(a)) else s.paired.get(a)

  /// Retry button in the "Not shared yet" dialog (#54).
  def retryEnabled(s: World, a: str): bool =
    if (FIX_RETRY) true else s.nameKnown.get(a) and s.folderReady.get(a)

  /// Can operator b's daemon read the Product's Drive folder?
  def canRead(s: World, b: str, pid: int): bool =
    s.products.get(pid).creator == b or FIX_DRIVE_SCOPE

  def reachable(s: World, a: str): bool = s.daemonUp.get(a) and s.session.get(a)

  /// Append CreateProduct for the key, or reuse the Product already created
  /// with it (create is idempotent on the key).
  pure def appendCreate(s: World, a: str, key: int): { s: World, pid: int } = {
    val existing = productsForKey(s.products, key)
    if (existing.size() > 0) { s: s, pid: existing.fold(-1, (acc, p) => if (acc < 0) p else acc) }
    else {
      s: { ...s, products: s.products.put(s.nextPid, { creator: a, key: key }), nextPid: s.nextPid + 1 },
      pid: s.nextPid,
    }
  }

  // ── Actions ──────────────────────────────────────────────────────────────

  action init = all {
    w' = {
      products: Map(), members: Set(), cloudDir: Set(),
      registry: Accts.mapBy(_ => Set()),
      paired: Accts.mapBy(_ => false),
      session: Accts.mapBy(_ => false),
      daemonUp: Accts.mapBy(_ => true),
      marker: Accts.mapBy(_ => NoMarker),
      uiPhase: Accts.mapBy(_ => "idle"),
      uiDisconnected: Accts.mapBy(_ => false),
      nameKnown: Accts.mapBy(_ => false),
      folderReady: Accts.mapBy(_ => false),
      resumed: Accts.mapBy(_ => false),
      failedKey: Accts.mapBy(_ => -1),
      lastJoin: Accts.mapBy(_ => ""),
      nextKey: 0, nextPid: 0,
      lastAction: "init",
    },
  }

  action pair(a: str): bool = all {
    w.daemonUp.get(a), not(w.paired.get(a)),
    w' = { ...w, paired: w.paired.set(a, true), session: w.session.set(a, true),
           uiDisconnected: w.uiDisconnected.set(a, false), lastAction: "pair" },
  }

  /// Daemon stops: in-memory sessions are gone.
  action stopDaemon(a: str): bool = all {
    w.daemonUp.get(a),
    w' = { ...w, daemonUp: w.daemonUp.set(a, false), session: w.session.set(a, false),
           lastAction: "stopDaemon" },
  }

  action startDaemon(a: str): bool = all {
    not(w.daemonUp.get(a)),
    w' = { ...w, daemonUp: w.daemonUp.set(a, true), lastAction: "startDaemon" },
  }

  /// Durable reattach after restart (/pair/status) or re-pair from the modal.
  action reattach(a: str): bool = all {
    w.daemonUp.get(a), w.paired.get(a), not(w.session.get(a)),
    w' = { ...w, session: w.session.set(a, true), uiDisconnected: w.uiDisconnected.set(a, false),
           lastAction: "reattach" },
  }

  /// Any dashboard request that reaches no live daemon session.
  action uiCallFails(a: str): bool = all {
    w.paired.get(a), not(reachable(w, a)), not(w.uiDisconnected.get(a)),
    w' = { ...w, uiDisconnected: w.uiDisconnected.set(a, true), lastAction: "uiCallFails" },
  }

  /// Browser reload: page state is lost, the localStorage marker is not.
  action reloadPage(a: str): bool = all {
    w' = { ...w, nameKnown: w.nameKnown.set(a, false), folderReady: w.folderReady.set(a, false),
           resumed: w.resumed.set(a, false), uiPhase: w.uiPhase.set(a, "idle"),
           lastAction: "reloadPage" },
  }

  /// User fills the form (name + folder) and clicks Create.
  action startCreate(a: str): bool = all {
    w.paired.get(a),
    w.marker.get(a).phase == "none",
    Set("idle", "failed", "published").contains(w.uiPhase.get(a)),
    w' = { ...w,
      marker: w.marker.set(a, { phase: "create_requested", key: w.nextKey, pid: -1 }),
      nextKey: w.nextKey + 1,
      nameKnown: w.nameKnown.set(a, true), folderReady: w.folderReady.set(a, true),
      uiPhase: w.uiPhase.set(a, "creating"),
      lastAction: "startCreate" },
  }

  def mayCallCreate(a: str): bool = and {
    w.marker.get(a).phase == "create_requested",
    w.uiPhase.get(a) == "creating" or (w.uiPhase.get(a) == "create_retry" and retryEnabled(w, a)),
  }

  /// product.create could not reach the daemon (transport error).
  action createUnreachable(a: str): bool = all {
    mayCallCreate(a), not(reachable(w, a)),
    w' = { ...w, uiPhase: w.uiPhase.set(a, "create_retry"),
           uiDisconnected: w.uiDisconnected.set(a, w.paired.get(a)), lastAction: "createUnreachable" },
  }

  /// product.create reached the daemon. `fault` is what happened:
  ///   ok            appended (or reused), route persisted, response delivered
  ///   lost          appended, route persisted, response lost (transport error)
  ///   registryFail  appended, route write failed → status "error"
  ///   denied        typed denial before any append (e.g. folder no longer
  ///                 validated after a restart) → status "rejected"
  action createCall(a: str, fault: str): bool = all {
    mayCallCreate(a), reachable(w, a),
    val key = w.marker.get(a).key
    val r = appendCreate(w, a, key)
    val registered = { ...r.s, registry: r.s.registry.set(a, r.s.registry.get(a).union(Set(r.pid))) }
    // On main the dashboard clears the marker on any non-"ok" status.
    val failedClears = { ...w, marker: w.marker.set(a, NoMarker), uiPhase: w.uiPhase.set(a, "failed"),
                         failedKey: w.failedKey.set(a, key) }
    w' = if (fault == "ok")
           { ...registered, marker: w.marker.set(a, { phase: "canonical_created", key: key, pid: r.pid }),
             uiPhase: w.uiPhase.set(a, "publishing"), lastAction: "create_ok" }
         else if (fault == "lost")
           { ...registered, uiPhase: w.uiPhase.set(a, "create_retry"), lastAction: "create_lost" }
         else if (fault == "registryFail")
           (if (FIX_MARKER) { ...r.s, uiPhase: w.uiPhase.set(a, "create_retry"), lastAction: "create_registryFail" }
            else { ...r.s, marker: failedClears.marker, uiPhase: failedClears.uiPhase,
                   failedKey: failedClears.failedKey, lastAction: "create_registryFail" })
         else  // denied
           (if (FIX_MARKER and productsForKey(w.products, key).size() > 0)
              { ...w, uiPhase: w.uiPhase.set(a, "create_retry"), lastAction: "create_denied" }
            else { ...failedClears, lastAction: "create_denied" }),
  }

  /// product.publish then the Cloud POST. Runs right after create, from the
  /// Retry button, or from auto-resume.
  action publishCall(a: str, cloudOk: bool): bool = all {
    w.marker.get(a).phase == "canonical_created",
    w.uiPhase.get(a) == "publishing" or (w.uiPhase.get(a) == "pub_retry" and retryEnabled(w, a)),
    reachable(w, a),
    val pid = w.marker.get(a).pid
    w' = if (not(w.registry.get(a).contains(pid)))
           { ...w, uiPhase: w.uiPhase.set(a, "pub_retry"), lastAction: "publish_not_registered" }
         else if (cloudOk)
           { ...w, cloudDir: w.cloudDir.union(Set(pid)), marker: w.marker.set(a, NoMarker),
             uiPhase: w.uiPhase.set(a, "published"), lastAction: "publish_ok" }
         else
           { ...w, uiPhase: w.uiPhase.set(a, "pub_retry"), lastAction: "publish_cloud_fail" },
  }

  /// After a reload, the dashboard resumes a pending create once. The retry
  /// dialog it opens has no display name and no completed folder step.
  action autoResume(a: str): bool = all {
    w.marker.get(a).phase != "none", not(w.resumed.get(a)), reachable(w, a),
    w.uiPhase.get(a) == "idle",
    w' = { ...w, resumed: w.resumed.set(a, true),
           uiPhase: w.uiPhase.set(a,
             if (w.marker.get(a).phase == "create_requested") "creating" else "publishing"),
           lastAction: "autoResume" },
  }

  /// Operator b joins a shared Product through the dashboard.
  action joinCall(b: str, pid: int): bool = all {
    w.cloudDir.contains(pid),
    w.products.get(pid).creator != b,
    w.paired.get(b), reachable(w, b),
    val registered = w.registry.get(b).contains(pid) or FIX_JOIN_ROUTE
    val reply =
      if (not(registered)) "product_not_registered"
      else if (not(canRead(w, b, pid))) "daemon_unavailable"
      else "granted"
    w' = { ...w,
      members: if (reply == "granted") w.members.union(Set((pid, b))) else w.members,
      registry: if (reply == "granted") w.registry.set(b, w.registry.get(b).union(Set(pid))) else w.registry,
      lastJoin: w.lastJoin.set(b, reply),
      lastAction: "join" },
  }

  /// Workspace-scoped daemons only: events synced into the daemon's own store
  /// and a restart lets the startup migration register them (harness #21).
  action syncAndRestart(b: str): bool = all {
    not(OPERATOR_SCOPED), w.daemonUp.get(b),
    val readable = w.products.keys().filter(p => canRead(w, b, p))
    w' = { ...w, registry: w.registry.set(b, w.registry.get(b).union(readable)),
           session: w.session.set(b, false), lastAction: "syncAndRestart" },
  }

  action step = {
    nondet a = Accts.oneOf()
    nondet fault = Set("ok", "lost", "registryFail", "denied").oneOf()
    nondet cloudOk = Set(true, false).oneOf()
    nondet pid = (if (w.cloudDir.size() > 0) w.cloudDir else Set(-1)).oneOf()
    any {
      pair(a), stopDaemon(a), startDaemon(a), reattach(a), uiCallFails(a), reloadPage(a),
      startCreate(a), createUnreachable(a), createCall(a, fault), publishCall(a, cloudOk),
      autoResume(a), syncAndRestart(a),
      all { pid >= 0, joinCall(a, pid) },
    }
  }

  // ── Structural invariants (hold on main) ─────────────────────────────────

  /// Create is idempotent on its key: never two Products for one key.
  val OneProductPerKey: bool =
    w.products.keys().forall(p => productsForKey(w.products, w.products.get(p).key).size() == 1)

  val SharedProductsExist: bool = w.cloudDir.subseteq(w.products.keys())

  val MembersOfExistingProducts: bool = w.members.forall(((p, _)) => w.products.keys().contains(p))

  val Structural: bool = and { OneProductPerKey, SharedProductsExist, MembersOfExistingProducts }

  // ── Intended properties (each violation is a bug) ────────────────────────

  /// NEW. "Failed" in the UI means nothing was created. Otherwise the user
  /// clicks Create again with a new key and gets a second Product.
  val FailedMeansNothingCreated: bool =
    Accts.forall(a => w.uiPhase.get(a) == "failed" implies
      productsForKey(w.products, w.failedKey.get(a)).size() == 0)

  /// NEW. A shared Product is never refused to a joiner for missing local
  /// registration.
  val JoinNotBlockedByRegistration: bool =
    Accts.forall(b => w.lastJoin.get(b) != "product_not_registered")

  /// #50. A joiner's daemon can read the shared Product.
  val JoinCanReadProduct: bool = Accts.forall(b => w.lastJoin.get(b) != "daemon_unavailable")

  /// #52. The header never says Connected while the disconnect modal shows.
  val HeaderAgreesWithModal: bool =
    Accts.forall(a => not(headerConnected(w, a) and w.uiDisconnected.get(a)))

  /// #54. A retry dialog always has a usable Retry button.
  val RetryUsable: bool =
    Accts.forall(a => Set("pub_retry", "create_retry").contains(w.uiPhase.get(a)) implies retryEnabled(w, a))

  val Intended: bool = and {
    Structural,
    FailedMeansNothingCreated, JoinNotBlockedByRegistration, JoinCanReadProduct,
    HeaderAgreesWithModal, RetryUsable,
  }

  // ── Witnesses ────────────────────────────────────────────────────────────
  val wPublished: bool = w.cloudDir.size() > 0
  val wJoined: bool = w.members.size() > 0
  val wResumed: bool = w.lastAction == "autoResume"
  val wCreateLostThenRecovered: bool =
    w.lastAction == "create_ok" and w.products.keys().exists(p => w.products.get(p).key < w.nextKey - 1)
  val wDisconnected: bool = Accts.exists(a => w.uiDisconnected.get(a))
}

module onboarding_main {
  import onboarding(
    Accts = Set("A", "B"), OPERATOR_SCOPED = true,
    FIX_DRIVE_SCOPE = false, FIX_HEADER = false, FIX_RETRY = false,
    FIX_MARKER = false, FIX_JOIN_ROUTE = false,
  ).*
}

/// main + the open PRs #70, #71, #72.
module onboarding_open_prs {
  import onboarding(
    Accts = Set("A", "B"), OPERATOR_SCOPED = true,
    FIX_DRIVE_SCOPE = true, FIX_HEADER = true, FIX_RETRY = true,
    FIX_MARKER = false, FIX_JOIN_ROUTE = false,
  ).*
}

module onboarding_all_fixed {
  import onboarding(
    Accts = Set("A", "B"), OPERATOR_SCOPED = true,
    FIX_DRIVE_SCOPE = true, FIX_HEADER = true, FIX_RETRY = true,
    FIX_MARKER = true, FIX_JOIN_ROUTE = true,
  ).*
}

/// Join registration fixed but Drive scope still drive.file: shows #50,
/// which the registration bug otherwise hides.
module onboarding_scope_check {
  import onboarding(
    Accts = Set("A", "B"), OPERATOR_SCOPED = true,
    FIX_DRIVE_SCOPE = false, FIX_HEADER = true, FIX_RETRY = true,
    FIX_MARKER = true, FIX_JOIN_ROUTE = true,
  ).*
}
