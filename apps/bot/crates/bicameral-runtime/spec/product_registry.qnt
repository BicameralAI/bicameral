// -*- mode: Bluespec; -*-
/**
 * Product registry cache: what a running daemon believes is registered and
 * bindable, compared with what is durably on disk.
 *
 * The daemon keeps three things for each operator:
 *   durable   operator registry files (FileProjectRegistrationStore)
 *   bindings  workspace-binding files (FileWorkspaceBindingStore)
 *   cache     DaemonDispatch.project_registrations, an in-memory projection of
 *             durable + bindings (`project_registrations_from_store`)
 *
 * Two different sources of truth answer "is this Product known?":
 *   - workspace.bind is gated on the CACHE (StaticWorkspaceBindingRegistry is
 *     built from cached names)
 *   - routing (publish/search/join) reads DURABLE via ProjectEventStoreRouter
 * so any path that changes durable without refreshing the cache makes a
 * Product routable but not bindable. That is the bug fixed in bicameral#65.
 *
 * Source correspondence:
 *   create          daemon.rs  persist_created_product_route -> store.write + refresh
 *   bindAccepted /
 *   bindRejected    daemon.rs  dispatch_body WorkspaceBind branch
 *   refresh         daemon.rs  refresh_project_registrations (keeps old cache on error)
 *   project         daemon.rs  project_registrations_from_store
 *   restart         daemon.rs  startup hydration (empty cache on error)
 *   corruptBinding  environment: a binding file becomes unreadable
 *   setListFailing  environment: registry list() starts/stops failing
 *
 * Not modelled: product.continue_setup (also refreshes; covered by its own
 * unit test), event-store health, routing/publish, hosted readiness redaction,
 * concurrency between requests (dispatch handles one request at a time per
 * cache write; the RwLock makes each refresh atomic).
 *
 * Model-based test: src/daemon/tests/registry_mbt.rs runs `quint run` on this
 * spec (fixed seed, 40 traces) and replays every trace against a real
 * DaemonDispatch. Traces are generated into a temp dir, never committed.
 * CI runs it in .github/workflows/bot-registry-model.yml when this spec or the
 * modelled Rust files change.
 *
 * Run locally (from apps/bot):
 *   npm install -g @informalsystems/quint@0.33.0
 *   cargo test -p bicameral-runtime --lib registry_mbt -- --ignored
 * Explore other sequences: PRODUCT_REGISTRY_MBT_SEED=0x1234 (same command).
 * Check the spec alone (from this directory):
 *   quint run product_registry.qnt --main=product_registry_2 --invariant=safety
 *
 * When changing the modelled code, update this spec first; never edit the
 * spec to make a failing replay pass.
 */
module product_registry {
  // ── Types ────────────────────────────────────────────────────────────────

  type ProductId = str

  /// State of the workspace-binding file for a Product.
  type Binding =
    | Unbound   // no binding file
    | Bound     // LocalWorkspaceBound
    | Damaged   // file exists but cannot be read/parsed

  /// `setup_state` in LocalProjectRegistration.
  type Setup =
    | Normal              // "normal"
    | RepairRequired      // "local_workspace_repair_required"
    | GroundingNotReady   // "code_grounding_not_ready"

  type Entry = { hasBinding: bool, setup: Setup }

  // ── Constants ────────────────────────────────────────────────────────────

  /// Products the operator may create during a trace (opaque ids).
  const PRODUCTS: Set[ProductId]

  // ── State ────────────────────────────────────────────────────────────────

  var durable: Set[ProductId]
  var bindings: ProductId -> Binding
  var cache: ProductId -> Entry
  var listFails: bool

  // Driver-facing bookkeeping: which request ran last and on which Product.
  var lastAction: str
  var lastProduct: ProductId

  // ── Functional layer ─────────────────────────────────────────────────────

  pure def entryFor(b: Binding): Entry = match b {
    | Bound => { hasBinding: true, setup: Normal }
    | Damaged => { hasBinding: false, setup: RepairRequired }
    | Unbound => { hasBinding: false, setup: GroundingNotReady }
  }

  /// project_registrations_from_store, when list() succeeds.
  pure def project(d: Set[ProductId], b: ProductId -> Binding): ProductId -> Entry =
    d.mapBy(p => entryFor(b.get(p)))

  /// refresh_project_registrations: on a list() error the old cache stays.
  pure def refresh(
    d: Set[ProductId], b: ProductId -> Binding, failing: bool, old: ProductId -> Entry
  ): ProductId -> Entry =
    if (failing) old else project(d, b)

  /// The workspace.bind gate: only Products in the cache can be bound.
  pure def bindAllowed(c: ProductId -> Entry, p: ProductId): bool =
    c.keys().contains(p)

  // ── Actions ──────────────────────────────────────────────────────────────

