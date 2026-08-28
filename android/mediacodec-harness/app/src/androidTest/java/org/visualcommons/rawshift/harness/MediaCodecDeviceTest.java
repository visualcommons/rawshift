package org.visualcommons.rawshift.harness;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import android.os.Build;
import android.util.Log;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(AndroidJUnit4.class)
public final class MediaCodecDeviceTest {
    @Test
    public void hardwareStillDecodeFixtures() {
        assertFalse("This is a physical-device gate; emulator results are not accepted", isEmulator());
        assertTrue("rawshift supports Android API 29+", Build.VERSION.SDK_INT >= 29);

        String report = NativeHarness.runSuite(Build.VERSION.SDK_INT);
        Log.i("rawshift-hwdec", "\n" + report);
        assertTrue(report, report.startsWith("PASS\n") || report.startsWith("PASS sdk="));
    }

    private static boolean isEmulator() {
        return Build.FINGERPRINT.startsWith("generic")
                || Build.FINGERPRINT.toLowerCase().contains("emulator")
                || Build.MODEL.contains("google_sdk")
                || Build.MODEL.contains("Emulator")
                || Build.MODEL.contains("Android SDK built for")
                || Build.MANUFACTURER.contains("Genymotion")
                || (Build.BRAND.startsWith("generic") && Build.DEVICE.startsWith("generic"))
                || Build.PRODUCT.contains("sdk_gphone")
                || Build.PRODUCT.contains("google_sdk")
                || Build.PRODUCT.contains("simulator");
    }
}
