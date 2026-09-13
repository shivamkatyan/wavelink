package dev.wavelink.app.emitter

import dev.wavelink.app.*

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.graphics.drawable.GradientDrawable
import android.media.projection.MediaProjectionManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.method.ScrollingMovementMethod
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView

/**
 * Emitter UI (framework-only Views — same discipline as the receiver; no
 * AndroidX/Compose). Refined surface for the same logic:
 *
 *  - Status card: coloured state dot + state name + one-line metrics summary
 *    (FR-053/FR-056: shape + spoken word carry the state; colour is redundant).
 *  - ONE contextual Start/Stop action (mutually exclusive, disabled while
 *    transitioning) — replaces two always-visible buttons.
 *  - Free/Pro toggle (FR-040/048) with FR-047 downgrade confirm; FR-052
 *    explain-before-prompt; dark-mode-aware tokens (values/ + values-night/).
 */
class EmitterActivity : Activity() {

    private lateinit var policyGate: PolicyGate
    private lateinit var tierSwitch: Switch
    private lateinit var statusView: TextView
    private lateinit var dotView: View
    private lateinit var stateLabel: TextView
    private lateinit var actionButton: Button
    private val main = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        policyGate = PolicyGate(SharedPrefsPolicyStore(this))
        setContentView(buildLayout())
    }

    private fun buildLayout(): ViewGroup {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(24, 48, 24, 24)
        }

        root.addView(
            TextView(this).apply {
                text = "Wavelink — app-audio capture"
                textSize = 20f
                setTextColor(getColor(R.color.wdr_textPrimary))
                contentDescription = "Wavelink, app audio capture"
            },
        )

        // FR-053 status card: dot + state + live metrics line.
        val card = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(14), dp(12), dp(14), dp(12))
            background = GradientDrawable().apply {
                cornerRadius = resources.getDimension(R.dimen.wdr_radius)
                setColor(getColor(R.color.wdr_surface))
            }
        }
        dotView = View(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                resources.getDimensionPixelSize(R.dimen.wdr_dot),
                resources.getDimensionPixelSize(R.dimen.wdr_dot),
            ).apply {
                marginEnd = dp(10)
            }
        }
        card.addView(dotView)
        stateLabel = TextView(this).apply {
            textSize = 16f
            setTextColor(getColor(R.color.wdr_textPrimary))
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 3f)
        }
        card.addView(stateLabel)
        root.addView(card)

        // FR-040/FR-048 persistent dev toggle: Free (lossy) vs Pro (+lossless).
        tierSwitch = Switch(this).apply {
            isChecked = policyGate.currentTier() == EntitlementTier.PRO
            text = if (isChecked) "Pro (lossless allowed)" else "Free (lossy only)"
            setTextColor(getColor(R.color.wdr_textSecondary))
            contentDescription = "Free or Pro entitlement toggle. " + text
            setOnCheckedChangeListener { _, checked ->
                val requested = if (checked) EntitlementTier.PRO else EntitlementTier.FREE
                val result = policyGate.toggle(requested)
                if (result == RenegotiationRequest.REQUIRES_CONFIRM) {
                    // FR-047: never silently downgrade a lossless session.
                    AlertDialog.Builder(this@EmitterActivity)
                        .setTitle("Confirm lossless -> lossy downgrade")
                        .setMessage(
                            "A lossless session is currently active. Switching to Free " +
                                "drops lossless fidelity. Downgrade now?",
                        )
                        .setPositiveButton("Downgrade") { _, _ ->
                            policyGate.applyConfirmedTier(EntitlementTier.FREE)
                            tierSwitch.isChecked = false
                        }
                        .setNegativeButton("Keep Pro", null)
                        .show()
                } else {
                    tierSwitch.text = if (isChecked) "Pro (lossless allowed)" else "Free (lossy only)"
                }
            }
        }
        root.addView(tierSwitch)

        // FR-052: explanation BEFORE the OS prompt.
        root.addView(
            Button(this).apply {
                text = "Explain permission"
                contentDescription = "Explains why capture permission is needed"
                backgroundTintList = android.content.res.ColorStateList.valueOf(getColor(R.color.wdr_accent))
                setOnClickListener { showExplanation() }
            },
        )

        // ONE contextual Start/Stop action.
        actionButton = Button(this).apply {
            text = "Start capture"
            backgroundTintList = android.content.res.ColorStateList.valueOf(getColor(R.color.wdr_accent))
            contentDescription = "Starts capture after asking the system for permission, or stops the current session"
            setOnClickListener {
                if (isCapturing()) {
                    EmitterService.stopCapture(this@EmitterActivity)
                } else {
                    requestConsent()
                }
            }
        }
        root.addView(actionButton)

        statusView = TextView(this).apply {
            textSize = 14f
            maxLines = 12
            movementMethod = ScrollingMovementMethod()
            gravity = Gravity.START
            setTextColor(getColor(R.color.wdr_textSecondary))
        }
        root.addView(statusView)

        renderStatus(EmitterService.displayStatus)

        return ScrollView(this).apply { addView(root) }
    }

    private fun isCapturing(): Boolean = EmitterService.displayStatus.state == "STREAMING"

    /** FR-053 render into the status card + log line. */
    private fun renderStatus(s: StatusModel) {
        val indicator = s.visualIndicator()
        val (label, colorRes) = when (s.state) {
            "STREAMING" -> Pair("Streaming", R.color.wdr_good)
            "ERROR" -> Pair("Error", R.color.wdr_error)
            "PAUSED" -> Pair("Paused", R.color.wdr_warn)
            else -> Pair("Idle", R.color.wdr_textSecondary)
        }
        stateLabel.text = label
        dotView.background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(getColor(colorRes))
        }
        // FR-056: shape/icon + spoken label carry the state (colour redundant).
        statusView.text = "${indicator.shape} ${indicator.spoken}\n${s.statusLine()}"
        statusView.contentDescription = "${indicator.spoken}. ${s.statusLine()}"
        // Single contextual action reflects the state.
        actionButton.text = if (isCapturing()) "Stop capture" else "Start capture"
        actionButton.isEnabled = s.state !in listOf("CONNECTING", "STOPPING")
    }

    /** FR-052 in-app explanation, shown immediately before the system prompt. */
    private fun showExplanation() {
        AlertDialog.Builder(this)
            .setTitle("Why capture permission is needed")
            .setMessage(
                "Wavelink emits this device's app audio (music, games) to a " +
                    "paired receiver over Wi-Fi. To do that Android asks your consent to " +
                    "capture playback. The system prompt below is asked again for EVERY " +
                    "capture session; you can revoke it at any time from the capture " +
                    "indicator. Protected content is silenced by the OS and is never " +
                    "captured. No audio is recorded or stored.",
            )
            .setPositiveButton("Got it", null)
            .show()
    }

    @Suppress("DEPRECATION")
    private fun requestConsent() {
        val pm = getSystemService(MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
        startActivityForResult(pm.createScreenCaptureIntent(), REQ_CONSENT)
    }

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQ_CONSENT) return
        if (resultCode == RESULT_OK && data != null) {
            EmitterService.startCapture(
                this,
                data,
                resultCode,
                CapturePolicy(allowMedia = true, allowGame = true, allowUnknown = true),
            )
        } else {
            renderStatus(StatusModel(state = "ERROR", route = "consent-denied", fidelity = "unknown"))
        }
    }

    override fun onResume() {
        super.onResume()
        // Live push from the service (posts to the main looper itself).
        EmitterService.displayStatusListener = { s -> main.post { renderStatus(s) } }
        renderStatus(EmitterService.displayStatus)
    }

    override fun onPause() {
        super.onPause()
        EmitterService.displayStatusListener = null
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private companion object {
        const val REQ_CONSENT = 1001
    }
}
