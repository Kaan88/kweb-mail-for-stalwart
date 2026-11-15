package ovh.kweb.mail.ui

/**
 * Which remembered state (scroll positions and the like) belongs to which entry of the back stack.
 * A screen that is covered by another one leaves the composition; its state is kept under [key] and
 * given back when the screen returns, and dropped once the route has left the stack ([gone]).
 */
object RouteStateRules {
    /**
     * The key of a route's state: a plain string (it must be saveable), different for every account
     * so that nothing one account's screen remembered can show in another's, and equal for equal routes
     * (the screens' view model stores are kept by route in the same way).
     */
    fun key(account: String, route: Route): String = "$account|$route"

    /** The keys that were on the stack before and are not now: their state is to be removed. */
    fun gone(before: Set<String>, account: String, stack: List<Route>): Set<String> = before - keys(account, stack)

    fun keys(account: String, stack: List<Route>): Set<String> = stack.mapTo(HashSet()) { key(account, it) }
}
