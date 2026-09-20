//! iPhone からノートの機能を呼ぶための入口。
//!
//! デスクトップ版は `amber-server` をパイプ越しに呼ぶ。操作名と JSON を渡すと
//! JSON が返る。iPhone も同じやり取りで、経路が C の ABI になるだけ。
//!
//! 公開する関数は 1 つだけにしてある。C の ABI で公開すると、ブリッジング
//! ヘッダーにも同じ宣言が要り、手で突き合わせて保守することになる。関数を
//! 増やせば操作 1 つの追加が 3 か所の修正になり、3 か所目は Xcode の中で、
//! ここからは確認できない。入口が 1 つなら match の分岐を足すだけで済む。
//!
//! 判断はここにはない。タイトルの決め方も抜粋の切り方も `amber_core::note`
//! にあり、デスクトップ版も同じコードを使う。この crate は文字列を受け渡す
//! だけ。

use std::ffi::{c_char, CStr, CString};

/// リクエストに答える。引数はどちらも UTF-8 の C 文字列で、返り値は JSON。
/// 呼び出し側が [`amber_free`] で解放する。
///
/// # Safety
///
/// `method` と `params` は NUL 終端の文字列か null。返したポインタの所有権は
/// 呼び出し側に移り、解放できるのは [`amber_free`] だけ。null は返さない。
#[no_mangle]
pub unsafe extern "C" fn amber_call(method: *const c_char, params: *const c_char) -> *mut c_char {
    // C の ABI を越えて panic が巻き戻るのは未定義動作で、呼び出し元は黙って
    // 終了してはいけないアプリ。異常は iPhone 側が表示できるエラーにして返す。
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

/// [`amber_call`] が返した文字列を解放する。
///
/// # Safety
///
/// `p` はこのライブラリが返した、まだ解放していないポインタ。null を渡しても
/// よく、その場合は何もしない（呼び出し側のエラー処理がそこを通る）。
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
    // 文字列の途中に NUL があると C の境界で答えが切れる。`serde_json` が
    // エスケープするので起こらないが、起きたときに黙って短い JSON を返さない。
    CString::new(text)
        .unwrap_or_else(|_| CString::new(r#"{"error":"答えに NUL が入りました"}"#).unwrap())
        .into_raw()
}

fn err(why: String) -> serde_json::Value {
    serde_json::json!({ "error": why })
}

// 入口そのものの試験。中身の判断は `amber_core::api` の試験が見ている。
// ここで確認するのは、C の境界を越えても壊れないかだけ。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn おかしなリクエストは_クラッシュではなく答えで返る() {
        assert!(amber_core::api::call("いない", &serde_json::json!({})).is_err());
        // 本物の入口を通ればエラーも普通の JSON。ここで null を受け取った
        // アプリには、何が起きたのか伝える手段がない。
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
    fn null_や壊れた_json_でアプリを道連れにしない() {
        // Swift は null ポインタを渡しうる。それがアプリの最期になってはいけない。
        let out = unsafe { amber_call(std::ptr::null(), std::ptr::null()) };
        assert!(!out.is_null());
        unsafe { amber_free(out) };

        let m = CString::new("notes").unwrap();
        let p = CString::new("{ これは JSON ではない").unwrap();
        let out = unsafe { amber_call(m.as_ptr(), p.as_ptr()) };
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { amber_free(out) };
        assert!(text.contains("JSON ではありません"), "{text}");

        // null の解放を許すのは、呼び出し側のエラー処理がそうするため。
        unsafe { amber_free(std::ptr::null_mut()) };
    }
}
