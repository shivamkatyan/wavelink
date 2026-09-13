package dev.wavelink.app.receiver

import dev.wavelink.app.*

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView

/**
 * Receiver launcher Activity (was MISSING — the app could not open). Refined
 * surface, framework-only Views (no AndroidX/Compose), mirroring the emitter:
 *  - status CARD (coloured dot + state + primary metrics; FR-053/FR-056).
 *  - ONE contextual Start/Stop (the foreground render service), mutually
 *    exclusive, disabled while transitioning.
 *  - output-route scan (FR-013/FR-015), tier toggle (FR-040/048) + FR-047
 *    downgrade confirm, and the honest demo-status disclaimer until the
 *    transport is wired (FrameSink seam).
 */
class ReceiverActivity : Activity() {

    private lateinit var policyGate: PolicyGate
    private lateinit var router: AudioOutputRouter
    private lateinit var tierSwitch: Switch
    private lateinit var routeView: TextView
    private lateinit var statusView: TextView
    private lateinit var dotView: View
    private lateinit var stateLabel: TextView
    private lateinit var actionButton: Button
    private val main = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        policyGate = PolicyGate(SharedPrefsPolicyStore(this))
        router = AudioOutputRouter(this)
        setContentView(buildLayout())
        refreshRoute()
    }

    private fun buildLayout(): ViewGroup {
        val scroll = ScrollView(this)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(24, 48, 24, 24)
        }

        root.addView(
            TextView(this).apply {
                text = "Wavelink — Wi-Fi audio receiver"
                textSize = 20f
                setTextColor(getColor(R.color.wdr_textPrimary))
                contentDescription = "Wavelink, Wi-Fi audio receiver"
            },
        )

        // FR-053 status card.
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
            ).apply { marginEnd = dp(10) }
        }
        card.addView(dotView)
        stateLabel = TextView(this).apply {
            textSize = 16f
            setTextColor(getColor(R.color.wdr_textPrimary))
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 3f)
        }
        card.addView(stateLabel)
        root.addView(card)

        // FR-040/FR-048 tier toggle.
        tierSwitch = Switch(this).apply {
            isChecked = policyGate.currentTier() == EntitlementTier.PRO
            text = tierLabel(isChecked)
            setTextColor(getColor(R.color.wdr_textSecondary))
            contentDescription = "Free or Pro entitlement toggle. $text"
            setOnCheckedChangeListener { _, checked ->
                val requested = if (checked) EntitlementTier.PRO else EntitlementTier.FREE
                val result = policyGate.toggle(requested)
                if (result == RenegotiationRequest.REQUIRES_CONFIRM) {
                    AlertDialog.Builder(this@ReceiverActivity)
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
                    tierSwitch.text = tierLabel(checked)
                }
            }
        }
        root.addView(tierSwitch)

        root.addView(
            Button(this).apply {
                text = "Scan output routes"
                contentDescription = "Scan output routes for USB DACs"
                backgroundTintList = android.content.res.ColorStateList.valueOf(getColor(R.color.wdr_accent))
                setOnClickListener { refreshRoute() }
            },
        )
        routeView = TextView(this).apply {
            text = "route: (scanning)"
            setTextColor(getColor(R.color.wdr_textSecondary))
        }
        root.addView(routeView)

        statusView = TextView(this).apply {
            textSize = 14f
            setPadding(0, 16, 0, 0)
            setTextColor(getColor(R.color.wdr_textSecondary))
            contentDescription = "Stream health summary"
        }
        root.addView(statusView)

        // ONE contextual Start/Stop of the foreground render service.
        actionButton = Button(this).apply {
            text = "Start receiver"
            backgroundTintList = android.content.res.ColorStateList.valueOf(getColor(R.color.wdr_accent))
            contentDescription = "Starts or stops the foreground receiver service"
            setOnClickListener {
                if (ReceiverService.serviceRunning) {
                    stopService(Intent(this@ReceiverActivity, ReceiverService::class.java))
                } else {
                    startForegroundService(Intent(this@ReceiverActivity, ReceiverService::class.java))
                }
                renderState(ReceiverService.serviceRunning)
            }
        }
        root.addView(actionButton)

        root.addView(
            TextView(this).apply {
                text = "Note: stream-health values are demo until the network transport is wired (FrameSink seam)."
                textSize = 12f
                setPadding(0, 16, 0, 0)
                setTextColor(getColor(R.color.wdr_textSecondary))
            },
        )

        scroll.addView(root)
        return scroll
    }

    private fun tierLabel(pro: Boolean): String =
        if (pro) "Pro (lossless allowed)" else "Free (lossy only)"

    private fun refreshRoute() {
        val dacs = router.usbDacDevices()
        routeView.text =
            if (dacs.isEmpty()) {
                "route: none (no USB DAC detected)"
            } else {
                "USB DAC(s): " + dacs.joinToString { it.name }
            }
    }

    private fun statusSummary(): String {
        val s = StatusModel() // demo defaults until transport wiring
        return "status=${s.state} · transport=${s.transport} · codec=${s.codec.ifBlank { "—" }} · " +
            "${s.sampleRateHz}/${s.bitDepth}/${s.channels}ch · latency=${s.latencyMs}ms · " +
            "buffer=${s.bufferFill} · loss=${s.packetLossPct}% · underruns=${s.underruns} · " +
            "route=${s.route} · fidelity=${s.fidelity}"
    }

    /** FR-053 render into the card; single action mirrors the service state. */
    private fun renderState(running: Boolean) {
        val colorRes = if (running) R.color.wdr_good else R.color.wdr_textSecondary
        stateLabel.text = if (running) "Rendering" else "Idle"
        stateLabel.setTextColor(getColor(if (running) R.color.wdr_good else R.color.wdr_textSecondary))
        dotView.background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(getColor(colorRes))
        }
        statusView.text = statusSummary() + if (running) "\n(receiver service running)" else ""
        statusView.contentDescription = (if (running) "Rendering." else "Idle.") + " " + statusSummary()
        actionButton.text = if (running) "Stop receiver" else "Start receiver"
        actionButton.isEnabled = true
    }

    override fun onResume() {
        super.onResume()
        // Live push from the service (posts to the main looper itself).
        ReceiverService.serviceListener = { running -> main.post { renderState(running) } }
        renderState(ReceiverService.serviceRunning)
    }

    override fun onPause() {
        super.onPause()
        ReceiverService.serviceListener = null
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
}
