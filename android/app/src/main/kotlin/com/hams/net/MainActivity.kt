package com.hams.net

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import com.hams.net.core.CaGuidance

/** Skeleton UI: connect button and the CA install guidance screen text. */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val layout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(32, 32, 32, 32) }
        layout.addView(TextView(this).apply { text = CaGuidance.text() })
        layout.addView(Button(this).apply {
            text = "Connect"
            setOnClickListener {
                val consent = VpnService.prepare(this@MainActivity)
                if (consent != null) startActivityForResult(consent, 1) else startVpn()
            }
        })
        setContentView(layout)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == 1 && resultCode == RESULT_OK) startVpn()
    }

    private fun startVpn() {
        startService(Intent(this, HamsVpnService::class.java))
    }
}
