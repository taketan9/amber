'use strict';
// Google カレンダーとの繋ぎ ── **グループカレンダーを 1 つ作るところだけ。**
//
// 予定の読み書きはここを通らない。端末のカレンダー（EventKit・`amber-cal`）が
// やる ── amber が作ったカレンダーは、Google アカウントの繋がっている Mac と
// iPhone に降りてくる（2026-09-13 に本人の実機で確かめた）。だから**通信が要る
// のは「作る」ときと「招待する」ときだけ**で、毎日の読み書きは通信ゼロのまま。
//
// # スコープは `calendar.app.created` 一つ
//
// 「アプリが自分で作った二次カレンダーだけを作成・閲覧・変更・削除」──
// Drive の `drive.file` とまったく同じ形で、**本人のメインのカレンダーには
// 指一本触れない**。本人のコンソールで区分を読んだ（2026-09-13）:
// `calendar.app.created` は**非機密**（審査が要らない）。
//
// # 招待だけは、このパスを通らない
//
// 人を招待する口（`Acl: insert`）には `calendar.acls` が要り、そちらは
// **機密**（審査が要る）。当画面は**そのカレンダーの共有設定のページを開いて、
// 人にメールアドレスを打ってもらう**（本人が決めた・2026-09-13）。一般公開の
// ときは審査を通して amber の中で完結させる。
//
// # キーは Drive と同じ一本
//
// `createDrive` の `token()` をそのまま渡す。**サインインを二度させない** ──
// 使う人から見れば「Google に繋ぐ」は一回のはず。カレンダーの許可だけは、
// **要る瞬間に**足しにいく（`drive.signIn({ scope })`）。

const CAL_API = 'https://www.googleapis.com/calendar/v3';

/// カレンダーの設定ページ。**人がここでグループの人を招待する。**
///
/// Google はこの URL にカレンダーの id を base64 で埋める（余りの `=` は落とす）。
/// **本人の本物のカレンダーで確かめた**（2026-09-13）── 組み立てた文字と、Google が
/// 出す URL が一字一句同じだった。id が無いときのために設定の入口も持っておく。
const SETTINGS_URL = 'https://calendar.google.com/calendar/u/0/r/settings';

function shareUrl(id) {
    if (!id) return SETTINGS_URL;
    const tag = Buffer.from(String(id), 'utf8').toString('base64').replace(/=+$/, '');
    return SETTINGS_URL + '/calendar/' + encodeURIComponent(tag);
}

/// 繋ぎを作る。**キーの出どころは外から渡す**（Drive と同じ一本／試験では偽物）。
///
/// - `token()`  ── いま使える鍵（無ければ null）
/// - `fetch`    ── 既定は Node の fetch
/// - `apiUrl`   ── 試験で偽の Google を立てるため
function createCal(opts) {
    const {
        token,
        fetch: doFetch = globalThis.fetch,
        apiUrl = CAL_API,
    } = opts;

    /// カレンダーの API を一つ叩く。キーが無ければ、**人の言葉で**断る。
    async function api(pathAndQuery, init = {}) {
        const access = await token();
        if (!access) {
            throw new Error('Google へのサインインが切れています。⚙ の「同期」からサインインし直してください');
        }
        const r = await doFetch(apiUrl + pathAndQuery, {
            ...init,
            headers: { ...(init.headers || {}), authorization: 'Bearer ' + access },
        });
        if (r.status === 204) return null;
        const text = await r.text();
        if (!r.ok) {
            let why = 'HTTP ' + r.status;
            try { why = JSON.parse(text).error.message || why; } catch { /* 文字のまま */ }
            // **許可が足りないときは、そう言う。** 「HTTP 403」では、もう一度
            // サインインすれば直ることが人には分からない。
            if (r.status === 401 || r.status === 403) {
                throw new Error('カレンダーへのアクセスが許可されていません（' + why + '）');
            }
            throw new Error(why);
        }
        return text ? JSON.parse(text) : null;
    }

    /// グループカレンダーを 1 つ作る。返すのは `{ id, name }`。
    ///
    /// **同じ名前でも、作れば別のカレンダーになる。** 2 つできるとグループが二手に
    /// 分かれるので、**作ったかどうかを憶えるのは呼ぶ側**（設定に id を置く）。
    /// ここで名前から探し直さないのは、`calendar.app.created` に一覧を読む力が
    /// 無いから ── 探せるふりをしない。
    async function make(name) {
        const got = await api('/calendars', {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ summary: name }),
        });
        return { id: got.id, name: got.summary || name };
    }

    /// そのカレンダーが、まだ向こうにあるか。**消されていたら null。**
    ///
    /// 人が Google の画面で消すことはある ── そのとき amber が憶えている id は
    /// 宙に浮く。黙って空の月を出すより、「もうありません」と言えるように。
    async function get(id) {
        try {
            const got = await api('/calendars/' + encodeURIComponent(id));
            return { id: got.id, name: got.summary || '' };
        } catch (e) {
            if (/HTTP 404|Not Found/i.test(e.message)) return null;
            throw e;
        }
    }

    /// 名前を変える。
    async function rename(id, name) {
        await api('/calendars/' + encodeURIComponent(id), {
            method: 'PATCH',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ summary: name }),
        });
        return { ok: true };
    }

    /// 消す。**呼ぶ側は、押す前に「グループの人からも消えます」と言うこと** ──
    /// グループカレンダーは持ち主が消すと全員から消える。
    ///
    /// **もう向こうに無ければ、消し終わっている**（依頼 536）。人が Google の
    /// 画面で先に消していることはある ── そこで「Not Found」と言って止まると、
    /// **amber の憶えだけが永久に外せなくなる**。`gone` で、もう無かったと言う。
    async function drop(id) {
        try {
            await api('/calendars/' + encodeURIComponent(id), { method: 'DELETE' });
            return { ok: true };
        } catch (e) {
            if (/HTTP 404|HTTP 410|Not Found/i.test(e.message)) return { ok: true, gone: true };
            throw e;
        }
    }

    return { make, get, rename, drop, shareUrl, api };
}

module.exports = { createCal, shareUrl, SETTINGS_URL, CAL_API };
