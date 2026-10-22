package org.pujaparchi.app

import android.content.Context

/** Server address, PIN and the sticky book live here; nothing else does. */
class Store(ctx: Context) {

    private val prefs = ctx.getSharedPreferences("puja", Context.MODE_PRIVATE)

    var base: String
        get() = prefs.getString("base", "")?.trim()?.trimEnd('/') ?: ""
        set(v) {
            val clean = v.trim().trimEnd('/')
            prefs.edit().putString("base", clean).apply()
        }

    var pin: String
        get() = prefs.getString("pin", "")?.trim() ?: ""
        set(v) {
            prefs.edit().putString("pin", v.trim()).apply()
        }

    /** Where the last successful capture was left, so the setup screen can nudge. */
    var lastName: String
        get() = prefs.getString("last_name", "") ?: ""
        set(v) {
            prefs.edit().putString("last_name", v).apply()
        }
}
