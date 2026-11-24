package com.hams.net

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.content.Context
import android.hardware.usb.UsbManager
import com.hams.net.core.StreamSerialPort
import com.hams.net.core.UsbLine
import com.hams.net.core.UsbSerialPort
import com.hams.net.core.OpenTnc
import com.hoho.android.usbserial.driver.UsbSerialProber
import java.util.UUID

object RfSerial {
    private val SPP: UUID = UUID.fromString("00001101-0000-1000-8000-00805F9B34FB")

    /** The first USB serial device (a TNC or a radio's serial line) the member has granted access to, or null. */
    fun openUsb(context: Context, baud: Int = 9600): OpenTnc? {
        val manager = context.getSystemService(Context.USB_SERVICE) as UsbManager
        val driver = UsbSerialProber.getDefaultProber().findAllDrivers(manager).firstOrNull() ?: return null
        val connection = manager.openDevice(driver.device) ?: return null
        val usb = driver.ports[0]
        usb.open(connection)
        usb.setParameters(baud, 8, 1, 0)
        val line = object : UsbLine {
            override fun write(data: ByteArray, timeoutMs: Int) = usb.write(data, timeoutMs)
            override fun read(buffer: ByteArray, timeoutMs: Int): Int = try { usb.read(buffer, timeoutMs) } catch (e: java.io.IOException) { -1 }
        }
        val port = UsbSerialPort(line)
        return OpenTnc(port, port.input()) { usb.close() }
    }

    /** A paired Bluetooth TNC over SPP (RFCOMM); needs the BLUETOOTH_CONNECT permission, which the caller has asked for. */
    @SuppressLint("MissingPermission")
    fun openBluetooth(device: BluetoothDevice): OpenTnc {
        val socket = device.createRfcommSocketToServiceRecord(SPP)
        socket.connect()
        return OpenTnc(StreamSerialPort(socket.outputStream), socket.inputStream) { socket.close() }
    }
}
