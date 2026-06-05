// Android host for the alacritty webview POC.
//
// Architecture:
//   1. The alacritty pty server binary ships as `libalacritty-pty-server.so`
//      inside jniLibs/{arm64-v8a,x86_64}/. Android extracts it into
//      `applicationInfo.nativeLibraryDir` at install (extractNativeLibs=true).
//   2. onCreate spawns it as a sidecar process bound to 127.0.0.1:<port>.
//   3. The WebView loads file:///android_asset/index.html which calls
//      `window.AlacrittyHost.wsUrl()` to get the ws:// address, then asks
//      the wasm AlacrittyTerminal to `connect()` to it.
//   4. The back gesture is consumed by an OnBackPressedCallback that
//      forwards ESC over `window.__alacrittyInput`. Double-back exits.
//
// All PTY byte shuttling rides the bundle's WebSocket — no stdio bridge.

package dev.alacritty.webview

import android.annotation.SuppressLint
import android.os.Bundle
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.WebSettings
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import java.io.File
import java.io.IOException
import java.net.Socket
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

class MainActivity : ComponentActivity() {

    companion object {
        private const val TAG = "Alacritty"
        // Loopback port the bundled server listens on. Random would be safer,
        // but the WebView has no way to discover it after spawn unless we
        // poll the host bridge — fixed port keeps the wiring obvious for POC.
        private const val SERVER_PORT = 7681
    }

    private lateinit var webView: WebView
    private val serverProcess = AtomicReference<Process?>(null)

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        spawnServer()

        webView = WebView(this).also { setContentView(it) }
        webView.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            @Suppress("DEPRECATION")
            allowFileAccessFromFileURLs = true
        }
        webView.addJavascriptInterface(Bridge(), "AlacrittyHost")
        webView.loadUrl("file:///android_asset/index.html")

        // Android has no physical Esc key. Remap the back gesture to ESC so
        // TUIs (vim, less, fzf) can cancel out. To actually exit the activity
        // the user can swipe-back twice quickly — see the time check below.
        var lastBackMs = 0L
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val now = System.currentTimeMillis()
                if (now - lastBackMs < 400) {
                    // Double-back within 400ms → actually exit.
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                    return
                }
                lastBackMs = now
                webView.evaluateJavascript(
                    "window.__alacrittyInput && window.__alacrittyInput('\\u001b')",
                    null,
                )
            }
        })
    }

    override fun onDestroy() {
        super.onDestroy()
        serverProcess.getAndSet(null)?.destroy()
    }

    /**
     * Spawn the pty server out of nativeLibraryDir. We use the `lib*.so`
     * naming trick so Android's installer extracts the file and marks it
     * executable, even though it's an ELF executable rather than a shared
     * library. extractNativeLibs=true in the manifest is what makes the
     * file land on disk instead of staying inside the APK zip.
     */
    private fun spawnServer() {
        val binary = File(applicationInfo.nativeLibraryDir, "libalacritty-pty-server.so")
        if (!binary.canExecute()) {
            Log.e(TAG, "server binary not executable at ${binary.absolutePath}")
            return
        }
        try {
            val pb = ProcessBuilder(
                binary.absolutePath,
                "--bind", "127.0.0.1",
                "--port", SERVER_PORT.toString(),
            ).redirectErrorStream(true)
            pb.environment()["HOME"] = applicationContext.filesDir.absolutePath
            pb.environment()["TMPDIR"] = applicationContext.cacheDir.absolutePath
            val proc = pb.start()
            serverProcess.set(proc)
            // Drain stdout/stderr so the pipe buffer doesn't fill and block the child.
            thread(name = "pty-server-log", isDaemon = true) {
                proc.inputStream.bufferedReader().useLines { lines ->
                    lines.forEach { Log.i(TAG, "[pty-server] $it") }
                }
            }
            thread(name = "pty-server-wait", isDaemon = true) {
                val code = proc.waitFor()
                Log.w(TAG, "pty-server exited code=$code")
            }
        } catch (e: IOException) {
            Log.e(TAG, "failed to spawn pty-server", e)
        }
    }

    /** Probe the server with a TCP connect until it accepts. Returns true on success. */
    private fun waitForServerListening(timeoutMs: Long = 3_000): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            try {
                Socket("127.0.0.1", SERVER_PORT).use { return true }
            } catch (_: IOException) {
                Thread.sleep(50)
            }
        }
        return false
    }

    inner class Bridge {
        /** JS calls this to learn where the bundled pty server is listening. */
        @JavascriptInterface
        fun wsUrl(): String {
            if (waitForServerListening()) {
                return "ws://127.0.0.1:$SERVER_PORT/"
            }
            Log.w(TAG, "pty-server didn't come up in time")
            return ""
        }

        @JavascriptInterface
        fun ready() {
            Log.d(TAG, "JS terminal ready")
        }
    }
}
