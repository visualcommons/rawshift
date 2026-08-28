package org.visualcommons.rawshift.harness;

import android.app.Activity;
import android.os.Build;
import android.os.Bundle;
import android.widget.TextView;

public final class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView report = new TextView(this);
        report.setText(NativeHarness.runSuite(Build.VERSION.SDK_INT));
        report.setTextIsSelectable(true);
        report.setPadding(32, 32, 32, 32);
        setContentView(report);
    }
}
