package io.github.wkdsteve.testing

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // The fleet is internal: no screenshot, no picture in the app switcher, no screen recording (AND-08). A debug build
    // leaves it off, so the views can be checked on a test phone.
    if (!BuildConfig.DEBUG) {
      window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
    }
  }
}