  action init = all {
    durable' = Set(),
    bindings' = PRODUCTS.mapBy(_ => Unbound),
    cache' = Map(),
    listFails' = false,
    lastAction' = "init",
    lastProduct' = "",
  }

  /// product.create (first attempt or idempotent retry).
  action create(p: ProductId): bool = {
    val d = durable.union(Set(p))
    all {
      durable' = d,
      cache' = refresh(d, bindings, listFails, cache),
      bindings' = bindings,
      listFails' = listFails,
      lastAction' = "create",
      lastProduct' = p,
    }
  }

  /// workspace.bind for a Product the cache knows about.
  action bindAccepted(p: ProductId): bool = all {
    bindings.get(p) == Unbound,
    bindAllowed(cache, p),
    val b = bindings.set(p, Bound)
    all {
      bindings' = b,
      cache' = refresh(durable, b, listFails, cache),
      durable' = durable,
      listFails' = listFails,
      lastAction' = "bindAccepted",
      lastProduct' = p,
    }
  }

  /// workspace.bind for a Product the cache does not know about.
  action bindRejected(p: ProductId): bool = all {
    bindings.get(p) == Unbound,
    not(bindAllowed(cache, p)),
    durable' = durable,
    bindings' = bindings,
    cache' = cache,
    listFails' = listFails,
    lastAction' = "bindRejected",
    lastProduct' = p,
  }

  /// Process restart: cache is rehydrated from disk (empty if list() fails).
  action restart: bool = all {
    cache' = refresh(durable, bindings, listFails, Map()),
    durable' = durable,
    bindings' = bindings,
    listFails' = listFails,
    lastAction' = "restart",
    lastProduct' = "",
  }

  /// Environment: a registered Product's binding file becomes unreadable.
  /// Nothing refreshes the cache, so it still shows the old entry.
  action corruptBinding(p: ProductId): bool = all {
    durable.contains(p),
    bindings.get(p) != Damaged,
    bindings' = bindings.set(p, Damaged),
    durable' = durable,
    cache' = cache,
    listFails' = listFails,
    lastAction' = "corruptBinding",
    lastProduct' = p,
  }

  /// Environment: the registry directory starts or stops failing list().
  action setListFailing(f: bool): bool = all {
    listFails != f,
    listFails' = f,
    durable' = durable,
    bindings' = bindings,
    cache' = cache,
    lastAction' = if (f) "listFailing" else "listHealthy",
    lastProduct' = "",
  }

  action step = {
    nondet p = PRODUCTS.oneOf()
    any {
      create(p),
      bindAccepted(p),
      bindRejected(p),
      restart,
      corruptBinding(p),
      setListFailing(true),
      setListFailing(false),
    }
  }

  // ── Properties ───────────────────────────────────────────────────────────

  val refreshingAction: bool =
    Set("create", "bindAccepted", "restart").contains(lastAction)

  /// The cache never shows a Product that is not durably registered.
  val cacheOnlyShowsRegistered: bool = cache.keys().subseteq(durable)

  /// A Product can only become bound if it is registered.
  val bindingsOnlyForRegistered: bool =
    PRODUCTS.forall(p => bindings.get(p) != Unbound implies durable.contains(p))

  /// After any request that refreshes, with a healthy registry, the cache is
  /// exactly the projection of disk. (Fails without the #65 refresh in create.)
  val refreshedCacheIsCurrent: bool =
    (refreshingAction and not(listFails)) implies cache == project(durable, bindings)

  /// A damaged binding never hides a registration: once refreshed it shows as
  /// repair-required.
  val damagedBindingStaysVisible: bool =
    (refreshingAction and not(listFails)) implies
      durable.forall(p => bindings.get(p) == Damaged implies
        cache.get(p) == { hasBinding: false, setup: RepairRequired })

  val safety: bool = and {
    cacheOnlyShowsRegistered,
    bindingsOnlyForRegistered,
    refreshedCacheIsCurrent,
    damagedBindingStaysVisible,
  }

  // ── Witnesses (reachability, checked with --witnesses) ───────────────────

  val wCreated: bool = lastAction == "create"
  val wBound: bool = lastAction == "bindAccepted"
  val wBindRejectedForRegistered: bool =
    lastAction == "bindRejected" and durable.contains(lastProduct)
  val wRestartedWithProducts: bool = lastAction == "restart" and cache.keys().size() > 0
  val wDamagedVisible: bool =
    durable.exists(p => cache.keys().contains(p) and cache.get(p).setup == RepairRequired)

  /// FINDING: a Product that is durably registered (routable) but absent from
  /// the cache (cannot be bound). Reached when list() fails during the refresh
  /// that follows create; heals on the next successful refresh or restart.
  val wRegisteredButUnbindable: bool =
    durable.exists(p => not(cache.keys().contains(p)))
}

module product_registry_2 {
  import product_registry(PRODUCTS = Set("p1", "p2")).*
}
