package com.hams.net

import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.test.core.app.ActivityScenario
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/** The RF section on a real (emulated) Android: callsign check, no TNC attached, disable. */
class RfScreenTest {
    private fun <T : View> find(root: View, type: Class<T>, match: (T) -> Boolean): T? {
        if (type.isInstance(root) && match(type.cast(root)!!)) return type.cast(root)
        if (root is ViewGroup) for (i in 0 until root.childCount) find(root.getChildAt(i), type, match)?.let { return it }
        return null
    }

    @Test fun enablingNeedsACallsignAndATnc() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            scenario.onActivity { activity ->
                val root = activity.findViewById<View>(android.R.id.content)
                val enable = assertNotNull(find(root, Button::class.java) { it.text.toString().startsWith("Enable RF") })
                val call = assertNotNull(find(root, EditText::class.java) { true })
                val status = assertNotNull(find(root, TextView::class.java) { it.text.toString() == "RF interface off." })
                enable.performClick()
                assertTrue(status.text.contains("callsign"), status.text.toString())
                call.setText("N0CALL")
                enable.performClick()
                assertTrue(status.text.contains("No TNC found"), status.text.toString())
                find(root, Button::class.java) { it.text.toString() == "Disable RF" }!!.performClick()
                assertEquals("RF interface off.", status.text.toString())
            }
        }
    }
}
