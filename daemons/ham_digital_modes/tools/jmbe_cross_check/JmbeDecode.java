import jmbe.iface.IAudioCodec;
import jmbe.codec.imbe.IMBEAudioCodec;
import jmbe.codec.ambe.AMBEAudioCodec;
import jmbe.codec.imbe.IMBEFrame;
import jmbe.codec.ambe.AMBEFrame;
import java.io.*;
import java.nio.file.*;
import java.util.*;

/** JmbeDecode <imbe|ambe> in.hex out.raw : one hex frame per line -> 8 kHz s16le PCM; per-frame error counts to stderr-file out.raw.err */
public class JmbeDecode {
    public static void main(String[] a) throws Exception {
        boolean imbe = a[0].equals("imbe");
        IAudioCodec c = imbe ? new IMBEAudioCodec() : new AMBEAudioCodec();
        List<String> lines = Files.readAllLines(Paths.get(a[1]));
        try (OutputStream o = new BufferedOutputStream(new FileOutputStream(a[2]));
             PrintWriter ew = new PrintWriter(a[2] + ".err")) {
            for (String l : lines) {
                l = l.trim(); if (l.isEmpty()) continue;
                byte[] d = new byte[l.length() / 2];
                for (int i = 0; i < d.length; i++) d[i] = (byte) Integer.parseInt(l.substring(2 * i, 2 * i + 2), 16);
                String info;
                if (imbe) { info = "-"; }
                else { AMBEFrame f = new AMBEFrame(d); info = f.getFrameType() + " " + Arrays.toString(f.getErrors()); }
                float[] pcm = c.getAudio(d);
                ew.println(info);
                for (float s : pcm) {
                    int v = Math.round(s * 32767f); v = Math.max(-32768, Math.min(32767, v));
                    o.write(v & 0xFF); o.write((v >> 8) & 0xFF);
                }
            }
        }
    }
}
