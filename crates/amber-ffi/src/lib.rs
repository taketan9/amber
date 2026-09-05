//! cian's notes, for a machine that cannot run the engine.
//!
//! The window talks to `cian-server` over a pipe: a method name, a JSON
//! object, a JSON answer. **A phone gets the same conversation through a C
//! ABI** — [`amber_call`] takes those two strings and returns that answer.
//!
//! One symbol rather than one per operation, deliberately. Every function a
//! C ABI exports has to be declared again in a bridging header, matched by
//! hand, and kept in step; a second method would otherwise be a change in
//! three places, and the third is in Xcode where nothing here can check it.
//! With one door, adding an operation is a match arm and no header edit.
//!
//! **The judgement is not here.** What a title is, what an excerpt leaves
//! out, what a note is called when it is made — all of that is
//! `amber_core::note`, which the window uses too. This crate is the doorway:
//! strings in, strings out, and nothing decided on the way past. That is the
//! whole reason the notes half of cian was written in the core rather than in
//! the renderer.

use std::ffi::{c_char, CStr, CString};

/// Answer a request. Both arguments are UTF-8 C strings; the answer is a
/// JSON object the caller must hand back to [`amber_free`].
///
/// # Safety
///
/// `method` and `params` must be valid NUL-terminated strings, or null.
/// The returned pointer is owned by the caller and is freed only by
/// [`amber_free`]; it is never null.
#[no_mangle]
pub unsafe extern "C" fn amber_call(method: *const c_char, params: *const c_char) -> *mut c_char {
    // A panic that unwinds across a C ABI is undefined behaviour, and the
    // caller here is an app that must not simply vanish. Anything that goes
    // wrong comes back as an error the phone can show.
    let answer = std::panic::catch_unwind(|| {
        let method = unsafe { cstr(method) };
        let params = unsafe { cstr(params) };
        let params: serde_json::Value = if params.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(&params) {
                Ok(v) => v,
                Err(e) => return err(format!("params は JSON ではありません: {e}")),
            }
        };
        match amber_core::api::call(&method, &params) {
            Ok(v) => v,
            Err(e) => err(format!("{e:#}")),
        }
    })
    .unwrap_or_else(|_| err("amber が内部で落ちました".into()));
    into_c(answer)
}

/// Give back a string [`amber_call`] returned.
///
/// # Safety
///
/// `p` must be a pointer this library returned and has not already been
/// given back. Null is accepted and does nothing.
#[no_mangle]
pub unsafe extern "C" fn amber_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn into_c(v: serde_json::Value) -> *mut c_char {
    let text = v.to_string();
    // A NUL inside would truncate the answer at the C boundary. It cannot
    // happen — `serde_json` escapes it — but the fallback says so rather than
    // handing back a silently shortened object.
    CString::new(text)
        .unwrap_or_else(|_| CString::new(r#"{"error":"答えに NUL が入りました"}"#).unwrap())
        .into_raw()
}

fn err(why: String) -> serde_json::Value {
    serde_json::json!({ "error": why })
}

// 扉そのものを試す。**中身の判断は `amber_core::api` のテストが見ている** ──
// ここで見るのは「C の境界を越えても壊れないか」だけ。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_request_is_an_answer_and_not_a_crash() {
        assert!(amber_core::api::call("いない", &serde_json::json!({})).is_err());
        // Through the real door, an error is JSON like anything else — an app
        // that got a null here would have no way to say what went wrong.
        let m = CString::new("いない").unwrap();
        let p = CString::new("{}").unwrap();
        let out = unsafe { amber_call(m.as_ptr(), p.as_ptr()) };
        assert!(!out.is_null());
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { amber_free(out) };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["error"].as_str().unwrap().contains("知らない操作"), "{text}");
    }

    #[test]
    fn null_and_nonsense_do_not_take_the_app_down_with_them() {
        // Swift can hand over a null pointer, and it must not be the last
        // thing the app ever does.
        let out = unsafe { amber_call(std::ptr::null(), std::ptr::null()) };
        assert!(!out.is_null());
        unsafe { amber_free(out) };

        let m = CString::new("notes").unwrap();
        let p = CString::new("{ これは JSON ではない").unwrap();
        let out = unsafe { amber_call(m.as_ptr(), p.as_ptr()) };
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { amber_free(out) };
        assert!(text.contains("JSON ではありません"), "{text}");

        // Freeing null is allowed, because the caller's error path will.
        unsafe { amber_free(std::ptr::null_mut()) };
    }
}
