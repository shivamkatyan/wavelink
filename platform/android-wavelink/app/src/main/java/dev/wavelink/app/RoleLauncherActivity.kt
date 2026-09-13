package dev.wavelink.app

import android.app.Activity
import android.content.Intent
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import dev.wavelink.app.emitter.EmitterActivity
import dev.wavelink.app.receiver.ReceiverActivity

class RoleLauncherActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(24, 48, 24, 24)
        }
        root.addView(TextView(this).apply {
            text = "Wavelink"; textSize = 30f; gravity = Gravity.CENTER
            setTextColor(getColor(R.color.wdr_textPrimary))
        })
        root.addView(TextView(this).apply {
            text = "What will this device do?"; textSize = 15f; gravity = Gravity.CENTER
            setPadding(0, 8, 0, 32); setTextColor(getColor(R.color.wdr_textSecondary))
        })
        root.addView(roleTile("Emitter", "Capture & stream this device's audio to a Wavelink receiver") {
            startActivity(Intent(this, EmitterActivity::class.java))
        })
        root.addView(roleTile("Receiver", "Play Wavelink streams through this device's output or USB DAC") {
            startActivity(Intent(this, ReceiverActivity::class.java))
        })
        setContentView(root)
    }
    private fun roleTile(title: String, subtitle: String, onTap: () -> Unit): TextView =
        TextView(this).apply {
            text = "$title\n$subtitle"; textSize = 17f
            setTextColor(getColor(R.color.wdr_textPrimary)); gravity = Gravity.CENTER
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { bottomMargin = 16 }
            setPadding(20, 22, 20, 22)
            background = GradientDrawable().apply {
                cornerRadius = resources.getDimension(R.dimen.wdr_radius)
                setColor(getColor(R.color.wdr_surface))
            }
            setOnClickListener { onTap() }
            contentDescription = "$title. $subtitle"
        }
}
