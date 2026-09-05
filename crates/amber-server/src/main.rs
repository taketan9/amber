//! amber の窓が話す相手。
//!
//! **扉であって、判断ではない。** 訊かれたことは `amber_core::api::call` が
//! 答える ── iPhone の C ABI（`amber-ffi`）が呼ぶのと同じ一枚。ここがやるのは
//! 行を JSON に直し、答えを行に戻すことだけ。
//!
//! 約束（cian の窓とエンジンが交わしているものと同じ形）:
//!
//! * 一行に JSON オブジェクト一つ。行きは `{"id":1,"method":"notes","params":{…}}`
//! * 返りは `{"id":1,"ok":{…}}` か `{"id":1,"error":"…"}`
//! * **`id` を必ず返す。** 返さないと、窓側の約束が永久に解けない ── 何本も
//!   同時に飛ぶので、返る順は当てにできない
//!
//! 標準出力はこの会話専用。**`println!` でものを言わない** ── 混ざった瞬間に
//! 窓は「エンジンが壊れた」としか言えなくなる。困りごとは標準エラーへ。

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
        // 書けないなら相手が居ない。黙って終わる ── 窓が閉じただけで、
        // 事故ではない。
        if writeln!(out, "{answer}").is_err() || out.flush().is_err() {
            break;
        }
    }
}

/// 一行に、一行で答える。**ここから先へ panic を出さない。**
fn answer(line: &str) -> serde_json::Value {
    let msg: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        // id が読めていないので誰の答えにもならないが、黙って捨てるより
        // 言ったほうがいい。窓側は id の無い行を記録に回す。
        Err(e) => return serde_json::json!({ "error": format!("行が JSON ではありません: {e}") }),
    };
    let id = msg["id"].clone();
    let method = msg["method"].as_str().unwrap_or("").to_string();
    let params = if msg["params"].is_object() {
        msg["params"].clone()
    } else {
        serde_json::json!({})
    };
    // 一つの呼び出しが落ちても、窓との会話は続く。**エンジンが死ぬと、
    // 開いていたノートごと消える**（未保存の文字も含めて）。
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
        assert_eq!(a["ok"]["desktop"], true, "窓は机の上に居る");
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
        // 窓が `params` を省くことは実際にある（引数の無い操作）。
        let a = answer(r#"{"id":1,"method":"version"}"#);
        assert_eq!(a["id"], 1);
        assert!(a["error"].is_null());
    }
}
