package com.hams.net

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import com.hams.net.core.CaGuidance
import com.hams.net.core.EnrolClient
import com.hams.net.core.EnrolPoll
import com.hams.net.core.RfController
import com.hams.net.core.RfStart
import com.hams.net.core.Blocklist
import com.hams.net.core.DemoLock
import com.hams.net.core.OwnedTxGate
import com.hams.net.core.TxHolds

/** Enrol (shows the code to approve at hams.com/my/ipv6), connect, and the CA install guidance. */
class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var rfStatus: TextView
    private val holds = TxHolds()
    private val rfController = RfController(OwnedTxGate(DemoLock(false, holds), holds, Blocklist())) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val layout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(32, 32, 32, 32) }
        status = TextView(this).apply { text = if (EnrolmentStore.load(this@MainActivity) != null) "Enrolled." else "Not enrolled." }
        layout.addView(status)
        layout.addView(Button(this).apply { text = "Enrol this phone"; setOnClickListener { enrol() } })
        layout.addView(Button(this).apply {
            text = "Connect"
            setOnClickListener {
                val consent = VpnService.prepare(this@MainActivity)
                if (consent != null) startActivityForResult(consent, 1) else startVpn()
            }
        })
        layout.addView(TextView(this).apply { text = CaGuidance.text() })
        val callsign = EditText(this).apply { hint = "Your callsign (for RF)" }
        rfStatus = TextView(this).apply { text = "RF interface off." }
        layout.addView(callsign)
        layout.addView(Button(this).apply {
            text = "Enable RF (USB TNC)"
            setOnClickListener { enableRf(callsign.text.toString()) }
        })
        layout.addView(Button(this).apply { text = "Disable RF"; setOnClickListener { rfController.disable(); rfStatus.text = "RF interface off." } })
        layout.addView(rfStatus)
        setContentView(layout)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == 1 && resultCode == RESULT_OK) startVpn()
    }

    private fun enableRf(callsign: String) {
        rfStatus.text = when (val r = rfController.enable(callsign) { RfSerial.openUsb(this) }) {
            is RfStart.On -> r.reminder
            is RfStart.Failed -> r.message
        }
    }

    private fun startVpn() {
        startService(Intent(this, HamsVpnService::class.java))
    }

    private fun enrol() {
        Thread {
            val flow = EnrolFlow(applicationContext)
            val result = flow.run { code -> runOnUiThread { status.text = "Approve code $code at hams.com/my/ipv6" } }
            runOnUiThread { status.text = if (result) "Enrolled." else "Enrolment failed or timed out." }
        }.start()
    }
}

/** The enrolment dance: ask for a code, wait for the member to approve it, keep the token. Blocking; call off the main thread. */
class EnrolFlow(private val context: android.content.Context, private val client: EnrolClient = EnrolClient(Portal::post), private val pause: (Long) -> Unit = Thread::sleep) {
    fun run(showCode: (String) -> Unit): Boolean {
        val start = client.start() ?: return false
        showCode(start.userCode)
        val publicKey = KeyVault.publicKeyBase64(context)
        repeat(120) {
            when (val r = client.poll(start.deviceCode, publicKey)) {
                is EnrolPoll.Done -> { EnrolmentStore.save(context, r.enrolment); return true }
                EnrolPoll.Refused -> return false
                EnrolPoll.Pending -> pause(start.intervalSeconds * 1000L)
            }
        }
        return false
    }
}
