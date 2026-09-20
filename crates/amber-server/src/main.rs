//! amber のデスクトップ版が呼び出す相手。
//!
//! 入口であって、判断ではない。リクエストには `amber_core::api::call` が答える。
//! iPhone 側の C ABI（`amber-ffi`）が呼ぶのと同じコード。ここでやるのは、行を
//! JSON に直し、答えを行に戻すことだけ。
//!
//! 取り決め（cian のデスクトップ版とエンジンが使っているものと同じ形）:
//!
//! * 1 行に JSON オブジェクト 1 つ。行きは `{"id":1,"method":"notes","params":{…}}`
//! * 返りは `{"id":1,"ok":{…}}` か `{"id":1,"error":"…"}`
//! * `id` は必ず返す。返さないと呼び出し側の Promise が解決しない。複数の
//!   リクエストが同時に飛ぶので、返る順は当てにできない
//!
//! 標準出力はこのやり取り専用で、`println!` は使わない。混ざった時点で、
//! 呼び出し側は「エンジンが壊れた」としか言えなくなる。ログは標準エラーへ。
use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = answer(&line);
        // 書き込めないなら相手がいない。黙って終了する。デスクトップ版が
        // 閉じただけで、異常ではない。
        if writeln!(out, "{answer}").is_err() || out.flush().is_err() {
            break;
        }
    }
}

/// 1 行につき 1 行で答える。ここから先へ panic を出さない。
fn answer(line: &str) -> serde_json::Value {
    let msg: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        // id が読めていないのでどのリクエストの答えにもならないが、黙って
        // 捨てるより返したほうがいい。呼び出し側は id のない行をログに回す。
        Err(e) => return serde_json::json!({ "error": format!("行が JSON ではありません: {e}") }),
    };
    let id = msg["id"].clone();
    let method = msg["method"].as_str().unwrap_or("").to_string();
    let params = if msg["params"].is_object() {
        msg["params"].clone()
    } else {
        serde_json::json!({})
    };
    // 1 つの呼び出しが落ちても、やり取りは続ける。エンジンが終了すると、
    // 開いていたノートごと消える（未保存の文字も含めて）。
    let got = std::panic::catch_unwind(|| amber_core::api::call(&method, &params));
    match got {
        Ok(Ok(v)) => serde_json::json!({ "id": id, "ok": v }),
        Ok(Err(e)) => serde_json::json!({ "id": id, "error": format!("{e:#}") }),
        Err(_) => serde_json::json!({ "id": id, "error": "amber が内部で落ちました" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 答えは必ずidを持って帰る() {
        let a = answer(r#"{"id":7,"method":"version","params":{}}"#);
        assert_eq!(a["id"], 7);
        assert!(a["ok"]["amber"].is_string());
        assert_eq!(a["ok"]["desktop"], true, "デスクトップ版として動いている");
    }

    #[test]
    fn 知らない操作でも_id_つきの_error_で返る() {
        let a = answer(r#"{"id":9,"method":"いない"}"#);
        assert_eq!(a["id"], 9);
        assert!(a["error"].as_str().unwrap().contains("いない"));
        assert!(a["ok"].is_null(), "error と ok が同時に立ってはいけない");
    }

    #[test]
    fn 壊れた行でも落ちない() {
        let a = answer("{ これは JSON ではない");
        assert!(a["error"].is_string());
    }

    #[test]
    fn params_が無くても_空の_object_として扱う() {
        // デスクトップ版が `params` を省くことは実際にある（引数のない操作）。
        let a = answer(r#"{"id":1,"method":"version"}"#);
        assert_eq!(a["id"], 1);
        assert!(a["error"].is_null());
    }
}
