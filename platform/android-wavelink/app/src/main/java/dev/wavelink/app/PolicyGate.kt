package dev.wavelink.app

/**
 * Free/Pro entitlement toggle per FR-040..FR-048, on the capture (emitter) side.
 *
 * Pure JVM by design: persistence is behind [PolicyStore] so this class (and
 * its tests) never touch android.* at runtime (core library desugaring not
 * required).
 *
 * "Unknown" (store never resolved a tier yet) FAILS CLOSED to the most
 * restrictive tier — see [PolicyGate] resolution rules ([EntitlementTier.UNKNOWN]).
 */

enum class EntitlementTier {
    /** Free (default): lossy capture only (FR-042). */
    FREE,

    /** Pro: adds lossless capture (FR-043). */
    PRO,

    /**
     * Store returned no resolved tier (first launch / deferred commerce adapter
     * unanswered). All gate decisions treat it as FREE, so an unresolved
     * entitlement permits the least (fails closed) — never silently grants Pro.
     */
    UNKNOWN,
}

/** True only for PRO: Free/UNKNOWN can never request a lossless path (FR-042). */
val EntitlementTier.allowsLossless: Boolean
    get() = this == EntitlementTier.PRO

/** Result of requesting a tier change when a session may be affected (FR-026/FR-047). */
enum class RenegotiationRequest {
    /** The tier switch is safe to apply now. */
    APPLIED,

    /**
     * The session is currently lossless and the request would silently
     * downgrade it to Free (lossy). The caller must pause-and-confirm (or honour
     * a saved downgrade preference) before applying; the gate does NOT change
     * the tier in this case.
     */
    REQUIRES_CONFIRM,
}

/** Minimal persistence seam so [PolicyGate] itself holds no Android dependencies. */
interface PolicyStore {
    fun readTier(): EntitlementTier
    fun writeTier(tier: EntitlementTier)
}

/**
 * Pure-JVM policy gate (emitter). "Currently streaming lossless" is session
 * state the caller drives via [setStreamingLossless]; the gate refuses to
 * silently downgrade a lossless session PRO -> FREE (lossless -> lossy,
 * FR-047) — same semantics as the receiver gate.
 */
class PolicyGate(private val store: PolicyStore) {

    private var tier: EntitlementTier = store.readTier()
    private var streamingLossless: Boolean = false

    /** An unresolved UNKNOWN tier resolves to FREE for every decision (fails closed). */
    private fun resolve(t: EntitlementTier): EntitlementTier =
        if (t == EntitlementTier.UNKNOWN) EntitlementTier.FREE else t

    fun currentTier(): EntitlementTier = tier

    /** Free (or unresolved/unknown) tier may never request a lossless session (FR-042). */
    fun allowLossless(): Boolean = resolve(tier).allowsLossless

    /** Advertised capabilities for capability negotiation (FR-006/FR-046). */
    fun policy(): Policy = Policy(lossless = allowLossless())

    /** Called by the session layer whenever fidelity state changes. */
    fun setStreamingLossless(streaming: Boolean) {
        streamingLossless = streaming
    }

    fun isStreamingLossless(): Boolean = streamingLossless

    /**
     * FR-047: a downgrade that would take the session from lossless-capable
     * (PRO) to lossy-only (FREE — or unresolved UNKNOWN which fails closed to
     * FREE) while currently streaming lossless must NOT be applied silently.
     * Returns true when the caller must pause-and-confirm first.
     */
    fun requiresConfirmForDowngrade(requested: EntitlementTier): Boolean =
        resolve(tier).allowsLossless && !resolve(requested).allowsLossless && streamingLossless

    /**
     * Toggle the tier. Never silently downgrades a lossless session: when
     * currently streaming lossless and switching PRO -> FREE the request is
     * surfaced as [RenegotiationRequest.REQUIRES_CONFIRM] and the tier is left
     * unchanged (re-applied after confirmed renegotiation). Requesting the
     * unresolved UNKNOWN tier is a no-op (fails closed).
     */
    fun toggle(newTier: EntitlementTier): RenegotiationRequest {
        if (newTier == tier) return RenegotiationRequest.APPLIED
        if (newTier == EntitlementTier.UNKNOWN) return RenegotiationRequest.APPLIED
        if (requiresConfirmForDowngrade(newTier)) return RenegotiationRequest.REQUIRES_CONFIRM
        applyTier(newTier)
        return RenegotiationRequest.APPLIED
    }

    /** Unconditional transitional switch used after the caller has confirmed. */
    fun applyConfirmedTier(newTier: EntitlementTier) {
        applyTier(newTier)
    }

    private fun applyTier(newTier: EntitlementTier) {
        // Never persist the unresolved sentinel; store FREE/PRO only.
        tier = resolve(newTier)
        store.writeTier(tier)
    }
}

/** Capability set a peer uses during FR-006 negotiation. */
data class Policy(
    val lossless: Boolean,
)
