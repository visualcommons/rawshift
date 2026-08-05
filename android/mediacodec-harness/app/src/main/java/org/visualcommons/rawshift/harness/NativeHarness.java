package org.visualcommons.rawshift.harness;

final class NativeHarness {
    static {
        System.loadLibrary("rawshift_mediacodec_harness_native");
    }

    private NativeHarness() {}

    static native String runSuite(int sdk);
}
